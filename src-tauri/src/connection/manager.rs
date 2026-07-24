use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use mongodb::{
    options::{AuthMechanism, ClientOptions, Credential, Tls, TlsOptions},
    Client,
};

use crate::connection::config::ConnectionConfig;
use crate::connection::ssh_tunnel::{SshAuth, SshTunnel};
use crate::error::AppError;

/// 客户端证书 (cert+key 合一的 PEM) 的解析结果.
/// MongoDB 驱动的 `tlsCertificateKeyFile` 要一个同时含证书和私钥的 PEM 文件;
/// UI 里 cert / key 是分开两栏, 因此:
///   - 只填了 cert (通常本身就是合并 PEM) -> 直接用;
///   - cert 和 key 分开两个文件 -> 读出来拼成一个临时 PEM 再用.
fn resolve_cert_key_file(config: &ConnectionConfig) -> Option<PathBuf> {
    let cert = config.tls_cert_file.as_deref().filter(|s| !s.is_empty());
    let key = config.tls_key_file.as_deref().filter(|s| !s.is_empty());
    match (cert, key) {
        (Some(c), None) => Some(PathBuf::from(c)),
        (None, Some(k)) => Some(PathBuf::from(k)),
        (Some(c), Some(k)) if c == k => Some(PathBuf::from(c)),
        (Some(c), Some(k)) => combine_cert_key(c, k),
        (None, None) => None,
    }
}

/// 把分开的 cert / key 文件拼成一个临时 PEM (cert 在前, key 在后).
/// 文件名用 cert+key 路径 hash, 稳定复用, 重连时覆盖同一文件.
fn combine_cert_key(cert_path: &str, key_path: &str) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let cert = std::fs::read(cert_path).ok()?;
    let key = std::fs::read(key_path).ok()?;
    let mut combined = cert;
    combined.push(b'\n');
    combined.extend_from_slice(&key);

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    cert_path.hash(&mut hasher);
    key_path.hash(&mut hasher);
    let path = std::env::temp_dir().join(format!("mongopilot-tls-{:x}.pem", hasher.finish()));
    std::fs::write(&path, combined).ok()?;
    Some(path)
}

/// 把 config 里的认证机制 / TLS 证书应用到已 parse 的 ClientOptions 上.
/// `build_uri` 只会拼出 `user:pass` 形式的密码认证 + `tls=true`, 这里补齐:
///   - x509 / ldap 认证机制 (URI 层拼不出, 需要显式 Credential);
///   - TLS 的 CA 文件 / 客户端证书 / 跳过校验 (URI 塞 Windows 路径易踩坑, 走结构体).
fn apply_auth_and_tls(opts: &mut ClientOptions, config: &ConnectionConfig) {
    // ---- TLS ----
    if config.tls {
        let mut tls = TlsOptions::builder().build();
        if let Some(ca) = config.tls_ca_file.as_deref().filter(|s| !s.is_empty()) {
            tls.ca_file_path = Some(PathBuf::from(ca));
        }
        if let Some(cert_key) = resolve_cert_key_file(config) {
            tls.cert_key_file_path = Some(cert_key);
        }
        if config.tls_allow_invalid {
            // 当前 TLS 后端只暴露 allow_invalid_certificates (它已放行自签/过期/主机名不匹配)
            tls.allow_invalid_certificates = Some(true);
        }
        opts.tls = Some(Tls::Enabled(tls));
    }

    // ---- 认证机制 ----
    // x509: 证书即身份, 无密码, authSource 必须是 $external, 用户名可省 (从证书 subject 取).
    // ldap: PLAIN 机制, 明文用户名/密码走 $external.
    match config.auth_type.as_str() {
        "x509" => {
            opts.credential = Some(
                Credential::builder()
                    .username(config.username.clone())
                    .source("$external".to_string())
                    .mechanism(AuthMechanism::MongoDbX509)
                    .build(),
            );
        }
        "ldap" => {
            opts.credential = Some(
                Credential::builder()
                    .username(config.username.clone())
                    .password(config.password.clone())
                    .source("$external".to_string())
                    .mechanism(AuthMechanism::Plain)
                    .build(),
            );
        }
        _ => {}
    }
}

/// 持有活跃的 MongoDB 客户端连接，以连接配置 ID 为键。
pub struct ConnectionManager {
    clients: Arc<RwLock<HashMap<String, Client>>>,
    tunnels: Arc<RwLock<HashMap<String, SshTunnel>>>,
    read_only: Arc<RwLock<HashMap<String, bool>>>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            tunnels: Arc::new(RwLock::new(HashMap::new())),
            read_only: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 把 MongoDB driver 的错误翻译成中文可读提示, 不认识的就原样回 AppError::Mongo
    fn classify_mongo_error(err: mongodb::error::Error) -> AppError {
        let s = err.to_string();
        if s.contains("Authentication failed")
            || s.contains("AuthenticationFailed")
            || s.contains("SCRAM failure")
            || s.contains("code: 18")
        {
            return AppError::Connection(format!(
                "认证失败: 请检查 1) 用户名/密码 2) 认证数据库 (大多数账号挂在 admin 库, 留空时会自动用 admin) 3) 密码中的特殊字符. 原始错误: {s}"
            ));
        }
        AppError::Mongo(err)
    }

    /// 检查连接是否为只读
    pub async fn is_read_only(&self, id: &str) -> bool {
        let map = self.read_only.read().await;
        *map.get(id).unwrap_or(&false)
    }

    /// 更新只读状态（保存配置后调用）
    pub async fn update_read_only(&self, id: &str, val: bool) {
        let mut map = self.read_only.write().await;
        map.insert(id.to_string(), val);
    }

    /// 如果启用了 SSH，先建立隧道，返回本地转发地址；否则返回 None
    async fn ensure_tunnel(&self, config: &ConnectionConfig) -> Result<Option<u16>, AppError> {
        if !config.ssh_enabled {
            return Ok(None);
        }

        let ssh_host = config.ssh_host.as_deref()
            .ok_or_else(|| AppError::Connection("SSH 主机地址未设置".into()))?;
        let ssh_port = config.ssh_port.unwrap_or(22);
        let ssh_username = config.ssh_username.as_deref()
            .ok_or_else(|| AppError::Connection("SSH 用户名未设置".into()))?;

        let ssh_auth = match config.ssh_auth_type.as_deref().unwrap_or("password") {
            "privateKey" => {
                let key_path = config.ssh_private_key.as_deref()
                    .ok_or_else(|| AppError::Connection("SSH 私钥路径未设置".into()))?;
                SshAuth::PrivateKey {
                    key_path: key_path.to_string(),
                    passphrase: config.ssh_passphrase.clone(),
                }
            }
            _ => {
                let password = config.ssh_password.as_deref()
                    .ok_or_else(|| AppError::Connection("SSH 密码未设置".into()))?;
                SshAuth::Password(password.to_string())
            }
        };

        // MongoDB 目标地址（SSH 隧道转发到的远端）
        let remote_host = &config.host;
        let remote_port = config.port;

        let tunnel = SshTunnel::start(
            ssh_host, ssh_port, ssh_username, ssh_auth,
            remote_host, remote_port,
        ).await?;

        let local_port = tunnel.local_port;

        let mut tunnels = self.tunnels.write().await;
        tunnels.insert(config.id.clone(), tunnel);

        Ok(Some(local_port))
    }

    /// 构建 MongoDB URI，如果有隧道则替换 host:port 为 localhost:localPort
    fn build_uri(config: &ConnectionConfig, local_port: Option<u16>) -> String {
        if let Some(port) = local_port {
            // SSH 隧道模式：连接到 localhost:localPort
            let mut tunneled = config.clone();
            tunneled.host = "127.0.0.1".to_string();
            tunneled.port = port;
            tunneled.srv = false; // SRV 不兼容隧道
            tunneled.to_driver_uri()
        } else {
            config.to_driver_uri()
        }
    }

    /// 打开连接并存储 Client 以复用。
    pub async fn connect(&self, config: &ConnectionConfig) -> Result<(), AppError> {
        let local_port = self.ensure_tunnel(config).await?;
        let uri = Self::build_uri(config, local_port);

        let mut opts = ClientOptions::parse(&uri)
            .await
            .map_err(AppError::Mongo)?;

        opts.app_name = Some("MongoPilot".to_string());
        opts.connect_timeout = Some(std::time::Duration::from_secs(10));
        opts.server_selection_timeout = Some(std::time::Duration::from_secs(10));
        // 连接池保活：防止空闲连接被服务器断开
        opts.heartbeat_freq = Some(std::time::Duration::from_secs(30));
        opts.max_idle_time = Some(std::time::Duration::from_secs(300));
        opts.min_pool_size = Some(1);
        opts.max_pool_size = Some(10);
        opts.retry_writes = Some(true);
        opts.retry_reads = Some(true);

        // 强制单节点模式: 除非用户在 URI 里显式配置了 replicaSet,
        // 否则一律走 directConnection=true, 避免 driver 拓扑探测/SDAM 把
        // 流量 fan-out 到多个 host, 也能规避某些服务端回包错位导致
        // CommandNotFound 之类 wire protocol 错位问题.
        if opts.repl_set_name.is_none() {
            opts.direct_connection = Some(true);
        }

        apply_auth_and_tls(&mut opts, config);

        let client = Client::with_options(opts).map_err(AppError::Mongo)?;

        // 通过 ping 验证连通性
        client
            .database("admin")
            .run_command(mongodb::bson::doc! { "ping": 1 })
            .await
            .map_err(Self::classify_mongo_error)?;

        let mut clients = self.clients.write().await;
        clients.insert(config.id.clone(), client);

        let mut ro = self.read_only.write().await;
        ro.insert(config.id.clone(), config.read_only);

        Ok(())
    }

    /// 测试连接但不存储。
    pub async fn test_connection(
        &self,
        config: &ConnectionConfig,
    ) -> Result<ServerInfo, AppError> {
        let local_port = self.ensure_tunnel(config).await?;
        let uri = Self::build_uri(config, local_port);

        let mut opts = ClientOptions::parse(&uri)
            .await
            .map_err(AppError::Mongo)?;

        opts.app_name = Some("MongoPilot".to_string());
        opts.connect_timeout = Some(std::time::Duration::from_secs(10));
        opts.server_selection_timeout = Some(std::time::Duration::from_secs(10));
        opts.retry_writes = Some(true);
        opts.retry_reads = Some(true);

        if opts.repl_set_name.is_none() {
            opts.direct_connection = Some(true);
        }

        apply_auth_and_tls(&mut opts, config);

        let client = Client::with_options(opts).map_err(AppError::Mongo)?;

        let result = client
            .database("admin")
            .run_command(mongodb::bson::doc! { "buildInfo": 1 })
            .await
            .map_err(Self::classify_mongo_error)?;

        let version = result
            .get_str("version")
            .unwrap_or("unknown")
            .to_string();

        // `hello` 是 MongoDB 4.4.1 才加的; 4.2.x 及更老只有 `isMaster`.
        // 先试 hello, 服务器返回 CommandNotFound 时退回 isMaster.
        let admin = client.database("admin");
        let info_doc: mongodb::bson::Document = match admin
            .run_command(mongodb::bson::doc! { "hello": 1 })
            .await
        {
            Ok(d) => d,
            Err(e) => {
                let s = e.to_string();
                if s.contains("CommandNotFound") || s.contains("no such command") {
                    admin
                        .run_command(mongodb::bson::doc! { "isMaster": 1 })
                        .await
                        .map_err(Self::classify_mongo_error)?
                } else {
                    return Err(Self::classify_mongo_error(e));
                }
            }
        };

        // hello 返回 isWritablePrimary, isMaster 返回 ismaster —— 两边都兼容
        let is_primary = info_doc
            .get_bool("isWritablePrimary")
            .or_else(|_| info_doc.get_bool("ismaster"))
            .unwrap_or(false);
        let set_name = info_doc
            .get_str("setName")
            .ok()
            .map(|s: &str| s.to_string());

        let topology = if set_name.is_some() {
            if is_primary { "Primary" } else { "Secondary" }
        } else {
            "Standalone"
        };

        // 测试连接后清理隧道
        if local_port.is_some() {
            let mut tunnels = self.tunnels.write().await;
            tunnels.remove(&config.id);
        }

        Ok(ServerInfo {
            version,
            topology: topology.to_string(),
            replica_set: set_name,
        })
    }

    /// 断开并移除客户端和隧道。
    pub async fn disconnect(&self, id: &str) {
        let mut clients = self.clients.write().await;
        clients.remove(id);

        let mut tunnels = self.tunnels.write().await;
        if let Some(tunnel) = tunnels.remove(id) {
            tunnel.close();
        }

        let mut ro = self.read_only.write().await;
        ro.remove(id);
    }

    /// 获取活跃客户端的引用。
    pub async fn get_client(&self, id: &str) -> Result<Client, AppError> {
        let clients = self.clients.read().await;
        clients
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::Connection(format!("{id} 没有活跃连接")))
    }

    /// 列出所有活跃连接的 ID。
    pub async fn active_ids(&self) -> Vec<String> {
        let clients = self.clients.read().await;
        clients.keys().cloned().collect()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub version: String,
    pub topology: String,
    pub replica_set: Option<String>,
}
