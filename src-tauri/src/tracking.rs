use crate::{capture, vision, Model};
use goldline::preview;
use serde::Serialize;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager};

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub revision: u64,
    pub angle: Option<f64>,
    pub origin: Option<vision::Point>,
    pub confidence: f64,
    pub message: String,
    pub captured_at: u64,
}

// Require two consistent detections to acquire / reacquire. Never keep a line
// after a missing frame, and never interpolate through an implausible angle jump.
#[derive(Default)]
struct Filter {
    previous: Option<f64>,
    last_time: Option<Instant>,
}

impl Filter {
    fn reset(&mut self) {
        *self = Self::default();
    }
    fn push(&mut self, angle: Option<f64>, now: Instant) -> Option<f64> {
        let Some(next) = angle else {
            self.reset();
            return None;
        };
        let dt = self
            .last_time
            .map(|t| now.saturating_duration_since(t).as_secs_f64());
        let result = match (self.previous, dt) {
            (Some(previous), Some(dt))
                if dt < 0.25 && (next - previous).abs() <= 10.0 + dt * 240.0 =>
            {
                Some(next)
            }
            _ => None,
        };
        self.previous = Some(next);
        self.last_time = Some(now);
        result
    }
}

pub fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut filter = Filter::default();
        let mut last_revision = u64::MAX;
        let mut last_preview: Option<Instant> = None;
        loop {
            std::thread::sleep(Duration::from_millis(33));
            let snapshot = app.state::<Model>().inner.lock().unwrap().snapshot.clone();
            if !snapshot.tracking_enabled || !snapshot.visible {
                filter.reset();
                last_revision = u64::MAX;
                last_preview = None;
                continue;
            }
            if last_revision != snapshot.revision {
                filter.reset();
                last_revision = snapshot.revision;
                last_preview = None;
                // Allow the WebView to clear the scan area before reading it.
                std::thread::sleep(Duration::from_millis(120));
                continue;
            }
            let (Some(overlay), Some(panel)) = (
                app.get_webview_window("overlay"),
                app.get_webview_window("main"),
            ) else {
                continue;
            };
            let start = Instant::now();
            let captured_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            // Diagnostic images go only to the panel and are capped at 8 fps.
            let wants_preview =
                last_preview.is_none_or(|t| t.elapsed() >= Duration::from_millis(125));
            let captured = capture::claw_region(&overlay, &panel, snapshot.tracking.region);
            let mut origin = None;
            let mut preview_frame = None;
            let mut preview_error = None;
            let mut status = "captureError";
            let mut message;
            let detection = match captured {
                Ok(frame) => {
                    let analysis = vision::analyze(
                        &frame.image,
                        frame.scale,
                        &snapshot.tracking,
                        wants_preview,
                    );
                    let elapsed = start.elapsed();
                    (status, message) = match analysis.status {
                        vision::Status::Invalid => ("invalid", "识别框过小，请扩大范围，完整包含两侧夹爪。".to_string()),
                        vision::Status::NoMetal => ("noMetal", "未找到足够的灰色像素，请检查识别框位置或灰色容差。".into()),
                        vision::Status::NoMatch => ("noMatch", "找到灰色区域，但未匹配完整双爪；请检查遮挡、夹子是否张开和识别框范围。".into()),
                        vision::Status::Ambiguous => ("ambiguous", "多个夹口候选无法区分，请缩小识别框，排除其他灰色物体。".into()),
                        vision::Status::Detected => ("detected", "正在跟随夹口中垂线".into()),
                    };
                    if capture::geometry(&overlay).ok() != Some(frame.geometry) {
                        status = "captureError";
                        message = "覆盖窗口正在移动，请稍候。".into();
                        None
                    } else {
                        if wants_preview {
                            match preview::frame(
                                &frame.image,
                                &analysis,
                                elapsed.as_millis() as u64,
                            ) {
                                Ok(encoded) => preview_frame = Some(encoded),
                                Err(error) => {
                                    preview_error = Some(format!("无法生成识别预览：{error}"));
                                }
                            }
                        }
                        if elapsed >= Duration::from_millis(250) {
                            status = "slow";
                            message = "画面采集或识别过慢，已暂停方向线；可缩小识别框。".into();
                            None
                        } else {
                            origin = analysis.detection.map(|d| vision::Point {
                                x: frame.region.x
                                    + d.start.x / frame.image.width() as f64 * frame.region.width,
                                y: frame.region.y
                                    + d.start.y / frame.image.height() as f64 * frame.region.height,
                            });
                            analysis.detection
                        }
                    }
                }
                Err(error) => {
                    message = error;
                    None
                }
            };
            let angle = if snapshot.calibrating {
                filter.reset();
                if detection.is_some() {
                    message = "已识别夹口，锁定后在游戏上绘制方向线。".into();
                }
                None
            } else {
                filter.push(detection.map(|d| d.angle), Instant::now())
            };
            if detection.is_some() && angle.is_none() && !snapshot.calibrating {
                status = "confirming";
                message = "正在确认夹口方向…".into();
            }
            let update = Update {
                revision: snapshot.revision,
                angle,
                origin: angle.and(origin),
                confidence: detection.map(|d| d.confidence).unwrap_or(0.0),
                captured_at,
                message: message.clone(),
            };
            let model = app.state::<Model>();
            let inner = model.inner.lock().unwrap();
            // Drop work captured before a mode, geometry or parameter change.
            if inner.snapshot.revision == snapshot.revision {
                let _ = app.emit("tracking-updated", update);
                if wants_preview {
                    let (preview_status, preview_message) = match preview_error {
                        Some(error) => ("previewError", error),
                        None => (status, message),
                    };
                    let _ = app.emit_to(
                        "main",
                        "preview-updated",
                        preview::Update {
                            revision: snapshot.revision,
                            captured_at,
                            status: preview_status.into(),
                            message: preview_message,
                            frame: preview_frame,
                        },
                    );
                    last_preview = Some(Instant::now());
                }
            }
            drop(inner);
            if detection.is_none() {
                std::thread::sleep(Duration::from_millis(120));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requires_reacquisition_after_jump_loss_and_stall() {
        let mut filter = Filter::default();
        let now = Instant::now();
        assert_eq!(filter.push(Some(-40.0), now), None);
        assert_eq!(
            filter.push(Some(-38.0), now + Duration::from_millis(33)),
            Some(-38.0)
        );
        assert_eq!(
            filter.push(Some(50.0), now + Duration::from_millis(66)),
            None
        );
        assert_eq!(
            filter.push(Some(51.0), now + Duration::from_millis(99)),
            Some(51.0)
        );
        assert_eq!(filter.push(None, now + Duration::from_millis(120)), None);
        assert_eq!(
            filter.push(Some(51.0), now + Duration::from_millis(132)),
            None
        );
        assert_eq!(filter.push(Some(52.0), now + Duration::from_secs(1)), None);
    }
}
