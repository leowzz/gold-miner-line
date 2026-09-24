use crate::vision::{Analysis, Detection};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use std::io::Cursor;
use xcap::image::{DynamicImage, ImageFormat, RgbaImage};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    /// Geometry uses original capture pixels. Both PNG layers cover these bounds.
    pub width: u32,
    pub height: u32,
    pub image: String,
    pub contours: String,
    pub detection: Option<Detection>,
    pub component_count: usize,
    pub candidate_count: usize,
    pub gray_pixels: usize,
    pub processing_ms: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub revision: u64,
    pub captured_at: u64,
    pub status: String,
    pub message: String,
    pub frame: Option<Frame>,
}

fn png(image: &RgbaImage) -> Result<String, String> {
    // Bound IPC and decode cost even when the user selects a whole monitor.
    let original = DynamicImage::ImageRgba8(image.clone());
    let small = if image.width() > 384 || image.height() > 384 {
        original.thumbnail(384, 384)
    } else {
        original
    };
    let mut buffer = Cursor::new(Vec::new());
    small
        .write_to(&mut buffer, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(buffer.into_inner())
    ))
}

pub fn frame(image: &RgbaImage, analysis: &Analysis, processing_ms: u64) -> Result<Frame, String> {
    let empty = RgbaImage::new(1, 1);
    let contours = analysis.contours.as_ref().unwrap_or(&empty);
    Ok(Frame {
        width: image.width(),
        height: image.height(),
        image: png(image)?,
        contours: png(contours)?,
        detection: analysis.detection,
        component_count: analysis.component_count,
        candidate_count: analysis.candidate_count,
        gray_pixels: analysis.gray_pixels,
        processing_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vision;
    fn decode(uri: &str) -> RgbaImage {
        let bytes = STANDARD
            .decode(uri.strip_prefix("data:image/png;base64,").unwrap())
            .unwrap();
        xcap::image::load_from_memory(&bytes).unwrap().to_rgba8()
    }
    #[test]
    fn image_contours_and_landmarks_share_one_capture_coordinate_system() {
        let scene =
            xcap::image::load_from_memory(include_bytes!("../tests/fixtures/short-rope.png"))
                .unwrap()
                .to_rgba8();
        let crop = xcap::image::imageops::crop_imm(&scene, 40, 95, 80, 80).to_image();
        let analysis = vision::analyze(&crop, 1.0, &vision::Options::default(), true);
        let rendered = frame(&crop, &analysis, 12).unwrap();
        let decoded = decode(&rendered.image);
        assert_eq!(decoded.dimensions(), crop.dimensions());
        assert!(decoded.as_raw() == crop.as_raw());
        let mask = decode(&rendered.contours);
        assert_eq!(mask.dimensions(), (80, 80));
        assert!(mask.pixels().any(|p| p.0 == [82, 240, 157, 255]));
        assert!(mask.pixels().any(|p| p.0[3] == 0));
        assert!((rendered.detection.unwrap().start.x - 32.0).abs() < 3.0);
        let large = xcap::image::imageops::resize(
            &crop,
            800,
            800,
            xcap::image::imageops::FilterType::Nearest,
        );
        let analysis = vision::analyze(&large, 10.0, &vision::Options::default(), true);
        let rendered = frame(&large, &analysis, 12).unwrap();
        assert_eq!((rendered.width, rendered.height), (800, 800));
        assert_eq!(decode(&rendered.image).dimensions(), (384, 384));
        assert!((rendered.detection.unwrap().start.x / 10.0 - 32.0).abs() < 3.0);
    }
}
