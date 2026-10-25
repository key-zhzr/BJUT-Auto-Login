//! Persisted desktop presentation; no account credentials belong here.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct TraySettings {
    pub icon: String,
    pub custom_png: String,
    pub menu_order: Vec<String>,
    pub title: String,
    pub show_account: bool,
    pub show_dual_stack: bool,
}
impl Default for TraySettings {
    fn default() -> Self {
        Self {
            icon: "default".into(),
            custom_png: String::new(),
            menu_order: ["status", "check", "login", "pause", "accounts"]
                .map(String::from)
                .to_vec(),
            title: "none".into(),
            show_account: false,
            show_dual_stack: true,
        }
    }
}
impl TraySettings {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !["default", "signal", "dot", "custom"].contains(&self.icon.as_str())
            || !["none", "status", "account"].contains(&self.title.as_str())
            || self.menu_order.len() > 5
            || self.custom_png.len() > 180_000
        {
            return Err("托盘设置无效".into());
        }
        let mut seen = std::collections::HashSet::new();
        if self.menu_order.iter().any(|key| {
            !["status", "check", "login", "pause", "accounts"].contains(&key.as_str())
                || !seen.insert(key)
        }) {
            return Err("托盘菜单选项无效".into());
        }
        if !self.custom_png.is_empty() {
            use base64::Engine;
            let png = base64::engine::general_purpose::STANDARD
                .decode(&self.custom_png)
                .map_err(|_| "请选择 PNG 图标")?;
            if png.len() < 24 || &png[..8] != b"\x89PNG\r\n\x1a\n" || png.len() > 131_072 {
                return Err("图标需为不超过 128 KB 的 PNG".into());
            }
            let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
            let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
            if width == 0 || height == 0 || width > 256 || height > 256 {
                return Err("图标尺寸不能超过 256 × 256".into());
            }
        }
        Ok(())
    }
}

#[cfg(desktop)]
pub(crate) fn image(settings: &TraySettings) -> Option<tauri::image::Image<'static>> {
    if settings.validate().is_err() {
        return None;
    }
    if settings.icon == "custom" {
        use base64::Engine;
        let png = base64::engine::general_purpose::STANDARD
            .decode(&settings.custom_png)
            .ok()?;
        return tauri::image::Image::from_bytes(&png).ok();
    }
    if !["signal", "dot"].contains(&settings.icon.as_str()) {
        return None;
    }
    let mut rgba = vec![0u8; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let dx = x as f64 - 15.5;
            let dy = y as f64 - 23.;
            let radius = (dx * dx + dy * dy).sqrt();
            let ink = if settings.icon == "dot" {
                dx * dx + (y as f64 - 15.5).powi(2) < 75.
            } else {
                (dx * dx + dy * dy < 6.)
                    || (dy < -2.
                        && dy.abs() > dx.abs() * 0.65
                        && [10., 17., 24.].iter().any(|r| (radius - r).abs() < 1.2))
            };
            if ink {
                let i = (y * 32 + x) * 4;
                rgba[i..i + 4].copy_from_slice(&[64, 125, 240, 255]);
            }
        }
    }
    Some(tauri::image::Image::new_owned(rgba, 32, 32))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_menu_order_and_untrusted_icons() {
        assert!(TraySettings::default().validate().is_ok());
        let mut value = TraySettings {
            menu_order: vec!["login".into(), "login".into()],
            ..Default::default()
        };
        assert!(value.validate().is_err());
        value.menu_order.clear();
        value.custom_png = "not-png".into();
        assert!(value.validate().is_err());
    }
}
