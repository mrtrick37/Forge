//! Offline Btrfs performance configuration and mount drop-in rendering.
//!
//! This ports the declarative portion of `kyth_shared.btrfs_perf`. It never
//! remounts a filesystem or reloads systemd; callers explicitly own those
//! operations after reviewing the generated drop-in.

use std::path::{Path, PathBuf};

pub const DEFAULT_CONFIG: &str = "/etc/kyth/btrfs-perf.toml";
pub const DEFAULT_DROP_IN: &str = "/etc/systemd/system/-.mount.d/99-kyth-btrfs.conf";
pub const DEFAULT_VAR_DROP_IN: &str = "/etc/systemd/system/var.mount.d/99-kyth-btrfs.conf";

const VALID_COMPRESS: &[&str] = &["zstd:1", "zstd:3", "zstd", "lzo", "off"];

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/btrfs-perf.toml");
        }
    }
    PathBuf::from(DEFAULT_CONFIG)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtrfsPerfConfig {
    pub profile: String,
    pub compress: String,
}

impl Default for BtrfsPerfConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            compress: "zstd:1".into(),
        }
    }
}

pub fn load(path: impl AsRef<Path>) -> BtrfsPerfConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return BtrfsPerfConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return BtrfsPerfConfig::default();
    };
    let profile = value
        .get("profile")
        .and_then(toml::Value::as_str)
        .unwrap_or("balanced")
        .to_ascii_lowercase();
    let profile = matches!(profile.as_str(), "balanced" | "kyth")
        .then_some(profile)
        .unwrap_or_else(|| "balanced".into());
    let compress = value
        .get("compress")
        .and_then(toml::Value::as_str)
        .unwrap_or("zstd:1")
        .to_string();
    let compress = VALID_COMPRESS
        .contains(&compress.as_str())
        .then_some(compress)
        .unwrap_or_else(|| "zstd:1".into());
    BtrfsPerfConfig { profile, compress }
}

pub fn save(path: impl AsRef<Path>, config: &BtrfsPerfConfig) -> std::io::Result<()> {
    let profile = matches!(config.profile.to_ascii_lowercase().as_str(), "kyth")
        .then_some("kyth")
        .unwrap_or("balanced");
    let text = format!(
        "# Kyth btrfs perf — offline\nprofile = \"{profile}\"\ncompress = \"{}\"\n",
        config.compress
    );
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

pub fn mount_options(compress: &str) -> String {
    let compress = if compress == "zstd" {
        "zstd:1"
    } else {
        compress
    };
    let compression = (compress != "off").then(|| format!("compress-force={compress}"));
    [
        compression,
        Some("noatime".into()),
        Some("space_cache=v2".into()),
        Some("commit=120".into()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(",")
}

fn remove_if_present(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Render or remove the root mount drop-in.
///
/// When `destination` is `None`, the installed defaults are used and the
/// `/var` drop-in is managed as well. Supplying a destination makes the
/// operation single-target, which keeps tests and callers from touching an
/// unrelated mount unit.
pub fn generate(
    config: &BtrfsPerfConfig,
    destination: Option<&Path>,
) -> std::io::Result<Option<PathBuf>> {
    let root = destination
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DROP_IN));
    let targets = if destination.is_some() {
        vec![root.clone()]
    } else {
        vec![root.clone(), PathBuf::from(DEFAULT_VAR_DROP_IN)]
    };
    if config.profile != "kyth" {
        for target in &targets {
            remove_if_present(target)?;
        }
        return Ok(None);
    }
    let content = format!(
        "# Kyth btrfs perf — generated\n[Mount]\nOptions={}\n",
        mount_options(&config.compress)
    );
    for target in &targets {
        crate::atomic_io::atomic_write_text(target, &content, Some(0o644))?;
    }
    Ok(Some(root))
}

pub fn status(drop_in: impl AsRef<Path>) -> &'static str {
    if drop_in.as_ref().is_file() {
        "kyth"
    } else {
        "balanced"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn loads_defaults_and_normalizes_invalid_values() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("btrfs-perf.toml");
        fs::write(&path, "profile = \"KYTH\"\ncompress = \"bad\"\n").unwrap();
        assert_eq!(
            load(&path),
            BtrfsPerfConfig {
                profile: "kyth".into(),
                compress: "zstd:1".into()
            }
        );
        fs::remove_file(&path).unwrap();
        assert_eq!(load(&path), BtrfsPerfConfig::default());
    }

    #[test]
    fn renders_mount_options_like_python() {
        assert_eq!(
            mount_options("zstd"),
            "compress-force=zstd:1,noatime,space_cache=v2,commit=120"
        );
        assert_eq!(mount_options("off"), "noatime,space_cache=v2,commit=120");
    }

    #[test]
    fn saves_and_generates_or_removes_one_explicit_dropin() {
        let directory = tempdir().unwrap();
        let config_path = directory.path().join("btrfs-perf.toml");
        let drop_in = directory.path().join("99-kyth-btrfs.conf");
        let config = BtrfsPerfConfig {
            profile: "kyth".into(),
            compress: "zstd:3".into(),
        };
        save(&config_path, &config).unwrap();
        assert_eq!(load(&config_path), config);
        assert_eq!(
            generate(&config, Some(&drop_in)).unwrap(),
            Some(drop_in.clone())
        );
        assert_eq!(fs::read_to_string(&drop_in).unwrap(), "# Kyth btrfs perf — generated\n[Mount]\nOptions=compress-force=zstd:3,noatime,space_cache=v2,commit=120\n");
        assert_eq!(status(&drop_in), "kyth");
        let balanced = BtrfsPerfConfig::default();
        assert_eq!(generate(&balanced, Some(&drop_in)).unwrap(), None);
        assert_eq!(status(&drop_in), "balanced");
    }
}
