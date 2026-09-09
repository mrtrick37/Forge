//! Offline per-device OpenRGB/liquidctl preset configuration.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbPreset {
    pub effect: String,
    pub brightness: i64,
    pub color: String,
}
impl Default for RgbPreset {
    fn default() -> Self {
        Self {
            effect: "rainbow".into(),
            brightness: 80,
            color: "#ffffff".into(),
        }
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    PathBuf::from(
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
                    .join(".config")
            }),
    )
    .join("kyth/rgb.toml")
}

fn parse_entry(value: &toml::Value) -> RgbPreset {
    let table = value.as_table();
    let brightness = table
        .and_then(|t| t.get("brightness"))
        .and_then(toml::Value::as_integer)
        .unwrap_or(80)
        .clamp(0, 100);
    RgbPreset {
        effect: table
            .and_then(|t| t.get("effect"))
            .and_then(toml::Value::as_str)
            .unwrap_or("rainbow")
            .into(),
        brightness,
        color: table
            .and_then(|t| t.get("color"))
            .and_then(toml::Value::as_str)
            .unwrap_or("#ffffff")
            .into(),
    }
}

/// Best-effort device argv, exactly as the Python launcher ordered them.
/// Every invocation is fire-and-forget upstream (`except Exception: pass`).
pub fn openrgb_argv(device: &str, preset: &RgbPreset) -> Vec<String> {
    vec![
        "openrgb".to_string(),
        "--device".to_string(),
        device.to_string(),
        "--mode".to_string(),
        preset.effect.clone(),
        "--brightness".to_string(),
        preset.brightness.to_string(),
    ]
}

pub fn liquidctl_argv(device: &str, preset: &RgbPreset) -> Vec<String> {
    vec![
        "liquidctl".to_string(),
        "--match".to_string(),
        device.to_string(),
        "set".to_string(),
        "led".to_string(),
        "color".to_string(),
        preset.color.clone(),
    ]
}

pub fn load(path: impl AsRef<Path>) -> BTreeMap<String, RgbPreset> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return BTreeMap::new();
    };
    value
        .get("devices")
        .and_then(toml::Value::as_table)
        .map(|items| {
            items
                .iter()
                .map(|(key, value)| (key.clone(), parse_entry(value)))
                .collect()
        })
        .unwrap_or_default()
}

pub fn save(path: impl AsRef<Path>, devices: &BTreeMap<String, RgbPreset>) -> std::io::Result<()> {
    let quote = |value: &str| toml::Value::String(value.to_string()).to_string();
    let mut lines = vec!["# Kyth RGB per-device, offline".to_string()];
    for (name, preset) in devices {
        lines.push(format!("[devices.{}]", quote(name)));
        lines.push(format!("effect = {}", quote(&preset.effect)));
        lines.push(format!("brightness = {}", preset.brightness));
        lines.push(format!("color = {}", quote(&preset.color)));
        lines.push(String::new());
    }
    crate::atomic_io::atomic_write_text(path, &format!("{}\n", lines.join("\n")), Some(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn projects_device_argv() {
        let preset = RgbPreset {
            effect: "static".into(),
            brightness: 50,
            color: "#ff0000".into(),
        };
        assert_eq!(
            openrgb_argv("0", &preset),
            vec![
                "openrgb",
                "--device",
                "0",
                "--mode",
                "static",
                "--brightness",
                "50"
            ]
        );
        assert_eq!(
            liquidctl_argv("0", &preset),
            vec![
                "liquidctl",
                "--match",
                "0",
                "set",
                "led",
                "color",
                "#ff0000"
            ]
        );
    }

    #[test]
    fn clamps_brightness_and_defaults_missing_values() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rgb.toml");
        std::fs::write(&path, "[devices.keyboard]\nbrightness = 200\n").unwrap();
        assert_eq!(
            load(&path)["keyboard"],
            RgbPreset {
                brightness: 100,
                ..Default::default()
            }
        );
    }
}
