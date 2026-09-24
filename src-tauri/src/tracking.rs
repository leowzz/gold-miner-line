use crate::{capture, vision, Model};
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
        loop {
            std::thread::sleep(Duration::from_millis(33));
            let snapshot = app.state::<Model>().inner.lock().unwrap().snapshot.clone();
            if !snapshot.tracking_enabled || snapshot.calibrating || !snapshot.visible {
                filter.reset();
                last_revision = u64::MAX;
                continue;
            }
            if last_revision != snapshot.revision {
                filter.reset();
                last_revision = snapshot.revision;
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
            let captured = capture::short_rope(&overlay, &panel, snapshot.tracking.region);
            let (detection, message) = match captured {
                Ok(frame) if start.elapsed() < Duration::from_millis(200) => {
                    let result = vision::detect(&frame.image, frame.scale, &snapshot.tracking).map(
                        |mut d| {
                            d.start.x = frame.region.x
                                + d.start.x / frame.image.width() as f64 * frame.region.width;
                            d.start.y = frame.region.y
                                + d.start.y / frame.image.height() as f64 * frame.region.height;
                            d
                        },
                    );
                    if capture::geometry(&overlay).ok() != Some(frame.geometry) {
                        (None, "覆盖窗口正在移动，请稍候。".into())
                    } else {
                        (
                            result,
                            "未找到唯一清晰短线，请调整识别框，避开轮子、支架和钩爪。".into(),
                        )
                    }
                }
                Ok(_) => (None, "画面读取过慢，已暂停方向线。".into()),
                Err(message) => (None, message),
            };
            if detection.is_none() {
                filter.reset();
            }
            let angle = filter.push(detection.map(|d| d.angle), Instant::now());
            let update = Update {
                revision: snapshot.revision,
                angle,
                origin: angle.and(detection.map(|d| d.start)),
                confidence: detection.map(|d| d.confidence).unwrap_or(0.0),
                captured_at,
                message: if angle.is_some() {
                    "正在跟随短线".into()
                } else if detection.is_some() {
                    "正在确认短线方向…".into()
                } else {
                    message
                },
            };
            let model = app.state::<Model>();
            let inner = model.inner.lock().unwrap();
            // Drop work captured before a mode, geometry or parameter change.
            if inner.snapshot.revision == snapshot.revision {
                let _ = app.emit("tracking-updated", update);
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
