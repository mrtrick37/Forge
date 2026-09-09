//! Offline BORE profile persistence and guarded sysctl drop-in rendering.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoreConfig {
    pub profile: String,
}

impl Default for BoreConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
        }
    }
}

fn normalize(config: BoreConfig) -> BoreConfig {
    BoreConfig {
        profile: (config.profile == "gaming")
            .then_some("gaming")
            .unwrap_or("balanced")
            .into(),
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/bore.toml");
        }
    }
    PathBuf::from("/etc/kyth/bore.toml")
}

pub fn load(path: impl AsRef<Path>) -> BoreConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return BoreConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return BoreConfig::default();
    };
    normalize(BoreConfig {
        profile: value
            .get("profile")
            .and_then(toml::Value::as_str)
            .unwrap_or("balanced")
            .into(),
    })
}

pub fn save(path: impl AsRef<Path>, config: &BoreConfig) -> std::io::Result<()> {
    let config = normalize(config.clone());
    crate::atomic_io::atomic_write_text(
        path,
        &format!("# Kyth Bore — offline\nprofile = \"{}\"\n", config.profile),
        Some(0o600),
    )
}

pub fn generate(
    config: &BoreConfig,
    destination: impl AsRef<Path>,
    scx_active: bool,
) -> std::io::Result<Option<PathBuf>> {
    let destination = destination.as_ref();
    let config = normalize(config.clone());
    if config.profile != "gaming" || scx_active {
        match std::fs::remove_file(destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        return Ok(None);
    }
    crate::atomic_io::atomic_write_text(
        destination,
        "# Kyth Bore gaming — generated\nkernel.sched_bore=1\nkernel.sched_bore_burst_penalty_offset=12\n",
        Some(0o644),
    )?;
    Ok(Some(destination.to_path_buf()))
}

pub fn status(destination: impl AsRef<Path>) -> &'static str {
    if destination.as_ref().is_file() {
        "gaming"
    } else {
        "balanced"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn normalizes_and_guards_bore_drop_in() {
        let directory = tempdir().unwrap();
        let config_path = directory.path().join("bore.toml");
        let drop_in = directory.path().join("bore.conf");
        std::fs::write(&config_path, "profile = \"gaming\"\n").unwrap();
        let config = load(&config_path);
        assert_eq!(config.profile, "gaming");
        generate(&config, &drop_in, false).unwrap();
        assert!(drop_in.exists());
        generate(&config, &drop_in, true).unwrap();
        assert!(!drop_in.exists());
    }
}
