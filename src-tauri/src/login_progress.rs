use std::{sync::Mutex, time::Instant};
use tauri::Emitter;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PhaseTiming {
    phase: String,
    duration_ms: u128,
}

struct Operation {
    id: String,
    phase: String,
    started: Instant,
    phase_started: Instant,
    submitted: bool,
    cancelled: bool,
    active: bool,
    timings: Vec<PhaseTiming>,
}

#[derive(Default)]
pub(crate) struct LoginProgressControl(Mutex<Option<Operation>>);

impl LoginProgressControl {
    pub(crate) fn submitted(&self, id: &str) -> bool {
        self.0
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|op| op.id == id && op.submitted)
    }
    pub(crate) fn start(&self, id: &str) -> Result<(), String> {
        if id.is_empty()
            || id.len() > 80
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("登录操作标识无效".to_string());
        }
        let mut current = self.0.lock().unwrap();
        if current
            .as_ref()
            .is_some_and(|op| op.id == id && op.cancelled)
        {
            return Err("已取消登录，未发送认证".to_string());
        }
        if current.as_ref().is_some_and(|op| op.active) {
            return Err("已有登录操作正在进行".to_string());
        }
        *current = Some(Operation {
            id: id.to_string(),
            phase: "checking".to_string(),
            started: Instant::now(),
            phase_started: Instant::now(),
            submitted: false,
            cancelled: false,
            active: true,
            timings: Vec::new(),
        });
        Ok(())
    }

    pub(crate) fn cancel(&self, id: &str) -> Result<(), String> {
        let mut current = self.0.lock().unwrap();
        if let Some(op) = current.as_mut().filter(|op| op.id == id) {
            if op.submitted {
                return Err("认证或注销请求已开始提交，将先核对结果".to_string());
            }
            op.cancelled = true;
            return Ok(());
        }
        if current.as_ref().is_some_and(|op| op.active) {
            return Err("登录操作已变化".to_string());
        }
        // A cancellation may overtake its IPC login call. Remember the token
        // so the later start cannot send credentials after the UI cancelled.
        if id.len() > 80 {
            return Err("登录操作标识无效".to_string());
        }
        *current = Some(Operation {
            id: id.to_string(),
            phase: "cancelled".to_string(),
            started: Instant::now(),
            phase_started: Instant::now(),
            submitted: false,
            cancelled: true,
            active: false,
            timings: Vec::new(),
        });
        Ok(())
    }

    pub(crate) fn check(&self, id: &str) -> Result<(), String> {
        if self
            .0
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|op| op.id == id && op.active && !op.cancelled)
        {
            Ok(())
        } else {
            Err("已取消登录，未继续发送认证".to_string())
        }
    }

    fn update(
        &self,
        id: &str,
        phase: &str,
        message: &str,
        submitting: bool,
        terminal: bool,
    ) -> Result<serde_json::Value, String> {
        let mut current = self.0.lock().unwrap();
        let op = current
            .as_mut()
            .filter(|op| op.id == id)
            .ok_or("登录操作已变化")?;
        if op.cancelled && !terminal {
            return Err("已取消登录，未发送认证".to_string());
        }
        // Cancellation and the first request submission share this mutex.
        op.submitted |= submitting;
        if op.phase != phase {
            op.timings.push(PhaseTiming {
                phase: op.phase.clone(),
                duration_ms: op.phase_started.elapsed().as_millis(),
            });
            op.phase = phase.to_string();
            op.phase_started = Instant::now();
        }
        if terminal {
            op.active = false;
        }
        Ok(
            serde_json::json!({"operationId": id, "phase": phase, "message": message, "elapsedMs": op.started.elapsed().as_millis(), "canCancel": !op.submitted && !op.cancelled && op.active, "timings": op.timings}),
        )
    }
}

pub(crate) struct Session<'a> {
    pub(crate) id: &'a str,
    pub(crate) control: &'a LoginProgressControl,
    pub(crate) app: &'a tauri::AppHandle,
}

impl Session<'_> {
    pub(crate) fn phase(&self, phase: &str, message: &str) -> Result<(), String> {
        let payload = self.control.update(self.id, phase, message, false, false)?;
        let _ = self.app.emit("login-progress", payload);
        Ok(())
    }
    pub(crate) fn submitting(&self, phase: &str, message: &str) -> Result<(), String> {
        let payload = self.control.update(self.id, phase, message, true, false)?;
        let _ = self.app.emit("login-progress", payload);
        Ok(())
    }
    pub(crate) fn finish(&self, success: bool, message: &str) {
        if let Ok(payload) = self.control.update(
            self.id,
            if success { "complete" } else { "stopped" },
            message,
            false,
            true,
        ) {
            let _ = self.app.emit("login-progress", payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_cannot_race_into_a_credential_submission() {
        let control = LoginProgressControl::default();
        control.start("a").unwrap();
        control.cancel("a").unwrap();
        assert!(control
            .update("a", "authenticating", "", true, false)
            .is_err());
        control.update("a", "stopped", "", false, true).unwrap();
        control.start("b").unwrap();
        control
            .update("b", "authenticating", "", true, false)
            .unwrap();
        assert!(control.cancel("b").is_err());
        assert!(control.check("b").is_ok());
        assert!(control.cancel("old").is_err());
    }
    #[test]
    fn cancellation_before_ipc_start_is_remembered() {
        let control = LoginProgressControl::default();
        control.cancel("late").unwrap();
        assert!(control.start("late").is_err());
        assert!(control.start("new").is_ok());
    }
}
