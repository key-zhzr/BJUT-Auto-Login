//! DPAPI-protected configuration. Credential Manager's single-entry size limit
//! must not become an account/password limit for the whole application.
use std::{
    io::{Read, Write},
    path::Path,
};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    },
};

const MAGIC: &[u8] = b"BJUT-WIN-CONFIG\x01";
const MAX_BYTES: usize = 16 * 1024 * 1024;
const ENTROPY: &[u8] = b"cn.edu.bjut.al/config/dpapi/v1";

fn transform(bytes: &[u8], protect: bool) -> Result<Vec<u8>, String> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES + 65536 {
        return Err("Windows 配置文件大小无效".into());
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: ENTROPY.len() as u32,
        pbData: ENTROPY.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: input/entropy remain live; DPAPI does not modify the input bytes.
    // Use current-user protection, never CRYPTPROTECT_LOCAL_MACHINE.
    let result = unsafe {
        if protect {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                None,
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if result.is_err() {
        return Err(if protect {
            "Windows 无法加密配置"
        } else {
            "Windows 无法解密配置，请使用原 Windows 用户或导入备份"
        }
        .into());
    }
    // SAFETY: DPAPI allocated output.cbData bytes with LocalAlloc. Copy them,
    // then wipe the native buffer (which may contain plaintext) before freeing.
    if output.pbData.is_null() || output.cbData == 0 {
        return Err("Windows 加密服务返回了空内容".into());
    }
    let mut copied = unsafe {
        let data = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
        let copied = data.to_vec();
        data.fill(0);
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        copied
    };
    if !protect && copied.len() > MAX_BYTES {
        copied.fill(0);
        return Err("Windows 配置内容过大".into());
    }
    Ok(copied)
}

pub(crate) fn read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("无法读取 Windows 加密配置".into()),
    };
    let mut payload = Vec::new();
    file.take((MAX_BYTES + 65537) as u64)
        .read_to_end(&mut payload)
        .map_err(|_| "无法读取 Windows 加密配置")?;
    if payload.len() > MAX_BYTES + 65536 {
        return Err("Windows 配置文件过大".into());
    }
    let encrypted = payload
        .strip_prefix(MAGIC)
        .ok_or("Windows 配置文件格式无效")?;
    transform(encrypted, false).map(Some)
}

pub(crate) fn write(path: &Path, plaintext: &[u8]) -> Result<(), String> {
    if plaintext.len() > MAX_BYTES {
        return Err("Windows 配置内容过大".into());
    }
    let encrypted = transform(plaintext, true)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| "无法创建 Windows 凭据目录")?;
    }
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = path.with_extension(format!("{}.{}.tmp", std::process::id(), unique));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| "无法写入 Windows 加密配置")?;
    let result = (|| {
        file.write_all(MAGIC)
            .and_then(|_| file.write_all(&encrypted))
            .and_then(|_| file.sync_all())
            .map_err(|_| "Windows 配置写入失败")?;
        drop(file);
        let mut restored = read(&temporary)?.ok_or("Windows 配置校验失败")?;
        let matches = restored == plaintext;
        restored.fill(0);
        if !matches {
            return Err("Windows 配置校验失败，原配置已保留".into());
        }
        std::fs::rename(&temporary, path)
            .map_err(|_| "无法替换 Windows 加密配置，原配置已保留".into())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn large_unicode_config_and_fifteen_character_password_survive_updates() {
        let directory =
            std::env::temp_dir().join(format!("bjut-dpapi-test-{}", std::process::id()));
        let path = directory.join("credentials.dpapi");
        let mut config = serde_json::json!({"accounts":[{"user":"fixture","pass":"123456789012345"}],"notes":"中文配置".repeat(4000)}).to_string();
        assert!(config.encode_utf16().count() > 2560);
        write(&path, config.as_bytes()).unwrap();
        assert_eq!(read(&path).unwrap().unwrap(), config.as_bytes());
        let encrypted = std::fs::read(&path).unwrap();
        assert!(!encrypted.windows(15).any(|part| part == b"123456789012345"));
        config.push(' ');
        write(&path, config.as_bytes()).unwrap();
        assert_eq!(read(&path).unwrap().unwrap(), config.as_bytes());
        let mut damaged = encrypted;
        let last = damaged.len() - 1;
        damaged[last] ^= 1;
        std::fs::write(&path, damaged).unwrap();
        assert!(read(&path).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
