//! Small, credential-free history of decisions, separate from verbose logs.
use std::collections::VecDeque;
use tauri::{Emitter, Manager};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NetworkEvent {
    time: String,
    kind: String,
    message: String,
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct Timeline {
    events: VecDeque<NetworkEvent>,
}
impl Timeline {
    pub(crate) fn entries(&self) -> Vec<NetworkEvent> {
        self.events.iter().cloned().collect()
    }
    fn add(&mut self, kind: &str, message: &str) -> bool {
        if kind != "login"
            && kind != "change"
            && self
                .events
                .iter()
                .rev()
                .find(|event| event.kind == kind)
                .is_some_and(|event| event.message == message)
        {
            return false;
        }
        self.events.push_back(NetworkEvent {
            time: chrono::Local::now().to_rfc3339(),
            kind: kind.into(),
            message: message.into(),
        });
        while self.events.len() > 80 {
            self.events.pop_front();
        }
        true
    }
}
fn path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("network-events.json"))
}
pub(crate) fn load(app: &tauri::AppHandle) -> Timeline {
    path(app)
        .and_then(|path| std::fs::read(path).ok())
        .filter(|bytes| bytes.len() < 128 * 1024)
        .and_then(|bytes| serde_json::from_slice::<Timeline>(&bytes).ok())
        .map(|mut timeline| {
            while timeline.events.len() > 80 {
                timeline.events.pop_front();
            }
            timeline
        })
        .unwrap_or_default()
}
pub(crate) fn record(app: &tauri::AppHandle, state: &crate::AppState, kind: &str, message: &str) {
    let mut timeline = state.network_events.lock().unwrap();
    if !timeline.add(kind, message) {
        return;
    }
    if let Some(path) = path(app) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(bytes) = serde_json::to_vec(&*timeline) {
            let _ = std::fs::write(path, bytes);
        }
    }
    let _ = app.emit("network-events", timeline.entries());
}
pub(crate) fn from_log(
    app: &tauri::AppHandle,
    state: &crate::AppState,
    module: &str,
    message: &str,
    log_type: &str,
) {
    // Store a vocabulary of decisions, never interpolate accounts, IPs or raw errors.
    let event = if module == "登录反馈" {
        Some((
            "login",
            if log_type == "success" {
                "登录完成，详细结果与阶段耗时已记入日志"
            } else if message.contains("取消") {
                "登录已取消；详细原因已记入日志"
            } else {
                "登录未确认成功；请查看日志中的网关结果与阶段耗时"
            },
        ))
    } else if module == "网络" && message.contains("网络检测完毕: 互联网已连通") {
        Some(("state", "已确认联网，保持后台检测"))
    } else if module == "网络" && message.contains("检测到校园网登录页面") {
        Some(("state", "发现校园认证页面，当前需要核对认证"))
    } else if message.contains("自动登录未开启，忽略重连") {
        Some(("decision", "自动登录已关闭，因此未提交认证"))
    } else if module == "网络" && message.starts_with("登录成功:") {
        Some(("state", "认证网关已接受自动登录，正在核对联网状态"))
    } else if message.contains("尝试使用账号") {
        Some(("login", "开始认证，正在使用已选择的校园网接口"))
    } else if message.contains("认证响应未确认")
        || message.contains("登录结果无法确认")
        || message.contains("登录请求已发送，但结果无法确认")
    {
        Some((
            "decision",
            "认证响应未确认，为避免重复提交已停止继续尝试账号",
        ))
    } else {
        None
    };
    if let Some((kind, detail)) = event {
        record(app, state, kind, detail);
    }
}
#[tauri::command]
pub(crate) fn get_network_events(
    state: tauri::State<std::sync::Arc<crate::AppState>>,
) -> Vec<NetworkEvent> {
    state.network_events.lock().unwrap().entries()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_polling_is_coalesced_but_each_login_and_state_change_is_retained() {
        let mut timeline = Timeline::default();
        assert!(timeline.add("state", "online"));
        assert!(timeline.add("decision", "auto off"));
        assert!(!timeline.add("state", "online"));
        assert!(timeline.add("state", "offline"));
        assert!(timeline.add("state", "online"));
        for _ in 0..90 {
            assert!(timeline.add("login", "completed"));
        }
        assert_eq!(timeline.entries().len(), 80);
        let decoded: Timeline =
            serde_json::from_slice(&serde_json::to_vec(&timeline).unwrap()).unwrap();
        assert_eq!(decoded.entries().len(), 80);
    }
}
