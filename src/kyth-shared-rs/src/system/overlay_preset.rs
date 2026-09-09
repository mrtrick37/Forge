//! Offline per-game MangoHud/vkBasalt presets.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayPreset {
    pub mangohud_layout: String,
    pub vkbasalt: String,
}
impl Default for OverlayPreset {
    fn default() -> Self {
        Self {
            mangohud_layout: "fps+frametime".into(),
            vkbasalt: "off".into(),
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
    .join("kyth/overlay.toml")
}

fn parse_entry(value: &toml::Value) -> OverlayPreset {
    let table = value.as_table();
    let raw = table
        .and_then(|t| t.get("vkbasalt"))
        .and_then(toml::Value::as_str)
        .unwrap_or("off");
    OverlayPreset {
        mangohud_layout: table
            .and_then(|t| t.get("mangohud_layout"))
            .and_then(toml::Value::as_str)
            .unwrap_or("fps+frametime")
            .into(),
        vkbasalt: matches!(raw, "cas" | "off" | "sharp")
            .then(|| raw.to_string())
            .unwrap_or_else(|| "off".into()),
    }
}

pub fn load(path: impl AsRef<Path>) -> BTreeMap<String, OverlayPreset> {
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
    games: &BTreeMap<String, OverlayPreset>,
) -> std::io::Result<()> {
    let quote = |value: &str| toml::Value::String(value.to_string()).to_string();
    let mut lines = vec!["# Kyth per-game overlay MangoHud+vkBasalt".to_string()];
    for (name, preset) in games {
        lines.push(format!("[games.{}]", quote(name)));
        lines.push(format!(
            "mangohud_layout = {}",
            quote(&preset.mangohud_layout)
        ));
        lines.push(format!("vkbasalt = {}", quote(&preset.vkbasalt)));
        lines.push(String::new());
    }
    crate::atomic_io::atomic_write_text(path, &format!("{}\n", lines.join("\n")), Some(0o600))
}

pub fn env_for_app(app: &str, path: impl AsRef<Path>) -> BTreeMap<String, String> {
    let Some(config) = load(path).remove(app) else {
        return BTreeMap::new();
    };
    let mut env = BTreeMap::new();
    if config.mangohud_layout != "off" && !config.mangohud_layout.is_empty() {
        env.insert("MANGOHUD_CONFIG".into(), config.mangohud_layout);
    }
    if config.vkbasalt == "cas" {
        env.insert("ENABLE_VKBASALT".into(), "1".into());
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn projects_only_enabled_overlay_environment() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("overlay.toml");
        std::fs::write(
            &path,
            "[games.game]\nmangohud_layout = \"off\"\nvkbasalt = \"cas\"\n",
        )
        .unwrap();
        assert_eq!(
            env_for_app("game", &path),
            BTreeMap::from([("ENABLE_VKBASALT".into(), "1".into())])
        );
        assert!(env_for_app("missing", &path).is_empty());
    }
}
