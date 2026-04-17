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

        // 先验证 SSH 连接是否能建立
        let ssh_addr = format!("{ssh_host}:{ssh_port}");
        let test_tcp = std::net::TcpStream::connect_timeout(
            &ssh_addr.parse().map_err(|e| AppError::Connection(format!("无效的 SSH 地址: {e}")))?,
            std::time::Duration::from_secs(10),
        ).map_err(|e| AppError::Connection(format!("无法连接 SSH 服务器 {ssh_addr}: {e}")))?;

        let mut test_sess = Session::new()
            .map_err(|e| AppError::Connection(format!("创建 SSH 会话失败: {e}")))?;
        test_sess.set_tcp_stream(test_tcp);
        test_sess.handshake()
            .map_err(|e| AppError::Connection(format!("SSH 握手失败: {e}")))?;

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
    // 建立 SSH 连接
    let ssh_tcp = std::net::TcpStream::connect(ssh_addr)?;
    ssh_tcp.set_nodelay(true)?;
    let mut sess = Session::new()?;
    sess.set_tcp_stream(ssh_tcp);
    sess.handshake()?;
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

    let mut buf = [0u8; 8192];

    // 设置超时，循环双向转发
    local.set_read_timeout(Some(std::time::Duration::from_millis(50)))?;

    loop {
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

        // remote → local
        match remote.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                local.write_all(&buf[..n])?;
            }
            Err(ref e) if e.raw_os_error() == Some(-37) => {} // LIBSSH2_ERROR_EAGAIN
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }
    }

    Ok(())
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
