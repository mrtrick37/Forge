//! Offline privacy preset configuration.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivacyConfig {
    pub geoclue: bool,
    pub fingerprint: bool,
    pub telem_opt_out: bool,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            geoclue: false,
            fingerprint: false,
            telem_opt_out: true,
        }
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join("kyth/privacy.toml");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
        .join(".config/kyth/privacy.toml")
}

pub fn load(path: impl AsRef<Path>) -> PrivacyConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return PrivacyConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return PrivacyConfig::default();
    };
    PrivacyConfig {
        geoclue: value
            .get("geoclue")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        fingerprint: value
            .get("fingerprint")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        telem_opt_out: value
            .get("telem_opt_out")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true),
    }
}

pub fn save(path: impl AsRef<Path>, config: PrivacyConfig) -> std::io::Result<()> {
    let text = format!(
        "# Kyth privacy preset, offline\ngeoclue = {}\nfingerprint = {}\ntelem_opt_out = {}\n",
        config.geoclue, config.fingerprint, config.telem_opt_out
    );
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn secure_defaults_round_trip() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("privacy.toml");
        assert_eq!(load(&path), PrivacyConfig::default());
        save(
            &path,
            PrivacyConfig {
                geoclue: true,
                fingerprint: true,
                telem_opt_out: false,
            },
        )
        .unwrap();
        assert_eq!(
            load(&path),
            PrivacyConfig {
                geoclue: true,
                fingerprint: true,
                telem_opt_out: false
            }
        );
    }
}
