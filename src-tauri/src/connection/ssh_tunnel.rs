use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;

use ssh2::Session;
use tokio::net::TcpListener;

use crate::error::AppError;

/// SSH 隧道句柄
pub struct SshTunnel {
    pub local_port: u16,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl SshTunnel {
    /// 建立 SSH 隧道：本地随机端口 → SSH → remote_host:remote_port
    pub async fn start(
        ssh_host: &str,
        ssh_port: u16,
        ssh_username: &str,
        ssh_auth: SshAuth,
        remote_host: &str,
        remote_port: u16,
    ) -> Result<Self, AppError> {
        // 在本地绑定随机端口
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| AppError::Connection(format!("绑定本地端口失败: {e}")))?;
        let local_port = listener.local_addr().unwrap().port();

        // 先验证 SSH 连接是否能建立 (握手对瞬时 banner/socket 错误做有限重试)
        let ssh_addr = format!("{ssh_host}:{ssh_port}");
        let test_sess = connect_and_handshake(&ssh_addr)?;
        authenticate(&test_sess, ssh_username, &ssh_auth)?;
        drop(test_sess);

        // 启动转发循环
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let ssh_addr = Arc::new(ssh_addr);
        let username = ssh_username.to_string();
        let auth = ssh_auth;
        let rhost = remote_host.to_string();
        let rport = remote_port;

        tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;
            loop {
                tokio::select! {
                    accept = listener.accept() => {
                        match accept {
                            Ok((tcp, _)) => {
                                let ssh_addr = ssh_addr.clone();
                                let username = username.clone();
                                let auth = auth.clone();
                                let rhost = rhost.clone();

                                // 每个连接在独立的阻塞线程处理（ssh2 是同步库）
                                tokio::task::spawn_blocking(move || {
                                    if let Err(e) = handle_forward(
                                        tcp, &ssh_addr, &username, &auth, &rhost, rport,
                                    ) {
                                        tracing::warn!("SSH 转发错误: {e}");
                                    }
                                });
                            }
                            Err(e) => {
                                tracing::warn!("SSH accept 失败: {e}");
                                break;
                            }
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        break;
                    }
                }
            }
        });

        Ok(SshTunnel {
            local_port,
            shutdown: shutdown_tx,
        })
    }

    pub fn close(&self) {
        let _ = self.shutdown.send(true);
    }
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}

/// 在阻塞线程中处理单个转发连接
fn handle_forward(
    tcp: tokio::net::TcpStream,
    ssh_addr: &str,
    username: &str,
    auth: &SshAuth,
    remote_host: &str,
    remote_port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 建立 SSH 连接 (握手带瞬时故障重试, 避免连接池并发起链时偶发 banner 失败)
    let sess = connect_and_handshake(ssh_addr)
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;
    authenticate(&sess, username, auth).map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;

    // 打开 direct-tcpip channel
    let channel = sess.channel_direct_tcpip(remote_host, remote_port, None)?;

    // 转发：tokio TcpStream → 同步 std TcpStream
    let std_tcp = tcp.into_std()?;
    std_tcp.set_nodelay(true)?;
    std_tcp.set_nonblocking(false)?;
    sess.set_blocking(false);

    let mut local = std_tcp;
    let mut remote = channel;

    // 大缓冲减少 syscall 次数, 提升大批量传输吞吐
    let mut buf = [0u8; 32768];

    // 本地读设短超时: 无上行数据时最多阻塞这么久; 下行吞吐靠每轮"排空 channel"保证,
    // 不再是每 50ms 才搬一块 (原实现下行被限到 ~160KB/s, 大数据导出会被服务器监视超时中断)
    local.set_read_timeout(Some(std::time::Duration::from_millis(20)))?;

    loop {
        // remote → local: 一次性把 SSH channel 里已到达的数据全部排空, 而不是每轮只搬一块
        loop {
            match remote.read(&mut buf) {
                Ok(0) => return Ok(()), // channel EOF, 远端关闭
                Ok(n) => {
                    local.write_all(&buf[..n])?;
                }
                Err(ref e) if e.raw_os_error() == Some(-37) => break, // LIBSSH2_ERROR_EAGAIN: 暂时没有更多数据
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.into()),
            }
        }

        // local → remote
        match local.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                sess.set_blocking(true);
                remote.write_all(&buf[..n])?;
                sess.set_blocking(false);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(e.into()),
        }
    }

    Ok(())
}

/// 建立 TCP + SSH 握手, 返回已握手 (未认证) 的 Session.
/// 对 "Failed getting banner" (LIBSSH2_ERROR_SOCKET_RECV -43) 这类握手期瞬时故障
/// 换新 TCP 连接重试, 最多 3 次. TCP 连接本身失败 (服务器不可达) 不重试, 直接返回.
fn connect_and_handshake(ssh_addr: &str) -> Result<Session, AppError> {
    let addr: std::net::SocketAddr = ssh_addr
        .parse()
        .map_err(|e| AppError::Connection(format!("无效的 SSH 地址: {e}")))?;

    const MAX_ATTEMPTS: usize = 3;
    let mut last_err = String::from("SSH 握手失败");
    for attempt in 1..=MAX_ATTEMPTS {
        // TCP 连接失败 = 服务器不可达, 不重试
        let tcp = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(10))
            .map_err(|e| AppError::Connection(format!("无法连接 SSH 服务器 {ssh_addr}: {e}")))?;
        let _ = tcp.set_nodelay(true);

        let mut sess = Session::new()
            .map_err(|e| AppError::Connection(format!("创建 SSH 会话失败: {e}")))?;
        // 握手阶段设超时, 避免个别情况下 banner 迟迟不来永久挂住
        sess.set_timeout(15_000);
        sess.set_tcp_stream(tcp);

        match sess.handshake() {
            Ok(()) => {
                // 握手成功后取消超时, 不影响后续阻塞转发
                sess.set_timeout(0);
                return Ok(sess);
            }
            Err(e) => {
                last_err = format!("SSH 握手失败: {e}");
                tracing::warn!("SSH 握手第 {attempt}/{MAX_ATTEMPTS} 次失败: {e}");
                if attempt < MAX_ATTEMPTS {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                }
            }
        }
    }
    Err(AppError::Connection(last_err))
}

fn authenticate(sess: &Session, username: &str, auth: &SshAuth) -> Result<(), AppError> {
    match auth {
        SshAuth::Password(password) => {
            sess.userauth_password(username, password)
                .map_err(|e| AppError::Connection(format!("SSH 密码认证失败: {e}")))?;
        }
        SshAuth::PrivateKey { key_path, passphrase } => {
            sess.userauth_pubkey_file(
                username,
                None,
                Path::new(key_path),
                passphrase.as_deref(),
            )
            .map_err(|e| AppError::Connection(format!("SSH 密钥认证失败: {e}")))?;
        }
    }
    if !sess.authenticated() {
        return Err(AppError::Connection("SSH 认证失败".into()));
    }
    Ok(())
}

/// SSH 认证方式
#[derive(Clone)]
pub enum SshAuth {
    Password(String),
    PrivateKey {
        key_path: String,
        passphrase: Option<String>,
    },
}
