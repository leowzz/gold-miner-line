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
            x: 0.41,
            y: 0.12,
            width: 0.18,
            height: 0.18,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "StoredOptions")]
pub struct Options {
    pub region: Region,
    pub color_tolerance: f64,
    pub show_reference: bool,
}

// Accept legacy rope options and migrate only the detection crop.
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct StoredOptions {
    region: Option<Region>,
    color_tolerance: Option<f64>,
    #[serde(rename = "darkness")]
    _darkness: Option<f64>,
    show_reference: Option<bool>,
    #[serde(rename = "skipRadius")]
    _skip_radius: Option<f64>,
    #[serde(rename = "searchRadius")]
    _search_radius: Option<f64>,
}
impl From<StoredOptions> for Options {
    fn from(value: StoredOptions) -> Self {
        Self {
            region: match (value.region, value.color_tolerance) {
                (Some(region), None)
                    if [region.x, region.y, region.width, region.height]
                        .iter()
                        .all(|v| v.is_finite())
                        && region.x >= 0.0
                        && region.y >= 0.0
                        && (0.005..=1.0).contains(&region.width)
                        && (0.005..=1.0).contains(&region.height)
                        && region.x + region.width <= 1.0
                        && region.y + region.height <= 1.0 =>
                {
                    // The old crop only enclosed a tiny rope. Expand downward
                    // around it once, preserving all unrelated user settings.
                    let width = region.width.max(0.18);
                    let height = region.height.max(0.18);
                    Region {
                        x: (region.x + region.width / 2.0 - width / 2.0).clamp(0.0, 1.0 - width),
                        y: (region.y - (height - region.height) / 4.0).clamp(0.0, 1.0 - height),
                        width,
                        height,
                    }
                }
                (region, _) => region.unwrap_or_default(),
            },
            color_tolerance: value.color_tolerance.unwrap_or(24.0),
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
        if [r.x, r.y, r.width, r.height, self.color_tolerance]
            .iter()
            .any(|v| !v.is_finite())
            || r.x < 0.0
            || r.y < 0.0
            || !(0.005..=1.0).contains(&r.width)
            || !(0.005..=1.0).contains(&r.height)
            || r.x + r.width > 1.0 + 1e-9
            || r.y + r.height > 1.0 + 1e-9
            || !(10.0..=80.0).contains(&self.color_tolerance)
        {
            return Err(
                "识别框需完整位于游戏画面内，宽高至少为 0.5%；灰色容差需在 10–80 之间。".into(),
            );
        }
        Ok(())
    }
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OptionsPatch {
    region: Option<Region>,
    color_tolerance: Option<f64>,
    show_reference: Option<bool>,
}
impl OptionsPatch {
    pub fn apply(self, current: &Options) -> Result<Options, String> {
        let options = Options {
            region: self.region.unwrap_or(current.region),
            color_tolerance: self.color_tolerance.unwrap_or(current.color_tolerance),
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
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub angle: f64,
    pub confidence: f64,
    /// Opening midpoint and a point along its perpendicular, toward the mine.
    pub start: Point,
    pub end: Point,
    pub jaw_left: Point,
    pub jaw_right: Point,
}

fn metal(pixel: &[u8; 4], tolerance: f64) -> bool {
    let low = pixel[0].min(pixel[1]).min(pixel[2]);
    let high = pixel[0].max(pixel[1]).max(pixel[2]);
    let light = (pixel[0] as f64 + pixel[1] as f64 + pixel[2] as f64) / 3.0;
    pixel[3] >= 128 && (high - low) as f64 <= tolerance && (70.0..=225.0).contains(&light)
}

fn cross(a: Point, b: Point, c: Point) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}
fn hull(mut points: Vec<Point>) -> Vec<Point> {
    points.sort_unstable_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    let mut lower = Vec::new();
    let mut upper = Vec::new();
    for &p in &points {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    for &p in points.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

// Open claw silhouette, in units of jaw-tip separation. Its opening lies on
// y=0; the two tips are (-.5,0), (.5,0), and the rope attachment lies above.
// This geometric outline has no screenshot/template bitmap or model dependency.
const ARM: &[(f64, f64)] = &[
    (-0.02, -0.57),
    (-0.36, -0.40),
    (-0.50, 0.0),
    (-0.22, -0.12),
    (-0.35, -0.12),
    (-0.24, -0.31),
    (0.02, -0.44),
];
fn inside(x: f64, y: f64, polygon: &[(f64, f64)]) -> bool {
    let mut result = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let (xi, yi) = polygon[i];
        let (xj, yj) = polygon[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            result = !result;
        }
        j = i;
    }
    result
}

struct Mask {
    width: usize,
    height: usize,
    pixels: Vec<bool>,
}
impl Mask {
    fn get(&self, x: i32, y: i32) -> bool {
        x >= 0
            && y >= 0
            && x < self.width as i32
            && y < self.height as i32
            && self.pixels[y as usize * self.width + x as usize]
    }
    fn near(&self, p: Point) -> bool {
        let (x, y) = (p.x.round() as i32, p.y.round() as i32);
        self.get(x, y)
            || self.get(x - 1, y)
            || self.get(x + 1, y)
            || self.get(x, y - 1)
            || self.get(x, y + 1)
    }
    fn components(&self) -> Vec<Vec<Point>> {
        let mut visited = vec![false; self.pixels.len()];
        let mut result = Vec::new();
        for seed in 0..self.pixels.len() {
            if visited[seed] || !self.pixels[seed] {
                continue;
            }
            visited[seed] = true;
            let mut stack = vec![seed];
            let mut points = Vec::new();
            while let Some(i) = stack.pop() {
                let (x, y) = ((i % self.width) as i32, (i / self.width) as i32);
                points.push(Point {
                    x: x as f64,
                    y: y as f64,
                });
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if self.get(x + dx, y + dy) {
                            let next = (y + dy) as usize * self.width + (x + dx) as usize;
                            if !visited[next] {
                                visited[next] = true;
                                stack.push(next);
                            }
                        }
                    }
                }
            }
            if points.len() >= 30 {
                result.push(points);
            }
        }
        result
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    #[default]
    Invalid,
    NoMetal,
    NoMatch,
    Ambiguous,
    Detected,
}

#[derive(Default)]
pub struct Analysis {
    pub detection: Option<Detection>,
    pub status: Status,
    pub component_count: usize,
    pub candidate_count: usize,
    pub gray_pixels: usize,
    pub contours: Option<RgbaImage>,
}

fn contours(mask: &Mask, selected: Option<&[Point]>) -> RgbaImage {
    let mut matched = vec![false; mask.pixels.len()];
    if let Some(points) = selected {
        for p in points {
            matched[p.y as usize * mask.width + p.x as usize] = true;
        }
    }
    let mut image = RgbaImage::new(mask.width as u32, mask.height as u32);
    for y in 0..mask.height {
        for x in 0..mask.width {
            if mask.get(x as i32, y as i32)
                && [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .any(|(dx, dy)| !mask.get(x as i32 + dx, y as i32 + dy))
            {
                image.put_pixel(
                    x as u32,
                    y as u32,
                    xcap::image::Rgba(if matched[y * mask.width + x] {
                        [82, 240, 157, 255]
                    } else {
                        [255, 174, 76, 255]
                    }),
                );
            }
        }
    }
    image
}

/// Detect both jaws as a gray open-claw silhouette, then construct the exact
/// perpendicular bisector of their tip-to-tip segment. The rope is unused.
pub fn detect(image: &RgbaImage, scale: f64, options: &Options) -> Option<Detection> {
    analyze(image, scale, options, false).detection
}

pub fn analyze(image: &RgbaImage, scale: f64, options: &Options, diagnostics: bool) -> Analysis {
    if options.validate().is_err()
        || !scale.is_finite()
        || scale <= 0.0
        || image.width() < 16
        || image.height() < 16
    {
        return Analysis::default();
    }
    let reduction = scale
        .max(image.width().max(image.height()) as f64 / 256.0)
        .max(1.0);
    let small = imageops::resize(
        image,
        (image.width() as f64 / reduction).round().max(1.0) as u32,
        (image.height() as f64 / reduction).round().max(1.0) as u32,
        imageops::FilterType::Triangle,
    );
    let mask = Mask {
        width: small.width() as usize,
        height: small.height() as usize,
        pixels: small
            .pixels()
            .map(|p| metal(&p.0, options.color_tolerance))
            .collect(),
    };
    let mut positive = Vec::new();
    for iy in -16..=0 {
        for ix in -14..=0 {
            let (x, y) = (ix as f64 / 28.0, iy as f64 / 28.0);
            if inside(x, y, ARM) {
                positive.push((x, y));
            }
        }
    }
    let negative: Vec<_> = (-4..=4)
        .flat_map(|x| (-8..=2).map(move |y| (x as f64 / 24.0, y as f64 / 24.0)))
        .collect();
    let mut candidates = Vec::new();
    let components = mask.components();
    for (component_id, points) in components.iter().enumerate() {
        // An almost solid crop is background, not a pair of jaws.
        if points.len() > mask.pixels.len() / 2 {
            continue;
        }
        let outline = hull(points.clone());
        for i in 0..outline.len() {
            for j in i + 1..outline.len() {
                let (mut left, mut right) = (outline[i], outline[j]);
                if left.x > right.x {
                    std::mem::swap(&mut left, &mut right);
                }
                let span = (right.x - left.x).hypot(right.y - left.y);
                if !(16.0..=180.0).contains(&span) {
                    continue;
                }
                let (tx, ty) = ((right.x - left.x) / span, (right.y - left.y) / span);
                let center = Point {
                    x: (left.x + right.x) / 2.0,
                    y: (left.y + right.y) / 2.0,
                };
                let transform = |x: f64, y: f64| Point {
                    x: center.x + span * (tx * x - ty * y),
                    y: center.y + span * (ty * x + tx * y),
                };
                // The entire claw must be inside the crop; an unknown jaw is not an empty gap.
                if [(-0.55, 0.06), (0.55, 0.06), (-0.40, -0.61), (0.40, -0.61)]
                    .iter()
                    .any(|&(x, y)| {
                        let p = transform(x, y);
                        p.x < 1.0
                            || p.y < 1.0
                            || p.x >= small.width() as f64 - 1.0
                            || p.y >= small.height() as f64 - 1.0
                    })
                {
                    continue;
                }
                let mut coverage = [0.0; 2];
                for (side, sign) in [-1.0, 1.0].iter().enumerate() {
                    coverage[side] = positive
                        .iter()
                        .filter(|&&(x, y)| mask.near(transform(x * sign, y)))
                        .count() as f64
                        / positive.len() as f64;
                }
                if coverage[0].min(coverage[1]) < 0.66 {
                    continue;
                }
                let empty = negative
                    .iter()
                    .filter(|&&(x, y)| {
                        !mask.get(
                            transform(x, y).x.round() as i32,
                            transform(x, y).y.round() as i32,
                        )
                    })
                    .count() as f64
                    / negative.len() as f64;
                if empty < 0.82 {
                    continue;
                }
                // Both jaws must explain most of the connected silhouette. Without
                // this, a support touching half a claw can resemble a smaller claw.
                let explained = points
                    .iter()
                    .filter(|p| {
                        let (rx, ry) = ((p.x - center.x) / span, (p.y - center.y) / span);
                        let (x, y) = (rx * tx + ry * ty, -rx * ty + ry * tx);
                        [
                            (0.0, 0.0),
                            (-0.045, 0.0),
                            (0.045, 0.0),
                            (0.0, -0.045),
                            (0.0, 0.045),
                        ]
                        .iter()
                        .any(|&(ox, oy)| {
                            inside(x + ox, y + oy, ARM) || inside(-x + ox, y + oy, ARM)
                        })
                    })
                    .count() as f64
                    / points.len() as f64;
                if explained < 0.62 {
                    continue;
                }
                let confidence = coverage[0].min(coverage[1]) * 0.5 + empty * 0.3 + explained * 0.2;
                candidates.push((confidence, center, span, left, right, component_id));
            }
        }
    }
    candidates.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
    let gray_pixels = mask.pixels.iter().filter(|p| **p).count();
    let mut report = Analysis {
        status: if components.is_empty() {
            Status::NoMetal
        } else {
            Status::NoMatch
        },
        component_count: components.len(),
        candidate_count: candidates.len(),
        gray_pixels,
        contours: diagnostics.then(|| contours(&mask, None)),
        detection: None,
    };
    let Some(&(confidence, center, span, left, right, component_id)) = candidates.first() else {
        return report;
    };
    let angle = -(right.y - left.y).atan2(right.x - left.x).to_degrees();
    if candidates.iter().skip(1).any(|(score, c, _, l, r, _)| {
        let other = -(r.y - l.y).atan2(r.x - l.x).to_degrees();
        *score > confidence - 0.06
            && ((c.x - center.x).hypot(c.y - center.y) > span * 0.35
                || (other - angle).abs() > 22.0)
    }) {
        report.status = Status::Ambiguous;
        return report;
    }
    report.status = Status::Detected;
    if diagnostics {
        report.contours = Some(contours(&mask, Some(&components[component_id])));
    }
    let sx = image.width() as f64 / small.width() as f64;
    let sy = image.height() as f64 / small.height() as f64;
    let left = Point {
        x: left.x * sx,
        y: left.y * sy,
    };
    let right = Point {
        x: right.x * sx,
        y: right.y * sy,
    };
    let start = Point {
        x: (left.x + right.x) / 2.0,
        y: (left.y + right.y) / 2.0,
    };
    let (dx, dy) = (-(right.y - left.y), right.x - left.x);
    let norm = dx.hypot(dy);
    report.detection = Some(Detection {
        angle: dx.atan2(dy).to_degrees(),
        confidence,
        start,
        end: Point {
            x: start.x + dx / norm,
            y: start.y + dy / norm,
        },
        jaw_left: left,
        jaw_right: right,
    });
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use xcap::image::Rgba;

    fn reference() -> RgbaImage {
        let image =
            xcap::image::load_from_memory(include_bytes!("../tests/fixtures/short-rope.png"))
                .unwrap()
                .to_rgba8();
        imageops::crop_imm(&image, 40, 95, 80, 80).to_image()
    }
    fn assert_bisector(d: Detection) {
        assert!((d.start.x - (d.jaw_left.x + d.jaw_right.x) / 2.0).abs() < 1e-6);
        assert!((d.start.y - (d.jaw_left.y + d.jaw_right.y) / 2.0).abs() < 1e-6);
        let dot = (d.end.x - d.start.x) * (d.jaw_right.x - d.jaw_left.x)
            + (d.end.y - d.start.y) * (d.jaw_right.y - d.jaw_left.y);
        assert!(dot.abs() < 1e-6, "not perpendicular: {d:?}");
        assert!(d.end.y >= d.start.y);
    }
    #[test]
    fn reference_jaws_and_mirror_give_a_perpendicular_bisector() {
        let image = reference();
        let d = detect(&image, 1.0, &Options::default()).unwrap();
        assert!((d.angle + 43.0).abs() < 4.0, "{d:?}");
        assert!((d.start.x - 32.0).hypot(d.start.y - 48.0) < 3.0, "{d:?}");
        assert_bisector(d);
        let d = detect(&imageops::flip_horizontal(&image), 1.0, &Options::default()).unwrap();
        assert!((d.angle - 43.0).abs() < 4.0, "{d:?}");
        assert_bisector(d);
    }
    #[test]
    fn does_not_need_the_short_rope() {
        let mut image = reference();
        for p in image.pixels_mut() {
            if p.0[..3].iter().all(|&c| c < 70) {
                *p = Rgba([43, 64, 149, 255]);
            }
        }
        let d = detect(&image, 1.0, &Options::default()).unwrap();
        assert!((d.angle + 43.0).abs() < 4.0, "{d:?}");
    }
    #[test]
    fn diagnostics_explain_failures_without_labeling_candidates_as_detected() {
        let blank = RgbaImage::from_pixel(80, 80, Rgba([43, 64, 149, 255]));
        let report = analyze(&blank, 1.0, &Options::default(), true);
        assert_eq!(report.status, Status::NoMetal);
        assert_eq!(report.gray_pixels, 0);
        assert!(report.contours.unwrap().pixels().all(|p| p.0[3] == 0));
        let mut two = RgbaImage::from_pixel(180, 100, Rgba([43, 64, 149, 255]));
        imageops::overlay(&mut two, &reference(), 5, 10);
        imageops::overlay(&mut two, &reference(), 95, 10);
        let report = analyze(&two, 1.0, &Options::default(), true);
        assert_eq!(report.status, Status::Ambiguous);
        assert!(report.detection.is_none());
        assert!(report.candidate_count >= 2);
        let outline = report.contours.unwrap();
        assert!(outline.pixels().any(|p| p.0 == [255, 174, 76, 255]));
        assert!(!outline.pixels().any(|p| p.0 == [82, 240, 157, 255]));
        let fast = analyze(&reference(), 1.0, &Options::default(), false);
        assert!(fast.contours.is_none());
        assert!(fast.detection.is_some());
    }
    fn transformed(angle: f64, scale: f64, center: (f64, f64)) -> RgbaImage {
        let source = reference();
        let mut image = RgbaImage::from_pixel(
            (190.0 * scale) as u32,
            (190.0 * scale) as u32,
            Rgba([43, 64, 149, 255]),
        );
        let phi = (-43.6 - angle).to_radians();
        let (sin, cos) = phi.sin_cos();
        for (x, y, p) in image.enumerate_pixels_mut() {
            let (rx, ry) = (x as f64 / scale - center.0, y as f64 / scale - center.1);
            let (sx, sy) = (
                (rx * cos + ry * sin + 40.0).round() as i32,
                (-rx * sin + ry * cos + 40.0).round() as i32,
            );
            if (0..80).contains(&sx) && (0..80).contains(&sy) {
                *p = *source.get_pixel(sx as u32, sy as u32);
            }
        }
        image
    }
    #[test]
    fn follows_rotated_claws_at_multiple_positions_and_dpi() {
        for scale in [1.0, 1.5, 2.0] {
            for center in [(70.0, 80.0), (110.0, 100.0)] {
                for angle in [-75.0, -40.0, 0.0, 40.0, 75.0] {
                    let d = detect(
                        &transformed(angle, scale, center),
                        scale,
                        &Options::default(),
                    )
                    .unwrap_or_else(|| {
                        panic!("missing angle={angle}, scale={scale}, center={center:?}")
                    });
                    assert!(
                        (d.angle - angle).abs() < 6.0,
                        "angle={angle}, scale={scale}: {d:?}"
                    );
                    assert_bisector(d);
                }
            }
        }
    }
    #[test]
    fn rejects_background_bars_one_jaw_and_multiple_claws() {
        for color in [[43, 64, 149, 255], [150, 150, 150, 255], [30, 30, 30, 255]] {
            assert!(detect(
                &RgbaImage::from_pixel(100, 100, Rgba(color)),
                1.0,
                &Options::default()
            )
            .is_none());
        }
        let mut bar = RgbaImage::from_pixel(100, 100, Rgba([43, 64, 149, 255]));
        for y in 20..80 {
            for x in 40..46 {
                bar.put_pixel(x, y, Rgba([150, 150, 150, 255]));
            }
        }
        assert!(detect(&bar, 1.0, &Options::default()).is_none());
        let mut partial = reference();
        for (x, y, p) in partial.enumerate_pixels_mut() {
            if x > 44 && y > 38 {
                *p = Rgba([43, 64, 149, 255]);
            }
        }
        assert!(
            detect(&partial, 1.0, &Options::default()).is_none(),
            "{:?}",
            detect(&partial, 1.0, &Options::default())
        );
        let mut two = RgbaImage::from_pixel(180, 100, Rgba([43, 64, 149, 255]));
        imageops::overlay(&mut two, &reference(), 5, 10);
        imageops::overlay(&mut two, &reference(), 95, 10);
        assert!(detect(&two, 1.0, &Options::default()).is_none());
    }
    #[test]
    fn migrates_small_rope_crop_once_and_preserves_unrelated_options() {
        let old:Options=serde_yaml::from_str("region: {x: 0.465, y: 0.14, width: 0.07, height: 0.06}\ndarkness: 100\nshowReference: true").unwrap();
        assert_eq!(old.region.width, 0.18);
        assert_eq!(old.region.height, 0.18);
        assert!(old.region.x < 0.465 && old.region.y < 0.14);
        assert_eq!(old.color_tolerance, 24.0);
        assert!(old.show_reference);
        let saved = serde_yaml::to_string(&old).unwrap();
        assert!(!saved.contains("darkness"));
        let next: Options = serde_yaml::from_str(&saved).unwrap();
        assert_eq!(next, old);
        // Invalid old crops must return validation errors, never panic in migration.
        for yaml in [
            "region: {x: 0.1, y: 0.1, width: 2, height: 0.1}",
            "region: {x: .nan, y: 0.1, width: 0.1, height: 0.1}",
        ] {
            let invalid: Options = serde_yaml::from_str(yaml).unwrap();
            assert!(invalid.validate().is_err());
        }
        let patch: OptionsPatch = serde_yaml::from_str("colorTolerance: 50").unwrap();
        let edited = patch.apply(&old).unwrap();
        assert_eq!(edited.region, old.region);
        assert!(edited.show_reference);
        assert_eq!(edited.color_tolerance, 50.0);
    }
}
