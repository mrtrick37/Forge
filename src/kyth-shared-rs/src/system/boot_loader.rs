//! Read-only boot-loader fast-path configuration.
//!
//! The privileged `/boot/loader/loader.conf` writer remains outside this
//! module. Rust consumers can safely inspect the configuration and present
//! its effective state without changing boot behavior.

use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_CONFIG_PATH: &str = "/etc/kyth/loader.toml";
const DEFAULT_LOADER_CONF: &str = "/boot/loader/loader.conf";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderConfig {
    pub fast: bool,
    pub timeout: i64,
}

impl Default for LoaderConfig {
    fn default() -> Self {
        Self {
            fast: false,
            timeout: 2,
        }
    }
}

pub fn loader_config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/loader.toml");
        }
    }
    PathBuf::from(DEFAULT_CONFIG_PATH)
}

pub fn load_loader(path: impl AsRef<Path>) -> LoaderConfig {
    let Ok(raw) = fs::read_to_string(path) else {
        return LoaderConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return LoaderConfig::default();
    };
    let table = value.as_table();
    let fast = table
        .and_then(|table| table.get("fast"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    let default_timeout = if fast { 0 } else { 2 };
    let timeout = table
        .and_then(|table| table.get("timeout"))
        .and_then(toml::Value::as_integer)
        .unwrap_or(default_timeout)
        .clamp(0, 10);
    LoaderConfig { fast, timeout }
}

pub fn load_loader_default() -> LoaderConfig {
    load_loader(loader_config_path(None::<PathBuf>))
}

pub fn save_loader(path: impl AsRef<Path>, config: &LoaderConfig) -> std::io::Result<()> {
    let timeout = config.timeout.clamp(0, 10);
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth loader — offline\nfast = {}\ntimeout = {}\n",
            config.fast, timeout
        ),
        Some(0o600),
    )
}

pub fn loader_status(conf: impl AsRef<Path>) -> &'static str {
    let Ok(raw) = fs::read_to_string(conf) else {
        return "balanced";
    };
    if raw.contains("timeout 0") || raw.contains("Kyth") {
        "fast"
    } else {
        "balanced"
    }
}

pub fn loader_status_default() -> &'static str {
    loader_status(DEFAULT_LOADER_CONF)
}

pub fn generate_loader_conf(
    config: &LoaderConfig,
    destination: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let destination = destination.as_ref();
    if !config.fast {
        if destination.is_file()
            && fs::read_to_string(destination)
                .ok()
                .is_some_and(|text| text.contains("Kyth"))
        {
            crate::atomic_io::atomic_write_text(destination, "timeout 2\n", Some(0o644))?;
        }
        return Ok(None);
    }
    crate::atomic_io::atomic_write_text(
        destination,
        &format!(
            "# Kyth loader fast-path — generated, greenboot-aware\ntimeout {}\n",
            config.timeout
        ),
        Some(0o644),
    )?;
    Ok(Some(destination.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn loads_defaults_and_clamps_timeout() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("loader.toml");
        fs::write(&path, "fast = true\ntimeout = 99\n").unwrap();
        assert_eq!(
            load_loader(&path),
            LoaderConfig {
                fast: true,
                timeout: 10
            }
        );
        assert_eq!(
            load_loader(directory.path().join("missing.toml")),
            LoaderConfig::default()
        );
    }

    #[test]
    fn detects_effective_loader_status() {
        let directory = tempdir().unwrap();
        let fast = directory.path().join("loader.conf");
        fs::write(&fast, "# Kyth loader\ntimeout 0\n").unwrap();
        assert_eq!(loader_status(&fast), "fast");
        fs::write(&fast, "timeout 2\n").unwrap();
        assert_eq!(loader_status(&fast), "balanced");
        assert_eq!(loader_status(directory.path().join("missing")), "balanced");
    }
}
