#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SchedulePlan {
    pub(crate) interval_seconds: i32,
    pub(crate) interface_poll_seconds: i32,
    pub(crate) reason: String,
}

impl Default for SchedulePlan {
    fn default() -> Self {
        Self {
            interval_seconds: 15,
            interface_poll_seconds: 3,
            reason: "等待首次检测".to_string(),
        }
    }
}

#[derive(Default)]
pub(crate) struct AdaptiveSchedule {
    identity: String,
    online_count: u32,
    offline_count: u32,
    pub(crate) plan: SchedulePlan,
}

pub(crate) struct ScheduleInput<'a> {
    pub(crate) identity: &'a str,
    pub(crate) online: bool,
    pub(crate) has_link: bool,
    pub(crate) background: bool,
    pub(crate) mobile_data: bool,
    pub(crate) power_saving: bool,
    pub(crate) enabled: bool,
    pub(crate) configured: i32,
}

impl AdaptiveSchedule {
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
    pub(crate) fn observe(&mut self, input: ScheduleInput<'_>) -> SchedulePlan {
        if self.identity != input.identity {
            self.reset();
            self.identity = input.identity.to_string();
        }
        if input.online {
            self.online_count = self.online_count.saturating_add(1);
            self.offline_count = 0;
        } else {
            self.offline_count = self.offline_count.saturating_add(1);
            self.online_count = 0;
        }
        let base = input.configured.max(5);
        let (mut seconds, mut reason) = (base, "使用设定的检测间隔");
        if input.enabled {
            if input.online && input.background && self.online_count >= 3 {
                let multiplier = if self.online_count >= 6 { 5 } else { 2 };
                seconds = base.saturating_mul(multiplier).min(base.max(300));
                reason = "连接持续稳定，已降低后台检测频率";
            } else if !input.online && input.has_link && self.offline_count <= 3 {
                seconds = base.min(5);
                reason = "连接尚未恢复，临时加快前 3 次检测";
            } else if !input.online && input.background {
                seconds = base.saturating_mul(2).min(base.max(300));
                reason = "连续未恢复，已放缓重试；网络变化会立即唤醒";
            }
            if input.power_saving {
                seconds = seconds.max(if input.background { 120 } else { 60 });
                reason = "系统处于省电或息屏状态，已减少主动探测";
            }
        }
        if input.mobile_data {
            seconds = seconds.max(if input.background { 300 } else { 120 });
            reason = "使用移动数据，低频检查并暂停校园认证";
        }
        let poll = if input.mobile_data || (input.enabled && input.power_saving) {
            30
        } else if input.enabled
            && input.background
            && (self.online_count >= 3 || self.offline_count > 3)
        {
            15
        } else {
            3
        };
        self.plan = SchedulePlan {
            interval_seconds: seconds,
            interface_poll_seconds: poll,
            reason: reason.to_string(),
        };
        self.plan.clone()
    }
}

pub(crate) fn power_saving() -> bool {
    use std::{
        sync::{Mutex, OnceLock},
        time::{Duration, Instant},
    };
    static CACHE: OnceLock<Mutex<Option<(Instant, bool)>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(|| Mutex::new(None)).lock().unwrap();
    if let Some((checked, value)) = *cache {
        if checked.elapsed() < Duration::from_secs(30) {
            return value;
        }
    }
    let value = read_power_saving();
    *cache = Some((Instant::now(), value));
    value
}

fn read_power_saving() -> bool {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/bin/pmset")
            .arg("-g")
            .output()
            .ok()
            .is_some_and(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                        let mut fields = line.split_whitespace();
                        fields.next() == Some("lowpowermode") && fields.next() == Some("1")
                    })
            })
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        let mut status = SYSTEM_POWER_STATUS::default();
        // SAFETY: the API writes one initialized stack structure.
        unsafe { GetSystemPowerStatus(&mut status).is_ok() && status.SystemStatusFlag == 1 }
    }
    #[cfg(target_os = "android")]
    {
        crate::call_android_network_helper_bool("isNetworkPowerSaving")
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_dir("/sys/class/power_supply")
            .ok()
            .is_some_and(|items| {
                items.flatten().any(|item| {
                    std::fs::read_to_string(item.path().join("status"))
                        .is_ok_and(|value| value.trim() == "Discharging")
                })
            })
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux",
        target_os = "android"
    )))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_configurations_enable_adaptive_checks_without_pinning_an_adapter() {
        let config: crate::config_model::AppConfig = serde_json::from_str("{}").unwrap();
        assert!(config.adaptive_network_checks);
        assert!(config.preferred_interface.is_empty());
    }
    fn sample<'a>(identity: &'a str, online: bool) -> ScheduleInput<'a> {
        ScheduleInput {
            identity,
            online,
            has_link: true,
            background: true,
            mobile_data: false,
            power_saving: false,
            enabled: true,
            configured: 60,
        }
    }
    #[test]
    fn stable_link_slows_down_but_loss_and_interface_changes_reset_it() {
        let mut schedule = AdaptiveSchedule::default();
        for _ in 0..6 {
            schedule.observe(sample("en7/172.26.99.10", true));
        }
        assert_eq!(schedule.plan.interval_seconds, 300);
        assert_eq!(schedule.plan.interface_poll_seconds, 15);
        assert_eq!(
            schedule
                .observe(sample("en7/172.26.99.10", false))
                .interval_seconds,
            5
        );
        for _ in 0..3 {
            schedule.observe(sample("en7/172.26.99.10", false));
        }
        assert_eq!(schedule.plan.interval_seconds, 120);
        assert_eq!(
            schedule
                .observe(sample("en0/192.168.1.10", true))
                .interval_seconds,
            60
        );
    }
    #[test]
    fn power_saving_mobile_data_and_user_disabled_mode_take_precedence() {
        let mut schedule = AdaptiveSchedule::default();
        let mut input = sample("wifi", false);
        input.power_saving = true;
        assert_eq!(schedule.observe(input).interval_seconds, 120);
        let mut input = sample("wifi", false);
        input.enabled = false;
        assert_eq!(schedule.observe(input).interval_seconds, 60);
        let mut input = sample("cellular", false);
        input.mobile_data = true;
        assert_eq!(schedule.observe(input).interval_seconds, 300);
    }
}
