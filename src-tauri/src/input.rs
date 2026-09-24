//! Only calibration controls intercept the mouse; transparent space belongs to the game.
use serde::Deserialize;
use std::{sync::Mutex, time::Duration};
use tauri::Manager;

#[derive(Clone, Copy, Deserialize)]
pub struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Rect {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }

    fn valid(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|n| n.is_finite())
            && self.width > 0.0
            && self.height > 0.0
    }
}

pub struct Input {
    regions: Vec<Rect>,
    viewport: (f64, f64),
    ignoring: bool,
    reported_error: bool,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            regions: vec![],
            viewport: (0.0, 0.0),
            ignoring: true,
            reported_error: false,
        }
    }
}

#[tauri::command]
pub fn set_input_regions(
    window: tauri::WebviewWindow,
    input: tauri::State<Mutex<Input>>,
    regions: Vec<Rect>,
    width: f64,
    height: f64,
) -> Result<(), String> {
    if window.label() != "overlay"
        || regions.len() > 64
        || !regions.iter().all(Rect::valid)
        || !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
    {
        return Err("无法更新校准控件的鼠标区域。".into());
    }
    let mut input = input.lock().unwrap();
    input.regions = regions;
    input.viewport = (width, height);
    Ok(())
}

// Keep whichever window received mouse-down in control until release. This also
// prevents a drag that started in the game from being stolen by a calibration handle.
fn should_ignore(active: bool, button_down: bool, ignoring: bool, hit: bool) -> bool {
    !active || if button_down { ignoring } else { !hit }
}

fn local_point(cursor: (f64, f64), position: (f64, f64), scale: f64) -> (f64, f64) {
    (
        (cursor.0 - position.0) / scale,
        (cursor.1 - position.1) / scale,
    )
}

#[cfg(target_os = "macos")]
fn pointer() -> Result<((f64, f64), bool), String> {
    use std::ffi::c_void;
    #[repr(C)]
    struct Point {
        x: f64,
        y: f64,
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreate(source: *const c_void) -> *const c_void;
        fn CGEventGetLocation(event: *const c_void) -> Point;
        fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: *const c_void);
    }
    // Read-only mouse state needs no event tap, input injection or recording grant.
    unsafe {
        let event = CGEventCreate(std::ptr::null());
        if event.is_null() {
            return Err("无法读取鼠标位置。".into());
        }
        let point = CGEventGetLocation(event);
        CFRelease(event);
        Ok((
            (point.x, point.y),
            (0..5).any(|button| CGEventSourceButtonState(0, button)),
        ))
    }
}

#[cfg(target_os = "windows")]
fn pointer() -> Result<((f64, f64), bool), String> {
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetCursorPos(point: *mut Point) -> i32;
        fn GetAsyncKeyState(key: i32) -> i16;
    }
    unsafe {
        let mut point = Point { x: 0, y: 0 };
        if GetCursorPos(&mut point) == 0 {
            return Err("无法读取鼠标位置。".into());
        }
        Ok((
            (point.x as f64, point.y as f64),
            [1, 2, 4, 5, 6].iter().any(|key| GetAsyncKeyState(*key) < 0),
        ))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn pointer() -> Result<((f64, f64), bool), String> {
    Err("局部鼠标穿透目前支持 Windows 和 macOS。".into())
}

pub fn refresh(app: &tauri::AppHandle) -> Result<(), String> {
    let active = {
        let model = app.state::<crate::Model>();
        let inner = model.inner.lock().unwrap();
        inner.snapshot.visible && inner.snapshot.calibrating
    };
    let overlay = app
        .get_webview_window("overlay")
        .ok_or("辅助窗口未能加载")?;
    let state = app.state::<Mutex<Input>>();
    let (hit, down) = if active {
        let (cursor, down) = pointer()?;
        let p = overlay.inner_position().map_err(|e| e.to_string())?;
        let size = overlay.inner_size().map_err(|e| e.to_string())?;
        let scale = overlay.scale_factor().map_err(|e| e.to_string())?;
        // CoreGraphics uses desktop points; Windows uses physical pixels. Divide
        // the macOS window position by its own screen scale, not the primary screen's.
        let unit = if cfg!(target_os = "macos") {
            scale
        } else {
            1.0
        };
        let (x, y) = local_point(cursor, (p.x as f64 / unit, p.y as f64 / unit), scale / unit);
        let input = state.lock().unwrap();
        let same_size = (input.viewport.0 - size.width as f64 / scale).abs() < 1.0
            && (input.viewport.1 - size.height as f64 / scale).abs() < 1.0;
        (
            same_size && input.regions.iter().any(|r| r.contains(x, y)),
            down,
        )
    } else {
        (false, false)
    };
    let ignoring = state.lock().unwrap().ignoring;
    let next = should_ignore(active, down, ignoring, hit);
    if next != ignoring {
        overlay
            .set_ignore_cursor_events(next)
            .map_err(|e| e.to_string())?;
        state.lock().unwrap().ignoring = next;
    }
    Ok(())
}

pub fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        let handle = app.clone();
        let (done, completed) = std::sync::mpsc::sync_channel(1);
        // Native calls run on the UI thread. Wait for completion so a busy UI
        // cannot accumulate polling callbacks. No WebView mousemove is required
        // to re-enable controls after the whole native window becomes transparent.
        if app
            .run_on_main_thread(move || {
                if let Err(error) = refresh(&handle) {
                    let state = handle.state::<Mutex<Input>>();
                    let mut input = state.lock().unwrap();
                    if !input.reported_error {
                        input.reported_error = true;
                        let model = handle.state::<crate::Model>();
                        let mut inner = model.inner.lock().unwrap();
                        inner.snapshot.notice = Some(format!(
                            "校准鼠标穿透不可用：{error} 可先锁定辅助线操作游戏。"
                        ));
                        crate::publish(&handle, &mut inner);
                    }
                }
                let _ = done.send(());
            })
            .is_err()
            || completed.recv().is_err()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_are_hit_in_logical_coordinates_on_scaled_negative_screens() {
        let control = Rect {
            x: 100.0,
            y: 50.0,
            width: 32.0,
            height: 32.0,
        };
        for scale in [1.0, 1.5, 2.0] {
            let origin = (-1920.0, -200.0);
            let cursor = (origin.0 + 110.0 * scale, origin.1 + 60.0 * scale);
            let point = local_point(cursor, origin, scale);
            assert!(control.contains(point.0, point.1));
        }
        assert!(!control.contains(99.0, 60.0));
        assert!(!control.contains(132.0, 60.0));
    }

    #[test]
    fn drags_keep_their_original_receiver_and_modes_override_capture() {
        assert!(should_ignore(true, false, false, false)); // blank area
        assert!(!should_ignore(true, false, true, true)); // return to control
        assert!(!should_ignore(true, true, false, false)); // drag out of control
        assert!(should_ignore(true, true, true, true)); // game drag crosses control
        assert!(should_ignore(false, true, false, true)); // lock/hide during drag
        assert!(should_ignore(true, false, false, false)); // release over blank
    }
}
