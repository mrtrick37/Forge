//! Offline search preference configuration.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchConfig {
    pub baloo: bool,
    pub recent: i64,
    pub apps_weight: i64,
    pub files_weight: i64,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            baloo: true,
            recent: 20,
            apps_weight: 3,
            files_weight: 1,
        }
    }
}

fn clamp(config: SearchConfig) -> SearchConfig {
    SearchConfig {
        baloo: config.baloo,
        recent: config.recent.clamp(5, 100),
        apps_weight: config.apps_weight.clamp(1, 5),
        files_weight: config.files_weight.clamp(1, 5),
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join("kyth/search.toml");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
        .join(".config/kyth/search.toml")
}

pub fn load(path: impl AsRef<Path>) -> SearchConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return SearchConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return SearchConfig::default();
    };
    let table = value.as_table();
    clamp(SearchConfig {
        baloo: table
            .and_then(|table| table.get("baloo"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(true),
        recent: table
            .and_then(|table| table.get("recent"))
            .and_then(toml::Value::as_integer)
            .unwrap_or(20),
        apps_weight: table
            .and_then(|table| table.get("apps_weight"))
            .and_then(toml::Value::as_integer)
            .unwrap_or(3),
        files_weight: table
            .and_then(|table| table.get("files_weight"))
            .and_then(toml::Value::as_integer)
            .unwrap_or(1),
    })
}

pub fn save(path: impl AsRef<Path>, config: &SearchConfig) -> std::io::Result<()> {
    let config = clamp(config.clone());
    let text = format!("# Kyth search parity — baloo + kickoff weights\nbaloo = {}\nrecent = {}\napps_weight = {}\nfiles_weight = {}\n", config.baloo, config.recent, config.apps_weight, config.files_weight);
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clamps_search_weights_and_round_trips() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("search.toml");
        save(
            &path,
            &SearchConfig {
                baloo: false,
                recent: 500,
                apps_weight: 0,
                files_weight: 9,
            },
        )
        .unwrap();
        assert_eq!(
            load(&path),
            SearchConfig {
                baloo: false,
                recent: 100,
                apps_weight: 1,
                files_weight: 5
            }
        );
    }
}
