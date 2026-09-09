//! Offline per-device libinput preset configuration.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct InputPreset {
    pub accel_profile: String,
    pub accel_speed: f64,
    pub tap_to_click: bool,
    pub scroll_method: String,
}

impl Default for InputPreset {
    fn default() -> Self {
        Self {
            accel_profile: "adaptive".into(),
            accel_speed: 0.0,
            tap_to_click: false,
            scroll_method: "twofinger".into(),
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
    .join("kyth/input.toml")
}

fn parse_entry(value: &toml::Value) -> InputPreset {
    let table = value.as_table();
    let profile = table
        .and_then(|t| t.get("accel_profile"))
        .and_then(toml::Value::as_str)
        .unwrap_or("adaptive");
    let accel_profile = matches!(profile, "adaptive" | "flat")
        .then(|| profile.to_string())
        .unwrap_or_else(|| "adaptive".into());
    let accel_speed = table
        .and_then(|t| t.get("accel_speed"))
        .and_then(toml::Value::as_float)
        .unwrap_or_else(|| {
            table
                .and_then(|t| t.get("accel_speed"))
                .and_then(toml::Value::as_integer)
                .map(|v| v as f64)
                .unwrap_or(0.0)
        })
        .clamp(-1.0, 1.0);
    InputPreset {
        accel_profile,
        accel_speed,
        tap_to_click: table
            .and_then(|t| t.get("tap_to_click"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        scroll_method: table
            .and_then(|t| t.get("scroll_method"))
            .and_then(toml::Value::as_str)
            .unwrap_or("twofinger")
            .to_string(),
    }
}

pub fn load(path: impl AsRef<Path>) -> BTreeMap<String, InputPreset> {
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

pub const XORG_CONF_DEST: &str = "/etc/X11/xorg.conf.d/50-kyth-input.conf";
pub const TTL_PATH: &str = "/run/kyth-input-ttl";
pub const TTL_SECS: u64 = 30;

/// Mirrors Python `str(float)`: integral values keep one decimal (`0.0`,
/// not Rust's `0`); the line is informational (xorg ignores comments).
pub fn py_float(value: f64) -> String {
    if value.fract() == 0.0 && value.is_finite() {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

/// Renders the `50-kyth-input.conf` body exactly as the Python launcher did,
/// including the leading spaces and trailing newline.
pub fn render_xorg_conf(devices: &BTreeMap<String, InputPreset>) -> String {
    let mut lines = vec![
        "Section \"InputClass\"".to_string(),
        " Identifier \"kyth-input\"".to_string(),
        " MatchIsPointer \"on\"".to_string(),
    ];
    for (name, preset) in devices {
        lines.push(format!(
            " # {name} accel {} speed {}",
            preset.accel_profile,
            py_float(preset.accel_speed)
        ));
    }
    lines.push("EndSection".to_string());
    format!("{}\n", lines.join("\n"))
}

pub fn save(
    path: impl AsRef<Path>,
    devices: &BTreeMap<String, InputPreset>,
) -> std::io::Result<()> {
    let quote = |value: &str| toml::Value::String(value.to_string()).to_string();
    let mut lines = vec!["# Kyth input per-device libinput".to_string()];
    for (name, preset) in devices {
        lines.push(format!("[devices.{}]", quote(name)));
        lines.push(format!("accel_profile = {}", quote(&preset.accel_profile)));
        lines.push(format!("accel_speed = {}", preset.accel_speed));
        lines.push(format!("tap_to_click = {}", preset.tap_to_click));
        lines.push(format!("scroll_method = {}", quote(&preset.scroll_method)));
        lines.push(String::new());
    }
    crate::atomic_io::atomic_write_text(path, &format!("{}\n", lines.join("\n")), Some(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_defaults_and_clamps_values() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("input.toml");
        std::fs::write(
            &path,
            "[devices.mouse]\naccel_profile = \"bad\"\naccel_speed = 4\ntap_to_click = true\n",
        )
        .unwrap();
        assert_eq!(
            load(&path)["mouse"],
            InputPreset {
                accel_profile: "adaptive".into(),
                accel_speed: 1.0,
                tap_to_click: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn renders_xorg_conf_like_python_launcher() {
        let mut devices = BTreeMap::new();
        devices.insert(
            "mouse".into(),
            InputPreset {
                accel_profile: "flat".into(),
                accel_speed: 0.5,
                ..Default::default()
            },
        );
        devices.insert("pad".into(), InputPreset::default());
        assert_eq!(
            render_xorg_conf(&devices),
            "Section \"InputClass\"\n Identifier \"kyth-input\"\n MatchIsPointer \"on\"\n # mouse accel flat speed 0.5\n # pad accel adaptive speed 0.0\nEndSection\n"
        );
        assert_eq!(render_xorg_conf(&BTreeMap::new()), "Section \"InputClass\"\n Identifier \"kyth-input\"\n MatchIsPointer \"on\"\nEndSection\n");
        assert_eq!(py_float(-1.0), "-1.0");
    }

    #[test]
    fn round_trips_device_names() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("input.toml");
        let mut devices = BTreeMap::new();
        devices.insert("USB \"mouse\"".into(), InputPreset::default());
        save(&path, &devices).unwrap();
        assert_eq!(load(&path), devices);
    }
}
