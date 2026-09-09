//! Offline readahead preference model.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadaheadConfig {
    pub enabled: bool,
    pub size_mb: i64,
}

impl Default for ReadaheadConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            size_mb: 512,
        }
    }
}

fn clamp(config: ReadaheadConfig) -> ReadaheadConfig {
    ReadaheadConfig {
        size_mb: config.size_mb.clamp(64, 4096),
        ..config
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/readahead.toml");
        }
    }
    PathBuf::from("/etc/kyth/readahead.toml")
}

pub fn load(path: impl AsRef<Path>) -> ReadaheadConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return ReadaheadConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return ReadaheadConfig::default();
    };
    clamp(ReadaheadConfig {
        enabled: value
            .get("enabled")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true),
        size_mb: value
            .get("size_mb")
            .and_then(toml::Value::as_integer)
            .unwrap_or(512),
    })
}

pub fn save(path: impl AsRef<Path>, config: &ReadaheadConfig) -> std::io::Result<()> {
    let config = clamp(config.clone());
    let text = format!(
        "# Kyth readahead — offline\nchecked = {}\nenabled = {}\nsize_mb = {}\n",
        config.enabled, config.enabled, config.size_mb
    );
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clamps_size_and_preserves_enabled_state() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("readahead.toml");
        save(
            &path,
            &ReadaheadConfig {
                enabled: false,
                size_mb: 9_999,
            },
        )
        .unwrap();
        assert_eq!(
            load(&path),
            ReadaheadConfig {
                enabled: false,
                size_mb: 4096
            }
        );
    }
}
