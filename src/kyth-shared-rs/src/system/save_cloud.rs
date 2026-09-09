//! Offline per-game save-cloud configuration.
//!
//! This ports the config model from `kyth_shared.save_cloud`. Restic/rclone
//! execution and save discovery remain explicit caller-owned operations.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_REPO: &str = "/var/cache/kyth/saves";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveCloudConfig {
    pub repo: String,
    pub remote: String,
    pub on_battery: bool,
}

impl Default for SaveCloudConfig {
    fn default() -> Self {
        Self {
            repo: DEFAULT_REPO.into(),
            remote: String::new(),
            on_battery: false,
        }
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join("kyth/save-cloud.toml");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
        .join(".config/kyth/save-cloud.toml")
}

pub fn load(path: impl AsRef<Path>) -> SaveCloudConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return SaveCloudConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return SaveCloudConfig::default();
    };
    let Some(table) = value.as_table() else {
        return SaveCloudConfig::default();
    };
    SaveCloudConfig {
        repo: table
            .get("repo")
            .and_then(toml::Value::as_str)
            .unwrap_or(DEFAULT_REPO)
            .into(),
        remote: table
            .get("remote")
            .and_then(toml::Value::as_str)
            .unwrap_or("")
            .into(),
        on_battery: table
            .get("on_battery")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
    }
}

pub fn save(path: impl AsRef<Path>, config: &SaveCloudConfig) -> std::io::Result<()> {
    let text = format!(
        "# Kyth save cloud — restic local + rclone remote, offline\nrepo = {:?}\nremote = {:?}\non_battery = {}\n",
        config.repo, config.remote, config.on_battery,
    );
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

/// Compat `drive_c` directories under every Steam compatdata prefix, mirroring
/// the `compatdata/*/pfx/drive_c` glob. Only existing paths are returned.
pub fn compat_drive_cs(home: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(prefixes) = std::fs::read_dir(home.join(".local/share/Steam/steamapps/compatdata"))
    else {
        return found;
    };
    for prefix in prefixes.flatten() {
        let candidate = prefix.path().join("pfx/drive_c");
        if candidate.exists() {
            found.push(candidate);
        }
    }
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn finds_only_existing_compat_drive_c_paths() {
        let home = tempdir().unwrap();
        let compatdata = home.path().join(".local/share/Steam/steamapps/compatdata");
        fs::create_dir_all(compatdata.join("123/pfx/drive_c")).unwrap();
        fs::create_dir_all(compatdata.join("456/pfx")).unwrap();
        fs::create_dir_all(compatdata.join("789")).unwrap();
        assert_eq!(
            compat_drive_cs(home.path()),
            vec![compatdata.join("123/pfx/drive_c")]
        );
        assert!(compat_drive_cs(&home.path().join("missing-home")).is_empty());
    }

    #[test]
    fn defaults_when_config_is_missing() {
        let directory = tempdir().unwrap();
        assert_eq!(
            load(directory.path().join("missing.toml")),
            SaveCloudConfig::default()
        );
    }

    #[test]
    fn round_trips_quoted_save_cloud_values_atomically() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("save-cloud.toml");
        let config = SaveCloudConfig {
            repo: "/mnt/My Saves".into(),
            remote: "nas:games".into(),
            on_battery: true,
        };
        save(&path, &config).unwrap();
        assert_eq!(load(&path), config);
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o077,
            0
        );
    }

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
}
