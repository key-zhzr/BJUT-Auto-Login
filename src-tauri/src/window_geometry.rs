//! Persist normal window bounds; recover safely when displays or DPI change.
use std::sync::{Arc, Mutex};
use tauri::Manager;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Geometry {
    x: i32,
    y: i32,
    width: f64,
    height: f64,
    maximized: bool,
}
struct WorkArea {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale: f64,
}
fn fit(saved: &Geometry, area: &WorkArea) -> Geometry {
    let scale = area.scale.max(0.5);
    let width = saved
        .width
        .clamp(320.0, 4096.0)
        .min((area.width as f64 / scale - 16.0).max(1.0));
    let height = saved
        .height
        .clamp(400.0, 4096.0)
        .min((area.height as f64 / scale - 40.0).max(1.0));
    Geometry {
        x: saved.x.clamp(
            area.x,
            area.x
                .saturating_add((area.width as f64 - width * scale).max(0.0) as i32),
        ),
        y: saved.y.clamp(
            area.y,
            area.y
                .saturating_add((area.height as f64 - height * scale - 32.0).max(0.0) as i32),
        ),
        width,
        height,
        maximized: saved.maximized,
    }
}
fn read(window: &tauri::WebviewWindow) -> Option<Geometry> {
    if window.is_minimized().ok()? || window.is_fullscreen().ok()? || window.is_maximized().ok()? {
        return None;
    }
    let pos = window.outer_position().ok()?;
    let size = window
        .inner_size()
        .ok()?
        .to_logical::<f64>(window.scale_factor().ok()?);
    (size.width >= 100.0 && size.height >= 100.0).then_some(Geometry {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
        maximized: false,
    })
}
pub(crate) fn install(window: &tauri::WebviewWindow) {
    let Ok(dir) = window.app_handle().path().app_data_dir() else {
        return;
    };
    let path = dir.join("window-geometry.json");
    let saved = std::fs::read(&path)
        .ok()
        .filter(|bytes| bytes.len() < 4096)
        .and_then(|bytes| serde_json::from_slice::<Geometry>(&bytes).ok())
        .filter(|saved| {
            saved.width.is_finite()
                && saved.height.is_finite()
                && saved.width > 0.0
                && saved.height > 0.0
        });
    let mut restored = None;
    if let Some(saved) = saved {
        let monitors = window.available_monitors().unwrap_or_default();
        let monitor = monitors
            .iter()
            .find(|monitor| {
                let area = monitor.work_area();
                saved.x >= area.position.x
                    && saved.y >= area.position.y
                    && (saved.x as i64) < area.position.x as i64 + area.size.width as i64
                    && (saved.y as i64) < area.position.y as i64 + area.size.height as i64
            })
            .or_else(|| monitors.first());
        if let Some(monitor) = monitor {
            let area = monitor.work_area();
            let fitted = fit(
                &saved,
                &WorkArea {
                    x: area.position.x,
                    y: area.position.y,
                    width: area.size.width,
                    height: area.size.height,
                    scale: monitor.scale_factor(),
                },
            );
            let _ = window.set_size(tauri::LogicalSize::new(fitted.width, fitted.height));
            let _ = window.set_position(tauri::PhysicalPosition::new(fitted.x, fitted.y));
            if fitted.maximized {
                let _ = window.maximize();
            }
            restored = Some(fitted);
        }
    }
    let Some(initial) = restored.or_else(|| read(window)) else {
        return;
    };
    let data = Arc::new(Mutex::new((initial, 0u64)));
    let window_copy = window.clone();
    window.on_window_event(move |event| {
        if !matches!(
            event,
            tauri::WindowEvent::Resized(_)
                | tauri::WindowEvent::Moved(_)
                | tauri::WindowEvent::CloseRequested { .. }
        ) {
            return;
        }
        if window_copy.is_minimized().unwrap_or(true)
            || window_copy.is_fullscreen().unwrap_or(false)
        {
            return;
        }
        let mut state = data.lock().unwrap();
        if let Some(normal) = read(&window_copy) {
            state.0 = normal;
        }
        state.0.maximized = window_copy.is_maximized().unwrap_or(false);
        state.1 = state.1.wrapping_add(1);
        let revision = state.1;
        let save = |geometry: &Geometry| {
            let _ = std::fs::create_dir_all(&dir);
            if let Ok(bytes) = serde_json::to_vec(geometry) {
                let _ = std::fs::write(&path, bytes);
            }
        };
        if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
            save(&state.0);
            return;
        }
        drop(state);
        let data = data.clone();
        let path = path.clone();
        let dir = dir.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            let state = data.lock().unwrap();
            if state.1 != revision {
                return;
            }
            let _ = std::fs::create_dir_all(dir);
            if let Ok(bytes) = serde_json::to_vec(&state.0) {
                let _ = std::fs::write(path, bytes);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removed_display_and_changed_dpi_keep_window_inside_work_area() {
        let saved = Geometry {
            x: 4000,
            y: -900,
            width: 2000.0,
            height: 1500.0,
            maximized: false,
        };
        let area = WorkArea {
            x: -1920,
            y: 24,
            width: 1920,
            height: 1056,
            scale: 2.0,
        };
        let result = fit(&saved, &area);
        assert!(result.x >= area.x && result.y >= area.y);
        assert!(result.x as f64 + result.width * area.scale <= 0.0);
        assert!(result.y as f64 + result.height * area.scale <= 1080.0);
        assert!(result.width < saved.width && result.height < saved.height);
    }
}
