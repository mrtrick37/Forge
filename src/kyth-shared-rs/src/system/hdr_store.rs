//! Persistent HDR-store preference and read-only audit projection.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HdrStoreConfig {
    pub preserve: bool,
}

impl Default for HdrStoreConfig {
    fn default() -> Self {
        Self { preserve: true }
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/hdr-store.toml");
        }
    }
    PathBuf::from("/etc/kyth/hdr-store.toml")
}

pub fn load(path: impl AsRef<Path>) -> HdrStoreConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return HdrStoreConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return HdrStoreConfig::default();
    };
    HdrStoreConfig {
        preserve: value
            .get("preserve")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true),
    }
}

pub fn save(path: impl AsRef<Path>, config: HdrStoreConfig) -> std::io::Result<()> {
    let text = format!(
        "# Kyth HDR store — offline\npreserve = {}\n",
        config.preserve
    );
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

/// Produce the small audit shape consumed by settings/status pages.
pub fn audit(path: impl AsRef<Path>) -> BTreeMap<String, i64> {
    super::hdr::load(path)
        .into_iter()
        .map(|(name, display)| (name, display.peak_nits))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn defaults_on_missing_or_malformed_store() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("hdr-store.toml");
        assert_eq!(load(&path), HdrStoreConfig::default());
        fs::write(&path, "preserve = \"yes\"\n").unwrap();
        assert_eq!(load(&path), HdrStoreConfig::default());
    }

    #[test]
    fn saves_preference_and_audits_display_peaks() {
        let directory = tempdir().unwrap();
        let store = directory.path().join("hdr-store.toml");
        save(&store, HdrStoreConfig { preserve: false }).unwrap();
        assert!(!load(&store).preserve);
        let display = directory.path().join("display-hdr.toml");
        fs::write(&display, "[displays.HDMI-1]\npeak_nits = 800\n").unwrap();
        assert_eq!(audit(&display).get("HDMI-1"), Some(&800));
    }
}
