use std::path::Path;
use std::sync::OnceLock;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::RngCore;

use crate::error::AppError;

static KEY: OnceLock<String> = OnceLock::new();

const KEY_FILE_NAME: &str = ".encryption.key";
const KEY_BYTES: usize = 32;

pub fn initialize(app_data_dir: &Path) -> Result<(), AppError> {
    if KEY.get().is_some() {
        return Ok(());
    }

    std::fs::create_dir_all(app_data_dir)
        .map_err(|e| AppError::Crypto(format!("无法创建数据目录: {e}")))?;

    let key_path = app_data_dir.join(KEY_FILE_NAME);
    let key = if key_path.exists() {
        let contents = std::fs::read_to_string(&key_path)
            .map_err(|e| AppError::Crypto(format!("无法读取密钥文件: {e}")))?;
        contents.trim().to_string()
    } else {
        let mut buf = [0u8; KEY_BYTES];
        rand::rngs::OsRng.fill_bytes(&mut buf);
        let encoded = BASE64.encode(buf);
        std::fs::write(&key_path, &encoded)
            .map_err(|e| AppError::Crypto(format!("无法写入密钥文件: {e}")))?;
        set_file_permissions(&key_path);
        encoded
    };

    KEY.set(key)
        .map_err(|_| AppError::Crypto("encryption key already initialized".into()))?;
    Ok(())
}

pub fn key() -> &'static str {
    KEY.get()
        .expect("encryption key not initialized — call key_store::initialize() at startup")
        .as_str()
}

#[cfg(unix)]
fn set_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_file_permissions(_path: &Path) {}
