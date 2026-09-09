//! Offline per-game Steam Input presets.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct SteamInputPreset {
    pub layout: String,
    pub gyro: bool,
    pub deadzone: f64,
}
impl Default for SteamInputPreset {
    fn default() -> Self {
        Self {
            layout: "gamepad".into(),
            gyro: false,
            deadzone: 0.2,
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
    .join("kyth/steam-input.toml")
}

fn parse_entry(value: &toml::Value) -> SteamInputPreset {
    let table = value.as_table();
    let deadzone = table
        .and_then(|t| t.get("deadzone"))
        .and_then(toml::Value::as_float)
        .unwrap_or_else(|| {
            table
                .and_then(|t| t.get("deadzone"))
                .and_then(toml::Value::as_integer)
                .map(|v| v as f64)
                .unwrap_or(0.2)
        })
        .clamp(0.0, 1.0);
    SteamInputPreset {
        layout: table
            .and_then(|t| t.get("layout"))
            .and_then(toml::Value::as_str)
            .unwrap_or("gamepad")
            .into(),
        gyro: table
            .and_then(|t| t.get("gyro"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        deadzone,
    }
}

pub fn load(path: impl AsRef<Path>) -> BTreeMap<String, SteamInputPreset> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return BTreeMap::new();
    };
    value
        .get("games")
        .and_then(toml::Value::as_table)
        .map(|items| {
            items
                .iter()
                .map(|(key, value)| (key.clone(), parse_entry(value)))
                .collect()
        })
        .unwrap_or_default()
}

pub fn save(
    path: impl AsRef<Path>,
    games: &BTreeMap<String, SteamInputPreset>,
) -> std::io::Result<()> {
    let quote = |value: &str| toml::Value::String(value.to_string()).to_string();
    let mut lines = vec!["# Kyth Steam Input per-game".to_string()];
    for (name, preset) in games {
        lines.push(format!("[games.{}]", quote(name)));
        lines.push(format!("layout = {}", quote(&preset.layout)));
        lines.push(format!("gyro = {}", preset.gyro));
        lines.push(format!("deadzone = {}", preset.deadzone));
        lines.push(String::new());
    }
    crate::atomic_io::atomic_write_text(path, &format!("{}\n", lines.join("\n")), Some(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_and_clamps_deadzone() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("steam-input.toml");
        std::fs::write(&path, "[games.game]\ndeadzone = -1\ngyro = true\n").unwrap();
        assert_eq!(
            load(&path)["game"],
            SteamInputPreset {
                gyro: true,
                deadzone: 0.0,
                ..Default::default()
            }
        );
    }
}
