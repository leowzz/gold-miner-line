use serde::{Deserialize, Serialize};
use xcap::image::{imageops, RgbaImage};

/// Relative to the overlay's content area, independent of the reference fan.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Default for Region {
    fn default() -> Self {
        Self {
            x: 0.465,
            y: 0.14,
            width: 0.07,
            height: 0.06,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "StoredOptions")]
pub struct Options {
    pub region: Region,
    pub darkness: f64,
    pub show_reference: bool,
}

// Accept the previous radial detector's fields without resetting the user's
// entire YAML configuration. New saves contain only the rectangle options.
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct StoredOptions {
    region: Option<Region>,
    darkness: Option<f64>,
    show_reference: Option<bool>,
    #[serde(rename = "skipRadius")]
    _skip_radius: Option<f64>,
    #[serde(rename = "searchRadius")]
    _search_radius: Option<f64>,
}
impl From<StoredOptions> for Options {
    fn from(value: StoredOptions) -> Self {
        Self {
            region: value.region.unwrap_or_default(),
            darkness: value.darkness.unwrap_or(115.0),
            show_reference: value.show_reference.unwrap_or(false),
        }
    }
}
impl Default for Options {
    fn default() -> Self {
        StoredOptions::default().into()
    }
}
impl Options {
    pub fn validate(&self) -> Result<(), String> {
        let r = self.region;
        if [r.x, r.y, r.width, r.height, self.darkness]
            .iter()
            .any(|v| !v.is_finite())
            || r.x < 0.0
            || r.y < 0.0
            || !(0.005..=1.0).contains(&r.width)
            || !(0.005..=1.0).contains(&r.height)
            || r.x + r.width > 1.0 + 1e-9
            || r.y + r.height > 1.0 + 1e-9
            || !(40.0..=180.0).contains(&self.darkness)
        {
            return Err(
                "识别框需完整位于游戏画面内，宽高至少为 0.5%；亮度上限需在 40–180 之间。".into(),
            );
        }
        Ok(())
    }
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OptionsPatch {
    region: Option<Region>,
    darkness: Option<f64>,
    show_reference: Option<bool>,
}
impl OptionsPatch {
    pub fn apply(self, current: &Options) -> Result<Options, String> {
        let options = Options {
            region: self.region.unwrap_or(current.region),
            darkness: self.darkness.unwrap_or(current.darkness),
            show_reference: self.show_reference.unwrap_or(current.show_reference),
        };
        options.validate()?;
        Ok(options)
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
#[derive(Debug, Clone, Copy)]
pub struct Detection {
    pub angle: f64,
    pub confidence: f64,
    /// Endpoints in capture pixels, oriented from upper to lower endpoint.
    pub start: Point,
    pub end: Point,
}

fn dark(image: &RgbaImage, x: f64, y: f64, threshold: f64) -> bool {
    let (x, y) = (x.round() as i32, y.round() as i32);
    // Outside the crop is unknown, never evidence of a contrasting background.
    if x < 0 || y < 0 || x >= image.width() as i32 || y >= image.height() as i32 {
        return true;
    }
    let p = image.get_pixel(x as u32, y as u32).0;
    let low = p[0].min(p[1]).min(p[2]);
    let high = p[0].max(p[1]).max(p[2]);
    p[3] >= 128
        && high - low <= 48
        && 0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64 <= threshold
}

/// Hough candidates followed by continuous, narrow-segment scoring. The whole
/// crop is searched; no pivot or expected line position is supplied. Work is
/// bounded by reducing Retina images and large crops to at most 256px per side.
pub fn detect(image: &RgbaImage, scale: f64, options: &Options) -> Option<Detection> {
    if options.validate().is_err()
        || !scale.is_finite()
        || scale <= 0.0
        || image.width() < 8
        || image.height() < 8
    {
        return None;
    }
    let reduction = scale
        .max(image.width().max(image.height()) as f64 / 256.0)
        .max(1.0);
    let small = imageops::resize(
        image,
        (image.width() as f64 / reduction).round() as u32,
        (image.height() as f64 / reduction).round() as u32,
        imageops::FilterType::Triangle,
    );
    let points: Vec<(f64, f64)> = small
        .enumerate_pixels()
        .filter(|(x, y, _)| dark(&small, *x as f64, *y as f64, options.darkness))
        .map(|(x, y, _)| (x as f64, y as f64))
        .collect();
    if points.len() < 8 || points.len() > (small.width() * small.height()) as usize / 2 {
        return None;
    }
    let diagonal = (small.width() as f64).hypot(small.height() as f64).ceil() as usize;
    let bins = diagonal * 2 + 1;
    let mut peaks = Vec::new();
    for degree in (-90..90).step_by(2) {
        let (dx, dy) = (degree as f64).to_radians().sin_cos();
        let mut votes = vec![0u32; bins];
        for &(x, y) in &points {
            votes[(x * dy - y * dx + diagonal as f64).round() as usize] += 1;
        }
        for (bin, &vote) in votes.iter().enumerate() {
            if vote >= 7 {
                peaks.push((vote, degree as f64, bin as f64 - diagonal as f64));
            }
        }
    }
    peaks.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    let mut candidates: Vec<(Detection, f64)> = Vec::new();
    // Suppress near-identical Hough peaks before expensive segment verification.
    let mut selected: Vec<(f64, f64)> = Vec::new();
    for (_, degree, rho) in peaks {
        if selected
            .iter()
            .any(|(a, r)| (a - degree).abs() < 5.0 && (r - rho).abs() < 3.0)
        {
            continue;
        }
        selected.push((degree, rho));
        let (dx, dy) = degree.to_radians().sin_cos();
        let mut along: Vec<f64> = points
            .iter()
            .filter(|(x, y)| (x * dy - y * dx - rho).abs() <= 1.25)
            .map(|(x, y)| x * dx + y * dy)
            .collect();
        along.sort_unstable_by(f64::total_cmp);
        let mut first = 0;
        for last in 1..=along.len() {
            if last < along.len() && along[last] - along[last - 1] <= 2.5 {
                continue;
            }
            let lo = along[first];
            let hi = along[last - 1];
            first = last;
            let length = hi - lo;
            if length < (7.0 * scale / reduction).max(6.0) {
                continue;
            }
            let mut hits = 0.0;
            let mut contrast = 0.0;
            let n = length.ceil() as usize + 1;
            for i in 0..n {
                let t = lo + length * i as f64 / (n - 1) as f64;
                let (x, y) = (dx * t + dy * rho, dy * t - dx * rho);
                let center = [-0.5, 0.0, 0.5]
                    .iter()
                    .any(|o| dark(&small, x + dy * o, y - dx * o, options.darkness));
                let flank = [-3.5, 3.5]
                    .iter()
                    .filter(|o| dark(&small, x + dy * *o, y - dx * *o, options.darkness))
                    .count() as f64
                    / 2.0;
                if center {
                    hits += 1.0;
                    contrast += 1.0 - flank;
                }
            }
            let coverage = hits / n as f64;
            let contrast = contrast / n as f64;
            if coverage < 0.8 || contrast < 0.65 {
                continue;
            }
            let start = Point {
                x: (dx * lo + dy * rho) * reduction,
                y: (dy * lo - dx * rho) * reduction,
            };
            let end = Point {
                x: (dx * hi + dy * rho) * reduction,
                y: (dy * hi - dx * rho) * reduction,
            };
            let confidence = coverage * 0.4 + contrast * 0.6;
            candidates.push((
                Detection {
                    angle: degree,
                    confidence,
                    start,
                    end,
                },
                length * confidence,
            ));
        }
        if selected.len() >= 80 {
            break;
        }
    }
    candidates.sort_unstable_by(|a, b| b.1.total_cmp(&a.1));
    let (best, score) = *candidates.first()?;
    let radians = best.angle.to_radians();
    let competing = candidates.iter().skip(1).any(|(other, value)| {
        let angle = (best.angle - other.angle).abs();
        let offset = ((other.start.x - best.start.x) * radians.cos()
            - (other.start.y - best.start.y) * radians.sin())
        .abs()
            / reduction;
        (angle.min(180.0 - angle) > 18.0 || offset > 4.0) && *value > score * 0.85
    });
    if competing {
        return None;
    }
    // Fit the supported pixels rather than retaining a quantized Hough angle.
    // This also avoids choosing a diagonal across the thickness of a short rope.
    let (dx, dy) = radians.sin_cos();
    let length = (best.end.x - best.start.x).hypot(best.end.y - best.start.y) / reduction;
    let support: Vec<_> = points
        .iter()
        .copied()
        .filter(|(x, y)| {
            let (rx, ry) = (x - best.start.x / reduction, y - best.start.y / reduction);
            (rx * dy - ry * dx).abs() <= 2.0 && (-0.5..=length + 0.5).contains(&(rx * dx + ry * dy))
        })
        .collect();
    if support.len() < 8 {
        return None;
    }
    let count = support.len() as f64;
    let (cx, cy) = support
        .iter()
        .fold((0.0, 0.0), |(a, b), (x, y)| (a + x / count, b + y / count));
    let (xx, xy, yy) = support.iter().fold((0.0, 0.0, 0.0), |(a, b, c), (x, y)| {
        (
            a + (x - cx).powi(2),
            b + (x - cx) * (y - cy),
            c + (y - cy).powi(2),
        )
    });
    let theta = (2.0 * xy).atan2(xx - yy) / 2.0;
    let (mut dy, mut dx) = theta.sin_cos();
    if dy < 0.0 {
        dx = -dx;
        dy = -dy;
    }
    let (lo, hi) = support
        .iter()
        .map(|(x, y)| (x - cx) * dx + (y - cy) * dy)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| {
            (lo.min(t), hi.max(t))
        });
    Some(Detection {
        angle: dx.atan2(dy).to_degrees(),
        confidence: best.confidence,
        start: Point {
            x: (cx + lo * dx) * reduction,
            y: (cy + lo * dy) * reduction,
        },
        end: Point {
            x: (cx + hi * dx) * reduction,
            y: (cy + hi * dy) * reduction,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use xcap::image::Rgba;
    fn scene(angle: f64, scale: f64, origin: (f64, f64)) -> RgbaImage {
        let mut image = RgbaImage::from_pixel(
            (100.0 * scale) as u32,
            (100.0 * scale) as u32,
            Rgba([43, 64, 149, 255]),
        );
        let (dx, dy) = angle.to_radians().sin_cos();
        for (x, y, p) in image.enumerate_pixels_mut() {
            let (rx, ry) = (x as f64 / scale - origin.0, y as f64 / scale - origin.1);
            if (0.0..=22.0).contains(&(rx * dx + ry * dy)) && (rx * dy - ry * dx).abs() <= 1.1 {
                *p = Rgba([48, 54, 52, 255]);
            }
        }
        image
    }
    #[test]
    fn finds_translated_segments_at_multiple_angles_and_dpi() {
        for scale in [1.0, 1.5, 2.0] {
            for origin in [(30.0, 20.0), (65.0, 55.0)] {
                for angle in [-76.0, -44.0, 0.0, 38.0, 76.0] {
                    let d =
                        detect(&scene(angle, scale, origin), scale, &Options::default()).unwrap();
                    assert!((d.angle - angle).abs() <= 5.0, "{angle} {scale}: {d:?}");
                    assert!(
                        (d.start.x / scale - origin.0).hypot(d.start.y / scale - origin.1) < 4.0,
                        "{d:?}"
                    );
                }
            }
        }
    }
    #[test]
    fn rejects_missing_solid_and_competing_segments() {
        for color in [[43, 64, 149, 255], [30, 30, 30, 255], [230, 230, 230, 255]] {
            assert!(detect(
                &RgbaImage::from_pixel(100, 100, Rgba(color)),
                1.0,
                &Options::default()
            )
            .is_none());
        }
        let mut image = scene(-44.0, 1.0, (30.0, 20.0));
        for (a, b) in image
            .pixels_mut()
            .zip(scene(44.0, 1.0, (65.0, 55.0)).pixels())
        {
            if b.0 == [48, 54, 52, 255] {
                *a = *b;
            }
        }
        assert!(detect(&image, 1.0, &Options::default()).is_none());
    }
    #[test]
    fn reference_crop_and_mirror() {
        let image =
            xcap::image::load_from_memory(include_bytes!("../tests/fixtures/short-rope.png"))
                .unwrap()
                .to_rgba8();
        let crop = imageops::crop_imm(&image, 85, 105, 30, 19).to_image();
        let d = detect(&crop, 1.0, &Options::default()).unwrap();
        assert!((d.angle + 40.0).abs() < 8.0, "{d:?}");
        let mirror = detect(&imageops::flip_horizontal(&crop), 1.0, &Options::default()).unwrap();
        assert!((mirror.angle - 40.0).abs() < 8.0, "{mirror:?}");
    }
    #[test]
    fn migrates_radial_options_and_validates_rectangle() {
        let old: Options = serde_yaml::from_str(
            "skipRadius: 3\nsearchRadius: 18\ndarkness: 100\nshowReference: true",
        )
        .unwrap();
        assert_eq!(old.region, Region::default());
        assert_eq!(old.darkness, 100.0);
        assert!(old.show_reference);
        assert!(!serde_yaml::to_string(&old).unwrap().contains("Radius"));
        for region in [
            Region {
                x: 0.99,
                ..Region::default()
            },
            Region {
                width: 0.0,
                ..Region::default()
            },
            Region {
                y: f64::NAN,
                ..Region::default()
            },
        ] {
            assert!(Options {
                region,
                ..Options::default()
            }
            .validate()
            .is_err());
        }
        let patch: OptionsPatch = serde_yaml::from_str("darkness: 120").unwrap();
        let result = patch.apply(&old).unwrap();
        assert_eq!(result.region, old.region);
        assert!(result.show_reference);
    }
}
