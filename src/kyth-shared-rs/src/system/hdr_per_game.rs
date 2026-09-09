//! Per-game HDR peak configuration.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameHdr {
    pub peak_nits: i64,
    pub itm: bool,
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join("kyth/hdr-per-game.toml");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
        .join(".config/kyth/hdr-per-game.toml")
}

impl Default for GameHdr {
    fn default() -> Self {
        Self {
            peak_nits: 400,
            itm: false,
        }
    }
}

fn clamp(config: GameHdr) -> GameHdr {
    GameHdr {
        peak_nits: config.peak_nits.clamp(100, 4_000),
        itm: config.itm,
    }
}

pub fn load(path: impl AsRef<Path>) -> BTreeMap<String, GameHdr> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return BTreeMap::new();
    };
    value
        .get("games")
        .and_then(toml::Value::as_table)
        .map(|games| {
            games
                .iter()
                .filter_map(|(app, value)| {
                    let table = value.as_table()?;
                    Some((
                        app.clone(),
                        clamp(GameHdr {
                            peak_nits: table
                                .get("peak_nits")
                                .and_then(toml::Value::as_integer)
                                .unwrap_or(400),
                            itm: table
                                .get("itm")
                                .and_then(toml::Value::as_bool)
                                .unwrap_or(false),
                        }),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn save(path: impl AsRef<Path>, games: &BTreeMap<String, GameHdr>) -> std::io::Result<()> {
    let mut text = String::from("# Kyth HDR per-game — offline\n");
    for (app, config) in games {
        let config = clamp(config.clone());
        text.push_str(&format!(
            "[games.{app:?}]\npeak_nits = {}\nitm = {}\n\n",
            config.peak_nits, config.itm
        ));
    }
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

pub fn for_app(app: &str, path: impl AsRef<Path>) -> Option<GameHdr> {
    load(path).remove(app)
}

pub fn cache_key(app: &str, driver_version: Option<&str>) -> String {
    format!("{}:{}", app, driver_version.unwrap_or("unknown"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clamps_and_round_trips_game_settings() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("hdr-per-game.toml");
        let mut games = BTreeMap::new();
        games.insert(
            "123".into(),
            GameHdr {
                peak_nits: 9_000,
                itm: true,
            },
        );
        save(&path, &games).unwrap();
        assert_eq!(
            for_app("123", &path),
            Some(GameHdr {
                peak_nits: 4_000,
                itm: true
            })
        );
        assert_eq!(for_app("missing", &path), None);
    }

    #[test]
    fn cache_key_includes_driver_version() {
        assert_eq!(cache_key("123", Some("mesa-25")), "123:mesa-25");
        assert_eq!(cache_key("123", None), "123:unknown");
    }
}
