// Offline verification on a screenshot; never captures the screen or sends input.
use goldline::vision;
use xcap::image::{imageops, Rgba};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 6 && args.len() != 7 {
        eprintln!("Usage: detect_claw IMAGE X Y WIDTH HEIGHT [OUTPUT.png]");
        std::process::exit(2);
    }
    let mut image = xcap::image::open(&args[1]).expect("read image").to_rgba8();
    let number = |i: usize| args[i].parse::<u32>().expect("pixel coordinate");
    let (x, y, w, h) = (number(2), number(3), number(4), number(5));
    assert!(
        x + w <= image.width() && y + h <= image.height(),
        "rectangle outside image"
    );
    let crop = imageops::crop_imm(&image, x, y, w, h).to_image();
    match vision::detect(&crop, 1.0, &vision::Options::default()) {
        Some(d) => {
            println!(
                "angle={:.2}°, confidence={:.3}, start=({:.1},{:.1})",
                d.angle,
                d.confidence,
                d.start.x + x as f64,
                d.start.y + y as f64
            );
            if let Some(path) = args.get(6) {
                for i in 0..w {
                    for j in [0, h - 1] {
                        image.put_pixel(x + i, y + j, Rgba([83, 227, 234, 255]));
                    }
                }
                for j in 0..h {
                    for i in [0, w - 1] {
                        image.put_pixel(x + i, y + j, Rgba([83, 227, 234, 255]));
                    }
                }
                for step in 0..=100 {
                    let t = step as f64 / 100.0;
                    let px =
                        (x as f64 + d.jaw_left.x * (1.0 - t) + d.jaw_right.x * t).round() as u32;
                    let py =
                        (y as f64 + d.jaw_left.y * (1.0 - t) + d.jaw_right.y * t).round() as u32;
                    image.put_pixel(px, py, Rgba([255, 110, 110, 255]));
                }
                let a = d.angle.to_radians();
                for step in 0..(image.width() + image.height()) {
                    let px = (x as f64 + d.start.x + a.sin() * step as f64).round() as i32;
                    let py = (y as f64 + d.start.y + a.cos() * step as f64).round() as i32;
                    if px >= 0 && py >= 0 && px < image.width() as i32 && py < image.height() as i32
                    {
                        image.put_pixel(px as u32, py as u32, Rgba([255, 241, 160, 255]));
                    }
                }
                image.save(path).expect("save annotation");
            }
        }
        None => {
            eprintln!("No unambiguous claw detected");
            std::process::exit(1);
        }
    }
}
