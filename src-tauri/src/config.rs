use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Settings {
    pub origin_x: f64,
    pub origin_y: f64,
    pub count: u32,
    pub spread: f64,
    pub rotation: f64,
    pub length: f64,
    pub color: String,
    pub width: f64,
    pub opacity: f64,
    pub show_origin: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            origin_x: 0.5,
            origin_y: 0.14,
            count: 9,
            spread: 150.0,
            rotation: 0.0,
            length: 1.0,
            color: "#53e3ea".into(),
            width: 1.5,
            opacity: 0.65,
            show_origin: true,
        }
    }
}

fn range(value: f64, min: f64, max: f64) -> bool {
    value.is_finite() && (min..=max).contains(&value)
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=31).contains(&self.count)
            || !range(self.origin_x, 0.0, 1.0)
            || !range(self.origin_y, 0.0, 1.0)
            || !range(self.spread, 0.0, 180.0)
            || !range(self.rotation, -90.0, 90.0)
            || !range(self.length, 0.05, 2.0)
            || !range(self.width, 0.5, 8.0)
            || !range(self.opacity, 0.05, 1.0)
            || self.color.len() != 7
            || !self.color.starts_with('#')
            || !self.color[1..].chars().all(|c| c.is_ascii_hexdigit())
        {
            return Err("参数超出可用范围，请检查输入。".into());
        }
        Ok(())
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
    origin_x: Option<f64>,
    origin_y: Option<f64>,
    count: Option<u32>,
    spread: Option<f64>,
    rotation: Option<f64>,
    length: Option<f64>,
    color: Option<String>,
    width: Option<f64>,
    opacity: Option<f64>,
    show_origin: Option<bool>,
}

impl SettingsPatch {
    pub fn apply(self, current: &Settings) -> Result<Settings, String> {
        let mut next = current.clone();
        macro_rules! apply { ($($field:ident),*) => { $(if let Some(v) = self.$field { next.$field = v; })* }; }
        apply!(
            origin_x,
            origin_y,
            count,
            spread,
            rotation,
            length,
            color,
            width,
            opacity,
            show_origin
        );
        next.validate()?;
        Ok(next)
    }
}

// Positions are physical desktop pixels (including negative monitor origins).
// Sizes are logical pixels, matching the WebView's CSS coordinates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub settings: Settings,
    pub bounds: Option<Bounds>,
}

pub fn load(path: &Path) -> Result<Config, String> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let config: Config = serde_yaml::from_str(&text).map_err(|e| e.to_string())?;
    config.settings.validate()?;
    if config
        .bounds
        .as_ref()
        .is_some_and(|b| !range(b.width, 240.0, 20000.0) || !range(b.height, 180.0, 20000.0))
    {
        return Err("窗口尺寸无效".into());
    }
    Ok(config)
}

pub fn save(path: &Path, config: &Config) -> Result<(), String> {
    let parent = path.parent().ok_or("配置目录无效")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let yaml = serde_yaml::to_string(config).map_err(|e| e.to_string())?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temp.write_all(yaml.as_bytes()).map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

// Clamp to a monitor's work area. Keep fractional-scale dimensions in logical units.
pub fn fit_bounds(b: &Bounds, x: i32, y: i32, w: u32, h: u32, scale: f64) -> Bounds {
    let width = b.width.min(w as f64 / scale);
    let height = b.height.min(h as f64 / scale);
    let max_x = x.saturating_add((w as f64 - width * scale).max(0.0).floor() as i32);
    let max_y = y.saturating_add((h as f64 - height * scale).max(0.0).floor() as i32);
    Bounds {
        x: b.x.clamp(x, max_x),
        y: b.y.clamp(y, max_y),
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_replace() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.yaml");
        let mut c = Config::default();
        save(&path, &c).unwrap();
        c.settings.count = 13;
        c.bounds = Some(Bounds {
            x: -1000,
            y: 40,
            width: 960.0,
            height: 640.0,
        });
        save(&path, &c).unwrap();
        assert_eq!(load(&path).unwrap(), c);
    }

    #[test]
    fn reject_invalid_and_corrupt_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.yaml");
        fs::write(&path, "settings: [broken").unwrap();
        assert!(load(&path).is_err());
        fs::write(&path, "settings:\n  count: 0").unwrap();
        assert!(load(&path).is_err());
        let patch: SettingsPatch = serde_yaml::from_str("originX: .nan").unwrap();
        assert!(patch.apply(&Settings::default()).is_err());
    }

    #[test]
    fn patch_preserves_other_fields() {
        let patch: SettingsPatch = serde_yaml::from_str("count: 1\nrotation: 90").unwrap();
        let s = patch.apply(&Settings::default()).unwrap();
        assert_eq!(s.count, 1);
        assert_eq!(s.origin_y, 0.14);
        assert_eq!(s.rotation, 90.0);
    }

    #[test]
    fn restore_with_negative_origin_and_dpi() {
        let b = Bounds {
            x: -4000,
            y: 2000,
            width: 2000.0,
            height: 800.0,
        };
        let fitted = fit_bounds(&b, -1920, 0, 1920, 1080, 1.5);
        assert_eq!(
            fitted,
            Bounds {
                x: -1920,
                y: 0,
                width: 1280.0,
                height: 720.0
            }
        );
    }
}
