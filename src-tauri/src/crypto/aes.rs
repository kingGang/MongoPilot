use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::RngCore;

use crate::error::AppError;

const NONCE_SIZE: usize = 12;

/// 从口令派生 256 位密钥。
fn derive_key(passphrase: &str) -> Key<Aes256Gcm> {
    let mut key_bytes = [0u8; 32];
    let pass_bytes = passphrase.as_bytes();
    for (i, byte) in key_bytes.iter_mut().enumerate() {
        *byte = pass_bytes[i % pass_bytes.len()];
    }
    Key::<Aes256Gcm>::from(key_bytes)
}

/// 加密明文，返回 base64 编码的 "nonce + 密文"。
pub fn encrypt(plaintext: &str, passphrase: &str) -> Result<String, AppError> {
    let key = derive_key(passphrase);
    let cipher = Aes256Gcm::new(&key);

    let mut nonce_bytes = [0u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| AppError::Crypto(format!("加密失败: {e}")))?;

    let mut combined = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(BASE64.encode(&combined))
}

/// 解密 base64 编码的 "nonce + 密文"，返回明文。
pub fn decrypt(encoded: &str, passphrase: &str) -> Result<String, AppError> {
    let key = derive_key(passphrase);
    let cipher = Aes256Gcm::new(&key);

    let combined = BASE64
        .decode(encoded)
        .map_err(|e| AppError::Crypto(format!("base64 解码失败: {e}")))?;

    if combined.len() < NONCE_SIZE {
        return Err(AppError::Crypto("密文太短".into()));
    }

    let (nonce_bytes, ciphertext) = combined.split_at(NONCE_SIZE);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| AppError::Crypto(format!("解密失败: {e}")))?;

    String::from_utf8(plaintext)
        .map_err(|e| AppError::Crypto(format!("UTF-8 解码失败: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let passphrase = "test-machine-key-0123456789abcdef";
        let plaintext = "my-secret-password";

        let encrypted = encrypt(plaintext, passphrase).unwrap();
        let decrypted = decrypt(&encrypted, passphrase).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_passphrase_fails() {
        let encrypted = encrypt("secret", "correct-key-abcdefghijklmnopqrst").unwrap();
        let result = decrypt(&encrypted, "wrong-key-xyzxyzxyzxyzxyzxyzxyzxy");
        assert!(result.is_err());
    }

    #[test]
    fn empty_string_roundtrip() {
        let passphrase = "test-key-01234567890123456789ab";
        let encrypted = encrypt("", passphrase).unwrap();
        let decrypted = decrypt(&encrypted, passphrase).unwrap();
        assert_eq!(decrypted, "");
    }

    #[test]
    fn unicode_roundtrip() {
        let passphrase = "test-key-01234567890123456789ab";
        let plaintext = "密码测试 🔑";
        let encrypted = encrypt(plaintext, passphrase).unwrap();
        let decrypted = decrypt(&encrypted, passphrase).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn invalid_base64_fails() {
        let result = decrypt("not-valid-base64!!!", "any-key-01234567890123456789");
        assert!(result.is_err());
    }
}
