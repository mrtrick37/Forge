//! Per-application performance preset configuration.
//!
//! This ports the offline TOML model from `kyth_shared.app_presets`. Applying
//! cgroup drop-ins remains a separate privileged operation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPreset {
    pub cpu_weight: i64,
    pub memory_max: String,
    pub latency: String,
}

impl Default for AppPreset {
    fn default() -> Self {
        Self {
            cpu_weight: 100,
            memory_max: "80%".to_string(),
            latency: "balanced".to_string(),
        }
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join("kyth/app-presets.toml");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
        .join(".config/kyth/app-presets.toml")
}

pub fn load(path: impl AsRef<Path>) -> BTreeMap<String, AppPreset> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return BTreeMap::new();
    };
    let Some(apps) = value.get("apps").and_then(toml::Value::as_table) else {
        return BTreeMap::new();
    };
    apps.iter()
        .filter_map(|(name, value)| {
            let entry = value.as_table()?;
            Some((
                name.clone(),
                AppPreset {
                    cpu_weight: entry
                        .get("cpu_weight")
                        .and_then(toml::Value::as_integer)
                        .unwrap_or(100),
                    memory_max: entry
                        .get("memory_max")
                        .and_then(toml::Value::as_str)
                        .unwrap_or("80%")
                        .to_string(),
                    latency: entry
                        .get("latency")
                        .and_then(toml::Value::as_str)
                        .unwrap_or("balanced")
                        .to_string(),
                },
            ))
        })
        .collect()
}

pub fn preset_for(app_id: &str, path: impl AsRef<Path>) -> AppPreset {
    load(path).remove(app_id).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn loads_nested_app_presets_and_defaults_missing_fields() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("app-presets.toml");
        fs::write(&path, "[apps.\"steam\"]\ncpu_weight = 200\nlatency = \"gaming\"\n[apps.invalid]\nnot_a_table = true\n").unwrap();
        let apps = load(&path);
        assert_eq!(
            apps["steam"],
            AppPreset {
                cpu_weight: 200,
                memory_max: "80%".into(),
                latency: "gaming".into()
            }
        );
        assert_eq!(preset_for("missing", &path), AppPreset::default());
    }
}
