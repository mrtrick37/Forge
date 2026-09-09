//! Offline print/scan autopilot configuration model.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintConfig {
    pub auto_add: bool,
    pub airscan: bool,
}

impl Default for PrintConfig {
    fn default() -> Self {
        Self {
            auto_add: true,
            airscan: true,
        }
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/print.toml");
        }
    }
    PathBuf::from("/etc/kyth/print.toml")
}

pub fn load(path: impl AsRef<Path>) -> PrintConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return PrintConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return PrintConfig::default();
    };
    let table = value.as_table();
    PrintConfig {
        auto_add: table
            .and_then(|table| table.get("auto_add"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(true),
        airscan: table
            .and_then(|table| table.get("airscan"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(true),
    }
}

pub fn save(path: impl AsRef<Path>, config: &PrintConfig) -> std::io::Result<()> {
    let text = format!(
        "# Kyth Print/Scan autopilot\nauto_add = {}\nairscan = {}\n",
        config.auto_add, config.airscan
    );
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn loads_defaults_and_saves_values() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("print.toml");
        assert_eq!(load(&path), PrintConfig::default());
        let config = PrintConfig {
            auto_add: false,
            airscan: true,
        };
        save(&path, &config).unwrap();
        assert_eq!(load(&path), config);
        assert!(fs::read_to_string(path)
            .unwrap()
            .contains("auto_add = false"));
    }
}
