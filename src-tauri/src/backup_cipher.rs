//! Compact backup envelope: fixed, authenticated version + salt + nonce +
//! compressed AES-GCM ciphertext. Existing JSON version-3 backups remain valid.
use super::{
    config_backup_key, validate_config_backup_passphrase, EncryptedConfigBackup,
    CONFIG_BACKUP_ITERATIONS, CONFIG_BACKUP_MAX_BYTES,
};
use aes_gcm::{
    aead::{rand_core::RngCore, Aead, OsRng, Payload},
    Aes256Gcm, KeyInit, Nonce,
};
use base64::Engine;
use std::io::{Read, Write};

const PREFIX: &str = "BJUT4:";
const AAD: &[u8] = b"BJUT-AL-CONFIG-4";

pub(super) fn encrypt(plaintext: &[u8], passphrase: &[u8]) -> Result<String, String> {
    validate_config_backup_passphrase(passphrase)?;
    if plaintext.len() > CONFIG_BACKUP_MAX_BYTES {
        return Err("配置备份内容过大".into());
    }
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(plaintext).map_err(|_| "备份压缩失败")?;
    let mut compressed = encoder.finish().map_err(|_| "备份压缩失败")?;
    let mut salt = [0; 16];
    let mut iv = [0; 12];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut iv);
    let mut key = config_backup_key(passphrase, &salt, CONFIG_BACKUP_ITERATIONS);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| "备份密钥无效")?;
    let encrypted = cipher.encrypt(
        Nonce::from_slice(&iv),
        Payload {
            msg: &compressed,
            aad: AAD,
        },
    );
    key.fill(0);
    compressed.fill(0);
    let encrypted = encrypted.map_err(|_| "配置备份加密失败")?;
    let mut packed = Vec::with_capacity(28 + encrypted.len());
    packed.extend_from_slice(&salt);
    packed.extend_from_slice(&iv);
    packed.extend(encrypted);
    Ok(format!(
        "{PREFIX}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(packed)
    ))
}

pub(super) fn decrypt(payload: &str, passphrase: &[u8]) -> Result<Vec<u8>, String> {
    // Import uses the password chosen by the exporting client. Legacy versions
    // counted bytes, so e.g. two emoji were valid eight-byte passwords.
    if passphrase.is_empty() || passphrase.len() > 1024 {
        return Err("请输入有效的备份密码".into());
    }
    if payload.len() > CONFIG_BACKUP_MAX_BYTES * 2 {
        return Err("配置备份内容过大".into());
    }
    let (salt, iv, ciphertext, iterations, compact) =
        if let Some(encoded) = payload.strip_prefix(PREFIX) {
            let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(encoded)
                .map_err(|_| "备份格式无效")?;
            if bytes.len() < 44 {
                return Err("备份内容不完整".into());
            }
            (
                bytes[..16].to_vec(),
                bytes[16..28].to_vec(),
                bytes[28..].to_vec(),
                CONFIG_BACKUP_ITERATIONS,
                true,
            )
        } else {
            let envelope: EncryptedConfigBackup =
                serde_json::from_str(payload).map_err(|_| "备份格式无效")?;
            if envelope.version != 3
                || envelope.kdf != "PBKDF2-HMAC-SHA256"
                || !(100_000..=1_000_000).contains(&envelope.iterations)
            {
                return Err("备份格式不受支持".into());
            }
            let decode = |text: &str| {
                base64::engine::general_purpose::STANDARD
                    .decode(text)
                    .map_err(|_| "备份参数无效".to_string())
            };
            (
                decode(&envelope.salt)?,
                decode(&envelope.iv)?,
                decode(&envelope.ciphertext)?,
                envelope.iterations,
                false,
            )
        };
    if salt.len() != 16 || iv.len() != 12 || ciphertext.len() > CONFIG_BACKUP_MAX_BYTES + 1024 {
        return Err("备份参数长度无效".into());
    }
    let mut key = config_backup_key(passphrase, &salt, iterations);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| "备份密钥无效")?;
    let result = cipher.decrypt(
        Nonce::from_slice(&iv),
        Payload {
            msg: &ciphertext,
            aad: if compact { AAD } else { &[] },
        },
    );
    key.fill(0);
    let mut decrypted = result.map_err(|_| "配置备份密码错误或内容已损坏")?;
    if !compact {
        if decrypted.len() > CONFIG_BACKUP_MAX_BYTES {
            decrypted.fill(0);
            return Err("备份内容过大".into());
        }
        return Ok(decrypted);
    }
    let decoded = decompress(&decrypted);
    decrypted.fill(0);
    decoded
}

fn decompress(compressed: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoded = Vec::new();
    let read = flate2::read::DeflateDecoder::new(compressed)
        .take((CONFIG_BACKUP_MAX_BYTES + 1) as u64)
        .read_to_end(&mut decoded);
    if read.is_err() || decoded.len() > CONFIG_BACKUP_MAX_BYTES {
        decoded.fill(0);
        return Err("备份内容损坏或解压后过大".into());
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_backups_are_shorter_and_authenticated_with_three_character_passwords() {
        let text =
            br#"{"accounts":[{"user":"example","pass":"secret"}],"theme":"basic","enabled":true}"#
                .repeat(30);
        let first = encrypt(&text, b"abc").unwrap();
        assert!(first.len() < text.len() / 2);
        assert_eq!(decrypt(&first, b"abc").unwrap(), text);
        assert_ne!(first, encrypt(&text, b"abc").unwrap());
        assert!(decrypt(&first, b"bad").is_err());
        let mut damaged = first.into_bytes();
        let last = damaged.len() - 8;
        damaged[last] = if damaged[last] == b'A' { b'B' } else { b'A' };
        assert!(decrypt(std::str::from_utf8(&damaged).unwrap(), b"abc").is_err());
        assert!(encrypt(&text, b"ab").is_err());
        assert!(encrypt(&text, "密碼".as_bytes()).is_err());
        assert!(encrypt(&text, "备份码".as_bytes()).is_ok());
    }
    #[test]
    fn version_three_backups_remain_readable() {
        let salt = [1; 16];
        let iv = [2; 12];
        let password = "🔐🔑".as_bytes();
        let key = config_backup_key(password, &salt, CONFIG_BACKUP_ITERATIONS);
        let encrypted = Aes256Gcm::new_from_slice(&key)
            .unwrap()
            .encrypt(Nonce::from_slice(&iv), b"legacy config".as_slice())
            .unwrap();
        let base64 = base64::engine::general_purpose::STANDARD;
        let old = serde_json::json!({"version":3,"kdf":"PBKDF2-HMAC-SHA256","iterations":CONFIG_BACKUP_ITERATIONS,"salt":base64.encode(salt),"iv":base64.encode(iv),"ciphertext":base64.encode(encrypted)}).to_string();
        assert_eq!(decrypt(&old, password).unwrap(), b"legacy config");
    }
    #[test]
    fn decompression_has_a_hard_output_limit() {
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(&vec![0; CONFIG_BACKUP_MAX_BYTES + 1])
            .unwrap();
        assert!(decompress(&encoder.finish().unwrap()).is_err());
    }
}
