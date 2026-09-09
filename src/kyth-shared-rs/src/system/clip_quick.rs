//! Clipboard history and quick-tile preferences.
//!
//! This ports the declarative portion of `kyth_shared.clip_quick`. Applying
//! the generated Klipper commands remains an explicit desktop action.

use std::path::Path;

pub const DEFAULT_CLIP_HISTORY: i64 = 20;
pub const DEFAULT_TILES: &[&str] = &["wifi", "bt", "night"];
pub const ALLOWED_TILES: &[&str] = &["wifi", "bt", "night", "plane"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipQuickConfig {
    pub clip_history: i64,
    pub tiles: Vec<String>,
}

impl Default for ClipQuickConfig {
    fn default() -> Self {
        Self {
            clip_history: DEFAULT_CLIP_HISTORY,
            tiles: DEFAULT_TILES.iter().map(|tile| (*tile).into()).collect(),
        }
    }
}

fn normalize_tiles(value: Option<&toml::Value>) -> Vec<String> {
    let tiles = value
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
        .filter(|tile| ALLOWED_TILES.contains(tile))
        .map(str::to_string)
        .collect::<Vec<_>>();
    if tiles.is_empty() {
        DEFAULT_TILES.iter().map(|tile| (*tile).into()).collect()
    } else {
        tiles
    }
}

pub fn load(path: impl AsRef<Path>) -> ClipQuickConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return ClipQuickConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return ClipQuickConfig::default();
    };
    ClipQuickConfig {
        clip_history: value
            .get("clip_history")
            .and_then(toml::Value::as_integer)
            .unwrap_or(DEFAULT_CLIP_HISTORY)
            .clamp(5, 100),
        tiles: normalize_tiles(value.get("tiles")),
    }
}

pub fn save(path: impl AsRef<Path>, config: &ClipQuickConfig) -> std::io::Result<()> {
    let history = config.clip_history.clamp(5, 100);
    let tiles = if config.tiles.is_empty() {
        DEFAULT_TILES.to_vec()
    } else {
        let filtered = config
            .tiles
            .iter()
            .map(String::as_str)
            .filter(|tile| ALLOWED_TILES.contains(tile))
            .collect::<Vec<_>>();
        if filtered.is_empty() {
            DEFAULT_TILES.to_vec()
        } else {
            filtered
        }
    };
    let rendered = tiles
        .iter()
        .map(|tile| format!("{tile:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let text =
        format!("# Kyth quick settings + clip\nclip_history = {history}\ntiles = [{rendered}]\n");
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

pub fn klipper_commands(config: &ClipQuickConfig) -> [Vec<String>; 2] {
    let history = config.clip_history.clamp(5, 100);
    let keep = vec![
        "kwriteconfig5",
        "--file",
        "klipperrc",
        "--group",
        "General",
        "--key",
        "KeepClipboardContents",
        if history > 0 { "true" } else { "false" },
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let mut max_items = vec![
        "kwriteconfig5",
        "--file",
        "klipperrc",
        "--group",
        "General",
        "--key",
        "MaxClipItems",
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();
    max_items.push(history.to_string());
    [keep, max_items]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn normalizes_history_and_tiles_and_round_trips() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("quick.toml");
        std::fs::write(
            &path,
            "clip_history = 999\ntiles = [\"wifi\", \"secret\"]\n",
        )
        .unwrap();
        assert_eq!(
            load(&path),
            ClipQuickConfig {
                clip_history: 100,
                tiles: vec!["wifi".into()]
            }
        );
        save(
            &path,
            &ClipQuickConfig {
                clip_history: 12,
                tiles: vec!["plane".into()],
            },
        )
        .unwrap();
        assert_eq!(load(&path).tiles, vec!["plane"]);
    }

    #[test]
    fn emits_only_fixed_klipper_argv() {
        let commands = klipper_commands(&ClipQuickConfig {
            clip_history: 7,
            tiles: vec![],
        });
        assert_eq!(
            commands[0],
            vec![
                "kwriteconfig5",
                "--file",
                "klipperrc",
                "--group",
                "General",
                "--key",
                "KeepClipboardContents",
                "true"
            ]
        );
        assert_eq!(commands[1].last().map(String::as_str), Some("7"));
    }
}
