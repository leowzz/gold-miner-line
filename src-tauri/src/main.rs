#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod config;
mod tracking;
use config::{Bounds, Config, Settings, SettingsPatch};
use goldline::vision;
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    settings: Settings,
    visible: bool,
    calibrating: bool,
    revision: u64,
    notice: Option<String>,
    save_error: Option<String>,
    tracking: vision::Options,
    tracking_enabled: bool,
}

struct Inner {
    config: Config,
    snapshot: Snapshot,
    dirty: Option<Instant>,
}
struct Model {
    inner: Mutex<Inner>,
    path: PathBuf,
}

fn publish(app: &tauri::AppHandle, inner: &mut Inner) -> Snapshot {
    inner.snapshot.settings = inner.config.settings.clone();
    inner.snapshot.tracking = inner.config.tracking.clone();
    inner.snapshot.revision += 1;
    let snapshot = inner.snapshot.clone();
    let _ = app.emit("state-changed", &snapshot);
    snapshot
}

#[tauri::command]
fn get_state(model: tauri::State<Model>) -> Snapshot {
    model.inner.lock().unwrap().snapshot.clone()
}

#[tauri::command]
fn set_tracking(
    app: tauri::AppHandle,
    model: tauri::State<Model>,
    enabled: Option<bool>,
    options: Option<vision::OptionsPatch>,
) -> Result<Snapshot, String> {
    if enabled == Some(true) {
        capture::request_permission();
    }
    let mut inner = model.inner.lock().unwrap();
    if let Some(options) = options {
        inner.config.tracking = options.apply(&inner.config.tracking)?;
        inner.dirty = Some(Instant::now());
    }
    if let Some(enabled) = enabled {
        inner.snapshot.tracking_enabled = enabled;
    }
    Ok(publish(&app, &mut inner))
}

#[tauri::command]
fn update_settings(
    app: tauri::AppHandle,
    model: tauri::State<Model>,
    patch: SettingsPatch,
) -> Result<Snapshot, String> {
    let mut inner = model.inner.lock().unwrap();
    inner.config.settings = patch.apply(&inner.config.settings)?;
    inner.dirty = Some(Instant::now());
    Ok(publish(&app, &mut inner))
}

#[tauri::command]
fn reset_settings(app: tauri::AppHandle, model: tauri::State<Model>) -> Snapshot {
    let mut inner = model.inner.lock().unwrap();
    inner.config.settings = Settings::default();
    inner.config.tracking = vision::Options::default();
    inner.dirty = Some(Instant::now());
    publish(&app, &mut inner)
}

fn apply_mode(
    app: &tauri::AppHandle,
    visible: Option<bool>,
    calibrating: Option<bool>,
) -> Result<Snapshot, String> {
    let model = app.state::<Model>();
    // Do not hold the state lock across native window calls; those can emit geometry events.
    let current = model.inner.lock().unwrap().snapshot.clone();
    let visible = visible.unwrap_or(current.visible);
    let calibrating = calibrating.unwrap_or(current.calibrating);
    let overlay = app
        .get_webview_window("overlay")
        .ok_or("辅助窗口未能加载，请重启应用。")?;
    overlay
        .set_ignore_cursor_events(!calibrating)
        .map_err(|e| e.to_string())?;
    overlay
        .set_focusable(calibrating)
        .map_err(|e| e.to_string())?;
    if visible {
        overlay.show()
    } else {
        overlay.hide()
    }
    .map_err(|e| e.to_string())?;
    let mut inner = model.inner.lock().unwrap();
    inner.snapshot.visible = visible;
    inner.snapshot.calibrating = calibrating;
    Ok(publish(app, &mut inner))
}

#[tauri::command]
fn set_mode(
    app: tauri::AppHandle,
    visible: Option<bool>,
    calibrating: Option<bool>,
) -> Result<Snapshot, String> {
    apply_mode(&app, visible, calibrating)
}

#[tauri::command]
fn recover_overlay(app: tauri::AppHandle) -> Result<Snapshot, String> {
    let overlay = app
        .get_webview_window("overlay")
        .ok_or("辅助窗口未能加载")?;
    let main = app.get_webview_window("main").ok_or("控制面板未能加载")?;
    if let Some(monitor) = main.current_monitor().map_err(|e| e.to_string())? {
        let area = monitor.work_area();
        let b = config::fit_bounds(
            &Bounds {
                x: area.position.x + 40,
                y: area.position.y + 40,
                width: 960.0,
                height: 636.0,
            },
            area.position.x,
            area.position.y,
            area.size.width,
            area.size.height,
            monitor.scale_factor(),
        );
        overlay
            .set_position(PhysicalPosition::new(b.x, b.y))
            .map_err(|e| e.to_string())?;
        overlay
            .set_size(tauri::LogicalSize::new(b.width, b.height))
            .map_err(|e| e.to_string())?;
    }
    apply_mode(&app, Some(true), Some(true))
}

fn capture_bounds(window: &tauri::Window) {
    if let (Ok(pos), Ok(size), Ok(scale)) = (
        window.outer_position(),
        window.inner_size(),
        window.scale_factor(),
    ) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        let model = window.state::<Model>();
        let mut inner = model.inner.lock().unwrap();
        let logical = size.to_logical::<f64>(scale);
        inner.config.bounds = Some(Bounds {
            x: pos.x,
            y: pos.y,
            width: logical.width,
            height: logical.height,
        });
        inner.dirty = Some(Instant::now());
        publish(window.app_handle(), &mut inner);
    }
}

fn flush(app: &tauri::AppHandle, force: bool) {
    let model = app.state::<Model>();
    let mut inner = model.inner.lock().unwrap();
    if !inner
        .dirty
        .is_some_and(|t| force || t.elapsed() >= Duration::from_millis(500))
    {
        return;
    }
    let previous_error = inner.snapshot.save_error.clone();
    match config::save(&model.path, &inner.config) {
        Ok(()) => {
            inner.dirty = None;
            inner.snapshot.save_error = None;
        }
        Err(_) => {
            inner.dirty = Some(Instant::now() + Duration::from_secs(4));
            inner.snapshot.save_error =
                Some("参数未能保存到本机，请检查磁盘空间和目录权限。应用会自动重试。".into());
        }
    }
    if previous_error != inner.snapshot.save_error {
        publish(app, &mut inner);
    }
}

fn main() {
    let app = tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    let snapshot = app.state::<Model>().inner.lock().unwrap().snapshot.clone();
                    let result = if shortcut.key == Code::KeyG {
                        apply_mode(app, Some(!snapshot.visible), None)
                    } else {
                        apply_mode(app, Some(true), Some(!snapshot.calibrating))
                    };
                    if let Err(error) = result {
                        let model = app.state::<Model>();
                        let mut inner = model.inner.lock().unwrap();
                        inner.snapshot.notice = Some(format!("切换辅助窗口失败：{error}"));
                        publish(app, &mut inner);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let path = app.path().app_config_dir()?.join("settings.yaml");
            let (config, notice) = match config::load(&path) {
                Ok(config) => (config, None),
                Err(_) => (
                    Config::default(),
                    Some("原有配置无法读取，已使用默认参数。调整后会保存新的配置。".into()),
                ),
            };
            let snapshot = Snapshot {
                settings: config.settings.clone(),
                visible: true,
                calibrating: true,
                revision: 0,
                notice,
                save_error: None,
                tracking: config.tracking.clone(),
                tracking_enabled: false,
            };
            let saved_bounds = config.bounds.clone();
            app.manage(Model {
                inner: Mutex::new(Inner {
                    config,
                    snapshot,
                    dirty: None,
                }),
                path,
            });

            let mut bounds = saved_bounds.unwrap_or(Bounds {
                x: 80,
                y: 100,
                width: 960.0,
                height: 636.0,
            });
            let monitors = app.available_monitors()?;
            let monitor = monitors
                .iter()
                .find(|m| {
                    let area = m.work_area();
                    bounds.x >= area.position.x
                        && bounds.y >= area.position.y
                        && (bounds.x as i64) < area.position.x as i64 + area.size.width as i64
                        && (bounds.y as i64) < area.position.y as i64 + area.size.height as i64
                })
                .cloned()
                .or(app.primary_monitor()?);
            if let Some(m) = monitor {
                let area = m.work_area();
                bounds = config::fit_bounds(
                    &bounds,
                    area.position.x,
                    area.position.y,
                    area.size.width,
                    area.size.height,
                    m.scale_factor(),
                );
            }
            let overlay = WebviewWindowBuilder::new(
                app,
                "overlay",
                WebviewUrl::App("index.html?overlay".into()),
            )
            .title("黄金矿工 · 校准覆盖层")
            .transparent(true)
            .decorations(false)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .visible(false)
            .resizable(true)
            .min_inner_size(240.0, 180.0)
            .inner_size(bounds.width, bounds.height)
            .build()?;
            overlay.set_position(PhysicalPosition::new(bounds.x, bounds.y))?;
            overlay.show()?;
            // Keep the controls reachable above the overlay on first launch.
            if let Some(main) = app.get_webview_window("main") {
                main.set_focus()?;
            }

            for (key, label) in [(Code::KeyG, "Ctrl+Shift+G"), (Code::KeyL, "Ctrl+Shift+L")] {
                let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), key);
                if app.global_shortcut().register(shortcut).is_err() {
                    let model = app.state::<Model>();
                    let mut inner = model.inner.lock().unwrap();
                    let message = format!(
                        "快捷键 {label} 注册失败，可使用面板按钮。请关闭占用快捷键的应用后重启。"
                    );
                    inner.snapshot.notice = Some(match inner.snapshot.notice.take() {
                        Some(previous) => format!("{previous}\n{message}"),
                        None => message,
                    });
                }
            }
            let handle = app.handle().clone();
            tracking::start(handle.clone());
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(250));
                flush(&handle, false);
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "overlay" {
                match event {
                    WindowEvent::Moved(_)
                    | WindowEvent::Resized(_)
                    | WindowEvent::ScaleFactorChanged { .. } => capture_bounds(window),
                    WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        let _ = apply_mode(window.app_handle(), Some(false), None);
                    }
                    _ => {}
                }
            }
            if window.label() == "main" && matches!(event, WindowEvent::CloseRequested { .. }) {
                flush(window.app_handle(), true);
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            update_settings,
            reset_settings,
            set_mode,
            recover_overlay,
            set_tracking
        ])
        .build(tauri::generate_context!())
        .expect("无法启动黄金矿工辅助线");
    app.run(|handle, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            flush(handle, true);
        }
    });
}
