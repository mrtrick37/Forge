//! Offline signing preference and Git config projection.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SigningConfig {
    pub gpg_key: String,
    pub cosign_key: String,
    pub gitsign: bool,
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join("kyth/signing.toml");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
        .join(".config/kyth/signing.toml")
}

pub fn load(path: impl AsRef<Path>) -> SigningConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return SigningConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return SigningConfig::default();
    };
    SigningConfig {
        gpg_key: value
            .get("gpg_key")
            .and_then(toml::Value::as_str)
            .unwrap_or("")
            .into(),
        cosign_key: value
            .get("cosign_key")
            .and_then(toml::Value::as_str)
            .unwrap_or("")
            .into(),
        gitsign: value
            .get("gitsign")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
    }
}

pub fn save(path: impl AsRef<Path>, config: &SigningConfig) -> std::io::Result<()> {
    let text = format!(
        "# Kyth signing preset, offline\ngpg_key = {:?}\ncosign_key = {:?}\ngitsign = {}\n",
        config.gpg_key, config.cosign_key, config.gitsign
    );
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

pub fn git_config(config: &SigningConfig) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    if !config.gpg_key.is_empty() {
        env.insert("commit.gpgsign".into(), "true".into());
        env.insert("user.signingkey".into(), config.gpg_key.clone());
    }
    if config.gitsign {
        env.insert("gpg.x509.program".into(), "gitsign".into());
        env.insert("gpg.format".into(), "x509".into());
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn round_trips_and_projects_git_settings() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("signing.toml");
        let config = SigningConfig {
            gpg_key: "ABC".into(),
            cosign_key: "cosign.key".into(),
            gitsign: true,
        };
        save(&path, &config).unwrap();
        assert_eq!(load(&path), config);
        assert_eq!(git_config(&config).get("gpg.format"), Some(&"x509".into()));
    }
}
