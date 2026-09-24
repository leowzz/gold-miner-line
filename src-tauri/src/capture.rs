use tauri::WebviewWindow;
use xcap::{image::RgbaImage, Monitor};

pub struct Frame {
    pub image: RgbaImage,
    pub region: crate::vision::Region,
    pub scale: f64,
    pub geometry: (i32, i32, u32, u32),
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Rect {
    fn overlaps(self, other: Self) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }
}

fn window_rect(window: &WebviewWindow) -> Result<(Rect, f64), String> {
    let p = window.inner_position().map_err(|e| e.to_string())?;
    let s = window.inner_size().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    // CoreGraphics uses global logical coordinates, GDI global physical pixels.
    let unit = if cfg!(target_os = "macos") {
        scale
    } else {
        1.0
    };
    Ok((
        Rect {
            x: p.x as f64 / unit,
            y: p.y as f64 / unit,
            width: s.width as f64 / unit,
            height: s.height as f64 / unit,
        },
        scale / unit,
    ))
}

pub fn geometry(window: &WebviewWindow) -> Result<(i32, i32, u32, u32), String> {
    let p = window.inner_position().map_err(|e| e.to_string())?;
    let s = window.inner_size().map_err(|e| e.to_string())?;
    Ok((p.x, p.y, s.width, s.height))
}

pub fn permission_granted() -> bool {
    #[cfg(target_os = "macos")]
    {
        #[link(name = "CoreGraphics", kind = "framework")]
        unsafe extern "C" {
            fn CGPreflightScreenCaptureAccess() -> bool;
        }
        // Query only. Granting screen recording remains a user action in Settings.
        unsafe { CGPreflightScreenCaptureAccess() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn request_permission() {
    #[cfg(target_os = "macos")]
    if !permission_granted() {
        #[link(name = "CoreGraphics", kind = "framework")]
        unsafe extern "C" {
            fn CGRequestScreenCaptureAccess() -> bool;
        }
        // Called only after the user turns on recognition. The OS owns consent.
        unsafe {
            CGRequestScreenCaptureAccess();
        }
    }
}

pub fn short_rope(
    window: &WebviewWindow,
    panel: &WebviewWindow,
    roi: crate::vision::Region,
) -> Result<Frame, String> {
    if !permission_granted() {
        return Err(
            "请在系统设置 → 隐私与安全性 → 屏幕与系统音频录制中允许 Goldline，然后重启应用。"
                .into(),
        );
    }
    let before = geometry(window)?;
    let (bounds, unit) = window_rect(window)?;
    let center = (
        bounds.x + (roi.x + roi.width / 2.0) * bounds.width,
        bounds.y + (roi.y + roi.height / 2.0) * bounds.height,
    );
    let x = (bounds.x + roi.x * bounds.width).floor();
    let y = (bounds.y + roi.y * bounds.height).floor();
    let region = Rect {
        x,
        y,
        width: (bounds.x + (roi.x + roi.width) * bounds.width).ceil() - x,
        height: (bounds.y + (roi.y + roi.height) * bounds.height).ceil() - y,
    };
    // A control panel covering the rope would produce plausible but wrong edges.
    if panel.is_visible().unwrap_or(true) && !panel.is_minimized().unwrap_or(false) {
        let (panel, _) = window_rect(panel)?;
        if region.overlaps(panel) {
            return Err("请把控制面板移出短线识别区域。".into());
        }
    }
    let monitor = Monitor::from_point(center.0.round() as i32, center.1.round() as i32)
        .map_err(|_| "识别框不在可用屏幕内，请重新校准。")?;
    let mx = monitor.x().map_err(|e| e.to_string())? as f64;
    let my = monitor.y().map_err(|e| e.to_string())? as f64;
    let mw = monitor.width().map_err(|e| e.to_string())? as f64;
    let mh = monitor.height().map_err(|e| e.to_string())? as f64;
    if x < mx || y < my || x + region.width > mx + mw || y + region.height > my + mh {
        return Err("识别区域跨越屏幕边缘，请把识别框移到同一块屏幕内。".into());
    }
    let image = monitor
        .capture_region(
            (x - mx) as u32,
            (y - my) as u32,
            region.width as u32,
            region.height as u32,
        )
        .map_err(|e| format!("无法读取短线画面，请检查屏幕录制权限或重启应用。详情：{e}"))?;
    if before != geometry(window)? {
        return Err("覆盖窗口正在移动，请稍候。".into());
    }
    let image_scale = image.width() as f64 / region.width;
    Ok(Frame {
        region: crate::vision::Region {
            x: (x - bounds.x) / bounds.width,
            y: (y - bounds.y) / bounds.height,
            width: region.width / bounds.width,
            height: region.height / bounds.height,
        },
        scale: unit * image_scale,
        image,
        geometry: before,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn panel_overlap_supports_negative_desktop_coordinates() {
        let roi = Rect {
            x: -800.0,
            y: 200.0,
            width: 50.0,
            height: 50.0,
        };
        assert!(roi.overlaps(Rect {
            x: -820.0,
            y: 210.0,
            width: 440.0,
            height: 820.0
        }));
        assert!(!roi.overlaps(Rect {
            x: -750.0,
            y: 200.0,
            width: 440.0,
            height: 820.0
        }));
    }
}
