//! Offline QuickSettings preference model.

use std::path::{Path, PathBuf};

pub const ALLOWED_TILES: &[&str] = &["wifi", "bt", "night", "plane", "battery"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickSettingsConfig {
    pub brightness: i64,
    pub tiles: Vec<String>,
}

impl Default for QuickSettingsConfig {
    fn default() -> Self { Self { brightness: 80, tiles: vec!["wifi".into(), "bt".into(), "night".into(), "plane".into()] } }
}

fn normalize_tiles(value: Option<&toml::Value>) -> Vec<String> {
    let tiles = value.and_then(toml::Value::as_array).map(|items| items.iter().filter_map(toml::Value::as_str).filter(|tile| ALLOWED_TILES.contains(tile)).map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
    if tiles.is_empty() { vec!["wifi".into(), "bt".into(), "night".into()] } else { tiles }
}

pub const TTL_PATH: &str = "/run/kyth-qs-ttl";
pub const TTL_SECS: u64 = 30;

/// PowerDevil brightness argv, exactly as the Python launcher ordered it.
/// The `brightness` note is recorded on spawn success regardless of exit
/// status (`run` defaults to `check=False` upstream).
pub fn brightness_argv(brightness: i64) -> Vec<String> {
    vec![
        "qdbus".to_string(),
        "org.kde.Solid.PowerManagement".to_string(),
        "/org/kde/Solid/PowerManagement/Actions/BrightnessControl".to_string(),
        "setBrightness".to_string(),
        brightness.to_string(),
    ]
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path { return path.as_ref().to_path_buf(); }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") { return PathBuf::from(config).join("kyth/quicksettings.toml"); }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into())).join(".config/kyth/quicksettings.toml")
}

pub fn load(path: impl AsRef<Path>) -> QuickSettingsConfig {
    let Ok(raw) = std::fs::read_to_string(path) else { return QuickSettingsConfig::default(); };
    let Ok(value) = raw.parse::<toml::Value>() else { return QuickSettingsConfig::default(); };
    QuickSettingsConfig {
        brightness: value.get("brightness").and_then(toml::Value::as_integer).unwrap_or(80).clamp(10, 100),
        tiles: normalize_tiles(value.get("tiles")),
    }
}

pub fn save(path: impl AsRef<Path>, config: &QuickSettingsConfig) -> std::io::Result<()> {
    let brightness = config.brightness.clamp(10, 100);
    let tiles = if config.tiles.is_empty() { vec!["wifi", "bt", "night"] } else { config.tiles.iter().map(String::as_str).filter(|tile| ALLOWED_TILES.contains(tile)).collect::<Vec<_>>() };
    let tiles = if tiles.is_empty() { vec!["wifi", "bt", "night"] } else { tiles };
    let rendered = tiles.iter().map(|tile| format!("{tile:?}")).collect::<Vec<_>>().join(", ");
    let text = format!("# Kyth QuickSettings deep\nbrightness = {brightness}\ntiles = [{rendered}]\n");
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clamps_brightness_and_filters_tiles() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("quicksettings.toml");
        std::fs::write(&path, "brightness = 999\ntiles = [\"wifi\", \"secret\"]\n").unwrap();
        assert_eq!(load(&path), QuickSettingsConfig { brightness: 100, tiles: vec!["wifi".into()] });
    }

    #[test]
    fn projects_brightness_argv() {
        assert_eq!(
            brightness_argv(80),
            vec!["qdbus", "org.kde.Solid.PowerManagement", "/org/kde/Solid/PowerManagement/Actions/BrightnessControl", "setBrightness", "80"]
        );
    }

    #[test]
    fn saves_valid_toml_array() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("quicksettings.toml");
        save(&path, &QuickSettingsConfig { brightness: 40, tiles: vec!["wifi".into(), "bt".into()] }).unwrap();
        assert_eq!(load(&path).brightness, 40);
        assert_eq!(load(&path).tiles, vec!["wifi", "bt"]);
    }
}
