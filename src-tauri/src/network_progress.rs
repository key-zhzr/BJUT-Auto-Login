use crate::{AppState, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::Emitter;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Progress {
    id: u64,
    generation: u64,
    revision: u64,
    message: String,
    percent: u8,
    elapsed_ms: u64,
    complete: bool,
}

#[derive(Default)]
pub(crate) struct Control(Mutex<Option<Progress>>);

impl Control {
    pub(crate) fn snapshot(&self) -> Option<Progress> {
        self.0.lock().unwrap().clone()
    }

    fn begin(&self, generation: u64, message: &str) -> Progress {
        let mut state = self.0.lock().unwrap();
        if let Some(newer) = state.as_ref().filter(|value| value.generation > generation) {
            return newer.clone();
        }
        let id = state.as_ref().map_or(1, |value| value.id + 1);
        let progress = Progress {
            id,
            generation,
            revision: 0,
            message: message.into(),
            percent: 5,
            elapsed_ms: 0,
            complete: false,
        };
        *state = Some(progress.clone());
        progress
    }

    fn update(
        &self,
        id: u64,
        generation: u64,
        message: &str,
        percent: u8,
        elapsed_ms: u64,
        complete: bool,
    ) -> Option<Progress> {
        let mut state = self.0.lock().unwrap();
        let value = state
            .as_mut()
            .filter(|value| value.id == id && value.generation == generation && !value.complete)?;
        value.revision += 1;
        value.message = message.into();
        value.percent = value.percent.max(percent.min(100));
        value.elapsed_ms = elapsed_ms;
        value.complete = complete;
        Some(value.clone())
    }
}

pub(crate) struct Run {
    app: tauri::AppHandle,
    state: Arc<AppState>,
    id: u64,
    generation: u64,
    started: Instant,
    finish_on_drop: bool,
}

impl Run {
    pub(crate) fn begin(
        app: &tauri::AppHandle,
        state: &Arc<AppState>,
        generation: u64,
        message: &str,
    ) -> Self {
        let progress = state.network_progress.begin(generation, message);
        let _ = app.emit("network-check-progress", &progress);
        Self {
            app: app.clone(),
            state: state.clone(),
            id: progress.id,
            generation,
            started: Instant::now(),
            finish_on_drop: true,
        }
    }

    pub(crate) fn phase(&self, message: &str, percent: u8) {
        self.publish(message, percent, false);
    }

    pub(crate) fn handoff(mut self) {
        self.finish_on_drop = false;
    }

    fn publish(&self, message: &str, percent: u8, complete: bool) {
        if self.generation != self.state.network_change_generation.load(Ordering::SeqCst) {
            return;
        }
        if let Some(progress) = self.state.network_progress.update(
            self.id,
            self.generation,
            message,
            percent,
            self.started.elapsed().as_millis() as u64,
            complete,
        ) {
            let _ = self.app.emit("network-check-progress", &progress);
        }
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        if !self.finish_on_drop {
            return;
        }
        let last = self.state.last_network_state.lock().unwrap().clone();
        let message = match last["state"].as_str() {
            Some("Online") => "检测完成，互联网已连通",
            Some("BjutCampus") => "检测完成，校园网需要登录认证",
            Some("Offline") => "检测完成，网络暂不可用",
            _ if self.state.manual_login_in_progress.load(Ordering::SeqCst) => {
                "检测已交给登录流程继续处理"
            }
            _ => "本轮检测已结束",
        };
        self.publish(message, 100, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_runs_and_completed_runs_cannot_overwrite_new_progress() {
        let control = Control::default();
        let first = control.begin(1, "first");
        let second = control.begin(2, "waiting for DHCP");
        assert!(control
            .update(first.id, 1, "old completed", 100, 5, true)
            .is_none());
        assert!(control
            .update(second.id, 1, "wrong network", 100, 5, true)
            .is_none());
        assert_eq!(
            control
                .update(second.id, 2, "probing", 60, 10, false)
                .unwrap()
                .percent,
            60
        );
        assert_eq!(
            control
                .update(second.id, 2, "next", 40, 20, false)
                .unwrap()
                .percent,
            60
        );
        assert!(
            control
                .update(second.id, 2, "done", 100, 30, true)
                .unwrap()
                .complete
        );
        assert!(control
            .update(second.id, 2, "late", 70, 40, false)
            .is_none());
    }
}
