use serde::{Deserialize, Serialize};
use crate::error::AppError;

fn uuid_v4() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        u16::from_be_bytes([bytes[4], bytes[5]]),
        u16::from_be_bytes([bytes[6], bytes[7]]) & 0x0fff,
        (u16::from_be_bytes([bytes[8], bytes[9]]) & 0x3fff) | 0x8000,
        {
            let hi = u32::from_be_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]) as u64;
            let lo = u16::from_be_bytes([bytes[14], bytes[15]]) as u64;
            (hi << 16) | lo
        }
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionConfig {
    pub id: String,
    pub name: String,
    pub group_path: String,
    pub color: Option<String>,
    pub conn_type: String,
    pub host: String,
    pub port: u16,
    pub auth_type: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub auth_db: Option<String>,
    pub replica_set: Option<String>,
    pub srv: bool,
    pub tls: bool,
    pub tls_ca_file: Option<String>,
    pub tls_cert_file: Option<String>,
    pub tls_key_file: Option<String>,
    pub tls_allow_invalid: bool,
    pub ssh_enabled: bool,
    pub ssh_host: Option<String>,
    pub ssh_port: Option<u16>,
    pub ssh_username: Option<String>,
    pub ssh_auth_type: Option<String>,
    pub ssh_password: Option<String>,
    pub ssh_private_key: Option<String>,
    pub ssh_passphrase: Option<String>,
    pub default_db: Option<String>,
    pub uri_options: Option<String>,
    pub read_only: bool,
    pub sort_order: i64,
}

impl ConnectionConfig {
    /// Build a driver-only URI (no ext.ssh.* params) for MongoDB driver connection.
    pub fn to_driver_uri(&self) -> String {
        self.build_uri(false)
    }

    /// Build a full URI including ext.ssh.* params for export/display.
    pub fn to_uri(&self) -> String {
        self.build_uri(true)
    }

    fn build_uri(&self, include_ssh_ext: bool) -> String {
        let scheme = if self.srv { "mongodb+srv" } else { "mongodb" };

        // Build authority (user:pass@host:port)
        let auth = if self.auth_type != "none" {
            if let (Some(user), Some(pass)) = (&self.username, &self.password) {
                let enc_user = urlencoding::encode(user).into_owned();
                let enc_pass = urlencoding::encode(pass).into_owned();
                format!("{}:{}@", enc_user, enc_pass)
            } else if let Some(user) = &self.username {
                let enc_user = urlencoding::encode(user).into_owned();
                format!("{}@", enc_user)
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let host_port = if self.srv {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        };

        let db_path = self
            .default_db
            .as_deref()
            .map(|d| format!("/{}", d))
            .unwrap_or_else(|| "/".to_string());

        // Build query params
        let mut params: Vec<String> = Vec::new();

        // 认证数据库: 用户没填且开了密码认证就默认 admin (mongosh 标准行为).
        // 99% 的 MongoDB 用户挂在 admin 库, 不显式声明 authSource 时会拿 path 里的库去找用户, 多半失败.
        let effective_auth_db: Option<String> = match &self.auth_db {
            Some(d) if !d.is_empty() => Some(d.clone()),
            _ if self.auth_type != "none" && self.username.is_some() => Some("admin".to_string()),
            _ => None,
        };
        if let Some(auth_db) = &effective_auth_db {
            params.push(format!("authSource={}", auth_db));
        }

        if let Some(rs) = &self.replica_set {
            params.push(format!("replicaSet={}", rs));
        }

        if self.tls {
            params.push("tls=true".to_string());
        }

        // SSH extension parameters (only for export, not for driver)
        if include_ssh_ext && self.ssh_enabled {
            if let Some(ssh_host) = &self.ssh_host {
                let ssh_port = self.ssh_port.unwrap_or(22);
                params.push(format!(
                    "ext.ssh.server={}",
                    urlencoding::encode(&format!("{ssh_host}:{ssh_port}"))
                ));
            }
            if let Some(ssh_user) = &self.ssh_username {
                params.push(format!("ext.ssh.username={}", urlencoding::encode(ssh_user)));
            }
            if let Some(ssh_key) = &self.ssh_private_key {
                params.push(format!("ext.ssh.keyPath={}", urlencoding::encode(ssh_key)));
            }
            if let Some(ssh_pass) = &self.ssh_password {
                params.push(format!("ext.ssh.password={}", urlencoding::encode(ssh_pass)));
            }
            if let Some(ssh_phrase) = &self.ssh_passphrase {
                params.push(format!("ext.ssh.passphrase={}", urlencoding::encode(ssh_phrase)));
            }
        }

        if let Some(extra) = &self.uri_options {
            if !extra.is_empty() {
                params.push(extra.clone());
            }
        }

        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };

        format!("{}://{}{}{}{}", scheme, auth, host_port, db_path, query)
    }

    /// Parse a MongoDB URI string into a ConnectionConfig.
    pub fn from_uri(uri: &str) -> Result<Self, AppError> {
        // Determine scheme
        let (srv, rest) = if let Some(r) = uri.strip_prefix("mongodb+srv://") {
            (true, r)
        } else if let Some(r) = uri.strip_prefix("mongodb://") {
            (false, r)
        } else {
            return Err(AppError::InvalidInput(
                "URI 必须以 mongodb:// 或 mongodb+srv:// 开头".to_string(),
            ));
        };

        // Split authority+path from query
        let (before_query, query_str) = match rest.split_once('?') {
            Some((b, q)) => (b, q),
            None => (rest, ""),
        };

        // Split path from authority
        let (authority, path) = match before_query.split_once('/') {
            Some((a, p)) => (a, p),
            None => (before_query, ""),
        };

        // Parse auth and host from authority
        let (auth_part, host_part) = match authority.rsplit_once('@') {
            Some((a, h)) => (Some(a), h),
            None => (None, authority),
        };

        let (username, password) = if let Some(auth) = auth_part {
            match auth.split_once(':') {
                Some((u, p)) => {
                    let decoded_u = urlencoding::decode(u)
                        .map_err(|e| AppError::InvalidInput(e.to_string()))?
                        .into_owned();
                    let decoded_p = urlencoding::decode(p)
                        .map_err(|e| AppError::InvalidInput(e.to_string()))?
                        .into_owned();
                    (Some(decoded_u), Some(decoded_p))
                }
                None => {
                    let decoded_u = urlencoding::decode(auth)
                        .map_err(|e| AppError::InvalidInput(e.to_string()))?
                        .into_owned();
                    (Some(decoded_u), None)
                }
            }
        } else {
            (None, None)
        };

        let auth_type = if username.is_some() { "password" } else { "none" }.to_string();

        // Parse host and port
        let (host, port) = if srv {
            (host_part.to_string(), 27017u16)
        } else {
            match host_part.rsplit_once(':') {
                Some((h, p)) => {
                    let port_num: u16 = p.parse().map_err(|_| {
                        AppError::InvalidInput(format!("无效的端口: {}", p))
                    })?;
                    (h.to_string(), port_num)
                }
                None => (host_part.to_string(), 27017u16),
            }
        };

        // Parse database from path (strip leading slash already done by split_once)
        let default_db = if path.is_empty() {
            None
        } else {
            Some(path.to_string())
        };

        // Parse query parameters
        let mut replica_set: Option<String> = None;
        let mut tls = false;
        let mut auth_db: Option<String> = None;
        let mut extra_params: Vec<String> = Vec::new();

        // SSH extension parameters (ext.ssh.*)
        let mut ssh_server: Option<String> = None;
        let mut ssh_username_param: Option<String> = None;
        let mut ssh_password_param: Option<String> = None;
        let mut ssh_key_path: Option<String> = None;
        let mut ssh_passphrase_param: Option<String> = None;

        if !query_str.is_empty() {
            for param in query_str.split('&') {
                if let Some((k, v)) = param.split_once('=') {
                    let decoded_v = urlencoding::decode(v).unwrap_or_default().into_owned();
                    match k {
                        "replicaSet" => replica_set = Some(decoded_v),
                        "tls" | "ssl" => tls = v == "true",
                        "authSource" => auth_db = Some(decoded_v),
                        "ext.ssh.server" => ssh_server = Some(decoded_v),
                        "ext.ssh.username" => ssh_username_param = Some(decoded_v),
                        "ext.ssh.password" => ssh_password_param = Some(decoded_v),
                        "ext.ssh.keyPath" => ssh_key_path = Some(decoded_v),
                        "ext.ssh.passphrase" => ssh_passphrase_param = Some(decoded_v),
                        _ => extra_params.push(param.to_string()),
                    }
                }
            }
        }

        // Parse SSH server into host:port
        let (ssh_host, ssh_port) = if let Some(server) = &ssh_server {
            match server.rsplit_once(':') {
                Some((h, p)) => (Some(h.to_string()), p.parse::<u16>().ok()),
                None => (Some(server.clone()), Some(22u16)),
            }
        } else {
            (None, None)
        };

        let ssh_enabled = ssh_host.is_some();
        let ssh_auth_type = if ssh_key_path.is_some() {
            Some("privateKey".to_string())
        } else if ssh_password_param.is_some() {
            Some("password".to_string())
        } else if ssh_enabled {
            Some("privateKey".to_string()) // default for SSH
        } else {
            None
        };

        let uri_options = if extra_params.is_empty() {
            None
        } else {
            Some(extra_params.join("&"))
        };

        // Determine connection type
        let conn_type = if replica_set.is_some() {
            "replicaset".to_string()
        } else if srv {
            "srv".to_string()
        } else {
            "standalone".to_string()
        };

        Ok(ConnectionConfig {
            id: uuid_v4(),
            name: host.clone(),
            group_path: String::new(),
            color: None,
            conn_type,
            host,
            port,
            auth_type,
            username,
            password,
            auth_db,
            replica_set,
            srv,
            tls,
            tls_ca_file: None,
            tls_cert_file: None,
            tls_key_file: None,
            tls_allow_invalid: false,
            ssh_enabled,
            ssh_host,
            ssh_port,
            ssh_username: ssh_username_param,
            ssh_auth_type,
            ssh_password: ssh_password_param,
            ssh_private_key: ssh_key_path,
            ssh_passphrase: ssh_passphrase_param,
            default_db,
            uri_options,
            read_only: false,
            sort_order: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_uri() {
        let config = ConnectionConfig::from_uri("mongodb://localhost:27017").unwrap();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 27017);
        assert!(!config.srv);
        assert_eq!(config.auth_type, "none");
    }

    #[test]
    fn parse_uri_with_auth() {
        let config =
            ConnectionConfig::from_uri("mongodb://admin:pa%40ss@myhost:27018/mydb").unwrap();
        assert_eq!(config.username.as_deref(), Some("admin"));
        assert_eq!(config.password.as_deref(), Some("pa@ss"));
        assert_eq!(config.host, "myhost");
        assert_eq!(config.port, 27018);
        assert_eq!(config.default_db.as_deref(), Some("mydb"));
        assert_eq!(config.auth_type, "password");
    }

    #[test]
    fn parse_srv_uri() {
        let config =
            ConnectionConfig::from_uri("mongodb+srv://user:pass@cluster0.abc.mongodb.net/test")
                .unwrap();
        assert!(config.srv);
        assert_eq!(config.host, "cluster0.abc.mongodb.net");
        assert_eq!(config.default_db.as_deref(), Some("test"));
    }

    #[test]
    fn parse_uri_with_options() {
        let config = ConnectionConfig::from_uri(
            "mongodb://localhost:27017/?replicaSet=rs0&tls=true&readPreference=secondary",
        )
        .unwrap();
        assert_eq!(config.replica_set.as_deref(), Some("rs0"));
        assert!(config.tls);
        assert_eq!(config.conn_type, "replicaset");
        assert_eq!(
            config.uri_options.as_deref(),
            Some("readPreference=secondary")
        );
    }

    #[test]
    fn parse_uri_with_ssh_key() {
        let config = ConnectionConfig::from_uri(
            "mongodb://dbuser:s3cr3tpassword@192.0.2.10:27017/testdb?authSource=admin&ext.ssh.server=jump.example.com%3A22&ext.ssh.username=sshuser&ext.ssh.keyPath=%2Fhome%2Fuser%2F.ssh%2Fid_rsa"
        ).unwrap();
        assert_eq!(config.host, "192.0.2.10");
        assert_eq!(config.port, 27017);
        assert_eq!(config.username.as_deref(), Some("dbuser"));
        assert_eq!(config.password.as_deref(), Some("s3cr3tpassword"));
        assert_eq!(config.default_db.as_deref(), Some("testdb"));
        assert_eq!(config.auth_db.as_deref(), Some("admin"));
        assert!(config.ssh_enabled);
        assert_eq!(config.ssh_host.as_deref(), Some("jump.example.com"));
        assert_eq!(config.ssh_port, Some(22));
        assert_eq!(config.ssh_username.as_deref(), Some("sshuser"));
        assert_eq!(config.ssh_auth_type.as_deref(), Some("privateKey"));
        assert_eq!(config.ssh_private_key.as_deref(), Some("/home/user/.ssh/id_rsa"));
    }

    #[test]
    fn parse_uri_with_ssh_password() {
        let config = ConnectionConfig::from_uri(
            "mongodb://user:pass@host:27017/db?authSource=admin&ext.ssh.server=jump.example.com%3A2222&ext.ssh.username=deploy&ext.ssh.password=sshpass123"
        ).unwrap();
        assert!(config.ssh_enabled);
        assert_eq!(config.ssh_host.as_deref(), Some("jump.example.com"));
        assert_eq!(config.ssh_port, Some(2222));
        assert_eq!(config.ssh_username.as_deref(), Some("deploy"));
        assert_eq!(config.ssh_auth_type.as_deref(), Some("password"));
        assert_eq!(config.ssh_password.as_deref(), Some("sshpass123"));
    }

    #[test]
    fn to_uri_with_ssh() {
        let config = ConnectionConfig {
            id: "test".to_string(),
            name: "test".to_string(),
            group_path: String::new(),
            color: None,
            conn_type: "standalone".to_string(),
            host: "192.0.2.10".to_string(),
            port: 27017,
            auth_type: "password".to_string(),
            username: Some("dbuser".to_string()),
            password: Some("s3cr3tpassword".to_string()),
            auth_db: Some("admin".to_string()),
            replica_set: None,
            srv: false,
            tls: false,
            tls_ca_file: None,
            tls_cert_file: None,
            tls_key_file: None,
            tls_allow_invalid: false,
            ssh_enabled: true,
            ssh_host: Some("jump.example.com".to_string()),
            ssh_port: Some(22),
            ssh_username: Some("sshuser".to_string()),
            ssh_auth_type: Some("privateKey".to_string()),
            ssh_password: None,
            ssh_private_key: Some("/home/user/.ssh/id_rsa".to_string()),
            ssh_passphrase: None,
            default_db: Some("testdb".to_string()),
            uri_options: None,
            sort_order: 0,
            read_only: false,
        };

        let uri = config.to_uri();
        assert!(uri.contains("dbuser:s3cr3tpassword@192.0.2.10:27017"));
        assert!(uri.contains("authSource=admin"));
        assert!(uri.contains("ext.ssh.server="));
        assert!(uri.contains("ext.ssh.username=sshuser"));
        assert!(uri.contains("ext.ssh.keyPath="));
    }

    #[test]
    fn invalid_uri_rejected() {
        let result = ConnectionConfig::from_uri("http://localhost");
        assert!(result.is_err());
    }

    #[test]
    fn uri_roundtrip() {
        let original = ConnectionConfig {
            id: "test".to_string(),
            name: "test".to_string(),
            group_path: String::new(),
            color: None,
            conn_type: "standalone".to_string(),
            host: "myhost".to_string(),
            port: 27018,
            auth_type: "password".to_string(),
            username: Some("admin".to_string()),
            password: Some("secret".to_string()),
            auth_db: Some("admin".to_string()),
            replica_set: None,
            srv: false,
            tls: true,
            tls_ca_file: None,
            tls_cert_file: None,
            tls_key_file: None,
            tls_allow_invalid: false,
            ssh_enabled: false,
            ssh_host: None,
            ssh_port: None,
            ssh_username: None,
            ssh_auth_type: None,
            ssh_password: None,
            ssh_private_key: None,
            ssh_passphrase: None,
            default_db: Some("testdb".to_string()),
            uri_options: None,
            sort_order: 0,
            read_only: false,
        };

        let uri = original.to_uri();
        assert!(uri.starts_with("mongodb://"));
        assert!(uri.contains("admin:secret@"));
        assert!(uri.contains("myhost:27018"));
        assert!(uri.contains("testdb"));
        assert!(uri.contains("tls=true"));
        assert!(uri.contains("authSource=admin"));
    }
}
