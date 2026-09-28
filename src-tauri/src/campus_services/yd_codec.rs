//! ydapp's datajson envelope, verified against index.621bca5e.js and the
//! supplied 2026-09-28 HAR. This is wire compatibility, not secret storage.
//! The key is carried in the envelope; authenticated HTTPS remains required.
use super::{CampusServiceError, Value};
use aes::{
    cipher::{BlockDecrypt, BlockEncrypt, KeyInit},
    Aes128,
};
use base64::{engine::general_purpose::STANDARD, Engine};

const MAX_BYTES: usize = 2 * 1024 * 1024;

pub(super) fn encode(value: &Value) -> Result<Value, CampusServiceError> {
    use aes_gcm::aead::{rand_core::RngCore, OsRng};
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut key = [0; 16];
    for byte in &mut key {
        // Rejection sampling keeps the 62-character alphabet unbiased.
        loop {
            let candidate = (OsRng.next_u32() & 255) as u8;
            if candidate < 248 {
                *byte = ALPHABET[(candidate % 62) as usize];
                break;
            }
        }
    }
    let bytes = serde_json::to_vec(value).map_err(|_| error("请求内容无法编码"))?;
    Ok(serde_json::json!({"datajson":encrypt(&bytes, &key)?}))
}

fn encrypt(input: &[u8], key: &[u8; 16]) -> Result<String, CampusServiceError> {
    if input.len() > MAX_BYTES {
        return Err(error("请求过大"));
    }
    let cipher = Aes128::new(key.into());
    let padding = 16 - input.len() % 16;
    let mut bytes = input.to_vec();
    bytes.resize(input.len() + padding, padding as u8);
    for block in bytes.as_chunks_mut::<16>().0 {
        cipher.encrypt_block(block.into());
    }
    let mut prefix = *key;
    prefix.rotate_left(10);
    prefix.reverse();
    Ok(format!(
        "{}{}",
        std::str::from_utf8(&prefix).map_err(|_| error("请求密钥无效"))?,
        STANDARD.encode(bytes)
    ))
}

pub(super) fn decode(value: Value) -> Result<Value, CampusServiceError> {
    let Some(encoded) = value.get("datajson") else {
        return Ok(value);
    };
    let encoded = encoded
        .as_str()
        .ok_or_else(|| error("响应封装格式不正确"))?;
    if encoded.len() <= 16
        || encoded.len() > MAX_BYTES
        || !encoded.as_bytes()[..16]
            .iter()
            .all(u8::is_ascii_alphanumeric)
    {
        return Err(error("响应封装不完整"));
    }
    let mut key: [u8; 16] = encoded.as_bytes()[..16].try_into().unwrap();
    key.reverse();
    key.rotate_left(6);
    let mut bytes = STANDARD
        .decode(&encoded[16..])
        .map_err(|_| error("响应编码无效"))?;
    if bytes.is_empty() || bytes.len() % 16 != 0 {
        return Err(error("响应数据不完整"));
    }
    let cipher = Aes128::new((&key).into());
    for block in bytes.as_chunks_mut::<16>().0 {
        cipher.decrypt_block(block.into());
    }
    let padding = *bytes.last().unwrap() as usize;
    if !(1..=16).contains(&padding)
        || !bytes[bytes.len() - padding..]
            .iter()
            .all(|byte| *byte as usize == padding)
    {
        return Err(error("响应校验失败"));
    }
    bytes.truncate(bytes.len() - padding);
    let text = String::from_utf8(bytes).map_err(|_| error("响应文本无效"))?;
    if matches!(text.trim_start().as_bytes().first(), Some(b'{' | b'[')) {
        serde_json::from_str(&text).map_err(|_| error("响应内容无法解析"))
    } else {
        Ok(Value::String(text))
    }
}

fn error(reason: &str) -> CampusServiceError {
    CampusServiceError::protocol(format!(
        "一卡通{reason}，请重新核对；若已提交付款，请先查询到账记录"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a user-supplied private HAR; never commit capture data"]
    fn validates_local_private_har_without_printing_payloads() {
        let path = std::env::var("BJUT_YDAPP_TEST_HAR").expect("provide a local HAR path");
        let har: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let mut requests = 0;
        let mut responses = 0;
        for entry in har["log"]["entries"].as_array().unwrap() {
            if reqwest::Url::parse(entry["request"]["url"].as_str().unwrap())
                .unwrap()
                .host_str()
                != Some(super::super::YD_HOST)
            {
                continue;
            }
            if let Some(text) = entry
                .pointer("/request/postData/text")
                .and_then(Value::as_str)
            {
                if let Ok(wrapped) = serde_json::from_str::<Value>(text) {
                    if wrapped.get("datajson").is_some() {
                        let data = decode(wrapped).expect("captured request decoding failed");
                        assert!(data.is_object());
                        assert!(decode(encode(&data).unwrap()).unwrap() == data);
                        requests += 1;
                    }
                }
            }
            if let Some(text) = entry
                .pointer("/response/content/text")
                .and_then(Value::as_str)
            {
                let bytes = if entry
                    .pointer("/response/content/encoding")
                    .and_then(Value::as_str)
                    == Some("base64")
                {
                    STANDARD.decode(text).unwrap()
                } else {
                    text.as_bytes().to_vec()
                };
                if let Ok(wrapped) = serde_json::from_slice::<Value>(&bytes) {
                    if wrapped.get("datajson").is_some() {
                        let data = decode(wrapped).expect("captured response decoding failed");
                        assert!(data.get("success").and_then(Value::as_bool) == Some(true));
                        responses += 1;
                    }
                }
            }
        }
        assert!(
            requests > 0 && responses > 0,
            "capture must contain encrypted request and response"
        );
    }
    #[test]
    fn independent_openssl_vector_and_unicode_roundtrip() {
        // Fixture generated independently with openssl aes-128-ecb / PKCS#7.
        let value = serde_json::json!({"success":true,"message":"核对成功"});
        let encrypted =
            "9876543210FEDCBA1hNo0rW19l4y9EX46A2MsDj9P/2p2DxwMWztmd5J8eeODy/L1YUP0mRoPjDMAo/J";
        assert_eq!(
            decode(serde_json::json!({"datajson":encrypted})).unwrap(),
            value
        );
        assert_eq!(decode(encode(&value).unwrap()).unwrap(), value);
        assert_ne!(encode(&value).unwrap(), encode(&value).unwrap());
    }
    #[test]
    fn legacy_json_html_and_malformed_envelopes() {
        let value = serde_json::json!({"success":false,"message":"余额不足"});
        assert_eq!(decode(value.clone()).unwrap(), value);
        let html = "<form action='https://example.test/'></form>";
        let encoded = encrypt(html.as_bytes(), b"0123456789ABCDEF").unwrap();
        assert_eq!(
            decode(serde_json::json!({"datajson":encoded})).unwrap(),
            html
        );
        for bad in [
            Value::Null,
            Value::Bool(false),
            Value::String("short".into()),
            Value::String("1234567890ABCDEFAAAA".into()),
            Value::String("资源受限资源受限资源受限".into()),
        ] {
            assert!(decode(serde_json::json!({"datajson":bad})).is_err());
        }
        let mut encoded = encrypt(b"padding check", b"0123456789ABCDEF").unwrap();
        encoded.pop();
        assert!(decode(serde_json::json!({"datajson":encoded})).is_err());
        let mut invalid_padding = [0u8; 16];
        Aes128::new(b"0123456789ABCDEF".into()).encrypt_block((&mut invalid_padding).into());
        let encoded = format!("9876543210FEDCBA{}", STANDARD.encode(invalid_padding));
        assert!(decode(serde_json::json!({"datajson":encoded})).is_err());
    }
}
