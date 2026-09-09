//! Telemetry opt-in/collector preference model.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryOptConfig {
    pub enabled: bool,
    pub collectors: Vec<String>,
}

impl Default for TelemetryOptConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            collectors: Vec::new(),
        }
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/telemetry-opt.toml");
        }
    }
    PathBuf::from("/etc/kyth/telemetry-opt.toml")
}

pub fn load(path: impl AsRef<Path>) -> TelemetryOptConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return TelemetryOptConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return TelemetryOptConfig::default();
    };
    let collectors = value
        .get("collectors")
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    TelemetryOptConfig {
        enabled: value
            .get("enabled")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true),
        collectors,
    }
}

pub fn save(path: impl AsRef<Path>, config: &TelemetryOptConfig) -> std::io::Result<()> {
    let collectors = config
        .collectors
        .iter()
        .map(|collector| format!("{collector:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth telemetry opt — offline\nenabled = {}\ncollectors = [{}]\n",
            config.enabled, collectors
        ),
        Some(0o600),
    )
}

pub fn effective_collectors(config: &TelemetryOptConfig, allowed: &[&str]) -> Vec<String> {
    if !config.enabled {
        return Vec::new();
    }
    config
        .collectors
        .iter()
        .filter(|collector| allowed.contains(&collector.as_str()))
        .cloned()
        .collect()
}

pub fn purge(path: impl AsRef<Path>) -> std::io::Result<()> {
    save(
        path,
        &TelemetryOptConfig {
            enabled: false,
            collectors: Vec::new(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn filters_collectors_against_allowed_set() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("telemetry-opt.toml");
        let config = TelemetryOptConfig {
            enabled: true,
            collectors: vec!["cpu".into(), "secret".into(), "gpu".into()],
        };
        save(&path, &config).unwrap();
        let loaded = load(&path);
        assert_eq!(
            effective_collectors(&loaded, &["cpu", "gpu"]),
            vec!["cpu", "gpu"]
        );
    }

    #[test]
    fn purge_is_disabled_and_empty() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("telemetry-opt.toml");
        purge(&path).unwrap();
        let config = load(&path);
        assert!(!config.enabled);
        assert!(effective_collectors(&config, &["cpu"]).is_empty());
    }
}
