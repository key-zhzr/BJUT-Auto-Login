//! Human-entered image verification. Cookies and credentials stay with the
//! suspended login request; the WebView receives only an image and an opaque id.
use base64::Engine;
use futures_util::StreamExt;
use serde::Serialize;
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

#[derive(Debug, PartialEq)]
pub(crate) enum Answer {
    Submit(String),
    Refresh,
    Cancel,
}

#[derive(Default)]
pub(crate) struct Control {
    pending: Mutex<HashMap<String, oneshot::Sender<Answer>>>,
}

impl Control {
    pub(crate) fn answer(
        &self,
        id: &str,
        text: Option<String>,
        refresh: bool,
    ) -> Result<(), String> {
        let answer = match (text, refresh) {
            (_, true) => Answer::Refresh,
            (Some(text), false)
                if !text.trim().is_empty()
                    && text.len() <= 64
                    && !text.chars().any(char::is_control) =>
            {
                Answer::Submit(text.trim().into())
            }
            (Some(_), false) => return Err("请填写图片中的验证码".into()),
            (None, false) => Answer::Cancel,
        };
        self.pending
            .lock()
            .unwrap()
            .remove(id)
            .ok_or("验证码已过期，请重新操作")?
            .send(answer)
            .map_err(|_| "验证请求已结束".into())
    }
}

#[derive(Default)]
struct WaitClock {
    elapsed: Duration,
    started: Option<Instant>,
}
type Emit = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

#[derive(Clone)]
pub(crate) struct Handler {
    control: Arc<Control>,
    emit: Emit,
    clock: Arc<Mutex<WaitClock>>,
}

impl Handler {
    pub(crate) fn new(
        control: Arc<Control>,
        emit: impl Fn(&str, serde_json::Value) + Send + Sync + 'static,
    ) -> Self {
        Self {
            control,
            emit: Arc::new(emit),
            clock: Arc::new(Mutex::new(WaitClock::default())),
        }
    }
    fn waited(&self) -> Duration {
        let clock = self.clock.lock().unwrap();
        clock.elapsed
            + clock
                .started
                .map(|start| start.elapsed())
                .unwrap_or_default()
    }
    pub(crate) async fn with_timeout<F: Future>(
        &self,
        budget: Duration,
        future: F,
    ) -> Result<F::Output, ()> {
        let started = Instant::now();
        let previous_wait = self.waited();
        let timer = async {
            loop {
                let network_time = started
                    .elapsed()
                    .saturating_sub(self.waited().saturating_sub(previous_wait));
                if network_time >= budget {
                    break;
                }
                tokio::time::sleep((budget - network_time).min(Duration::from_millis(200))).await;
            }
        };
        futures_util::pin_mut!(future, timer);
        match futures_util::future::select(future, timer).await {
            futures_util::future::Either::Left((result, _)) => Ok(result),
            futures_util::future::Either::Right(_) => Err(()),
        }
    }
    pub(crate) async fn prompt(&self, title: &str, image: String) -> Result<Answer, String> {
        use aes_gcm::aead::{rand_core::RngCore, OsRng};
        let mut bytes = [0u8; 16];
        OsRng.fill_bytes(&mut bytes);
        let id = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let (sender, receiver) = oneshot::channel();
        self.control
            .pending
            .lock()
            .unwrap()
            .insert(id.clone(), sender);
        self.clock.lock().unwrap().started = Some(Instant::now());
        let _guard = PromptGuard {
            handler: self.clone(),
            id: id.clone(),
        };
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Prompt<'a> {
            id: &'a str,
            title: &'a str,
            image: String,
            expires_in_seconds: u16,
        }
        (self.emit)(
            "image-captcha",
            serde_json::to_value(Prompt {
                id: &id,
                title,
                image,
                expires_in_seconds: 120,
            })
            .unwrap(),
        );
        let answer = tokio::time::timeout(Duration::from_secs(120), receiver)
            .await
            .map_err(|_| "验证码已过期，请重试")?
            .map_err(|_| "验证请求已结束")?;
        if answer == Answer::Cancel {
            return Err("已取消验证码验证".into());
        }
        Ok(answer)
    }
}

struct PromptGuard {
    handler: Handler,
    id: String,
}
impl Drop for PromptGuard {
    fn drop(&mut self) {
        self.handler
            .control
            .pending
            .lock()
            .unwrap()
            .remove(&self.id);
        let mut clock = self.handler.clock.lock().unwrap();
        if let Some(start) = clock.started.take() {
            clock.elapsed += start.elapsed();
        }
        drop(clock);
        (self.handler.emit)("image-captcha-close", serde_json::json!({"id":self.id}));
    }
}

pub(crate) async fn image(response: reqwest::Response) -> Result<String, String> {
    if !response.status().is_success() {
        return Err("验证码图片加载失败".into());
    }
    let mut bytes = Vec::new();
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|_| "验证码图片读取失败")?;
        if bytes.len() + chunk.len() > 512 * 1024 {
            return Err("验证码图片过大".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    encode_image(&bytes)
}

fn encode_image(bytes: &[u8]) -> Result<String, String> {
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        return Err("服务器没有返回有效的验证码图片".into());
    };
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn answers_are_single_use_and_unknown_challenges_are_rejected() {
        let control = Control::default();
        let (tx, mut rx) = oneshot::channel();
        control.pending.lock().unwrap().insert("fixture".into(), tx);
        assert!(control.answer("other", Some("1234".into()), false).is_err());
        control
            .answer("fixture", Some(" 1234 ".into()), false)
            .unwrap();
        assert_eq!(rx.try_recv().unwrap(), Answer::Submit("1234".into()));
        assert!(control
            .answer("fixture", Some("1234".into()), false)
            .is_err());
        assert!(encode_image(b"<svg onload='bad()'>").is_err());
        assert!(encode_image(b"<html>login expired</html>").is_err());
    }
    #[test]
    fn dropping_a_prompt_removes_its_pending_answer() {
        let control = Arc::new(Control::default());
        let handler = Handler::new(control.clone(), |_, _| {});
        let (tx, _) = oneshot::channel();
        control.pending.lock().unwrap().insert("fixture".into(), tx);
        drop(PromptGuard {
            handler,
            id: "fixture".into(),
        });
        assert!(control.pending.lock().unwrap().is_empty());
    }

    #[test]
    fn human_input_does_not_consume_the_network_timeout() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let control = Arc::new(Control::default());
                let answer_control = control.clone();
                let handler = Handler::new(control, move |event, payload| {
                    if event != "image-captcha" {
                        return;
                    }
                    let id = payload["id"].as_str().unwrap().to_string();
                    let control = answer_control.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(80)).await;
                        control.answer(&id, Some("1234".into()), false).unwrap();
                    });
                });
                let result = handler
                    .with_timeout(
                        Duration::from_millis(25),
                        handler.prompt("fixture", "fixture".into()),
                    )
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(result, Answer::Submit("1234".into()));
                assert!(handler
                    .with_timeout(
                        Duration::from_millis(5),
                        tokio::time::sleep(Duration::from_millis(50))
                    )
                    .await
                    .is_err());
            });
    }
}
