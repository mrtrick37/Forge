//! Offline I/O tuning preference and udev rule rendering.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IoTuneConfig {
    pub profile: String,
    pub read_ahead_kb: i64,
}

impl Default for IoTuneConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            read_ahead_kb: 128,
        }
    }
}

fn normalize(config: IoTuneConfig) -> IoTuneConfig {
    IoTuneConfig {
        profile: if config.profile == "kyth" {
            "kyth".into()
        } else {
            "balanced".into()
        },
        read_ahead_kb: config.read_ahead_kb.clamp(8, 4096),
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/io.toml");
        }
    }
    PathBuf::from("/etc/kyth/io.toml")
}

pub fn load(path: impl AsRef<Path>) -> IoTuneConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return IoTuneConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return IoTuneConfig::default();
    };
    normalize(IoTuneConfig {
        profile: value
            .get("profile")
            .and_then(toml::Value::as_str)
            .unwrap_or("balanced")
            .into(),
        read_ahead_kb: value
            .get("read_ahead_kb")
            .and_then(toml::Value::as_integer)
            .unwrap_or(128),
    })
}

pub fn save(path: impl AsRef<Path>, config: &IoTuneConfig) -> std::io::Result<()> {
    let config = normalize(config.clone());
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth I/O tune — offline\nprofile = {:?}\nread_ahead_kb = {}\n",
            config.profile, config.read_ahead_kb
        ),
        Some(0o600),
    )
}

pub fn generate(
    config: &IoTuneConfig,
    destination: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let config = normalize(config.clone());
    let destination = destination.as_ref();
    if config.profile != "kyth" {
        match std::fs::remove_file(destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        return Ok(None);
    }
    let content = format!("# Kyth I/O tune — generated, remove with ujust io-tune default\nACTION==\"add|change\", KERNEL==\"nvme[0-9]*n[0-9]*\", ATTR{{queue/scheduler}}=\"none\"\nACTION==\"add|change\", KERNEL==\"nvme[0-9]*n[0-9]*\", ATTR{{queue/read_ahead_kb}}=\"{}\"\nACTION==\"add|change\", KERNEL==\"nvme[0-9]*n[0-9]*\", ATTR{{queue/wbt_lat_usec}}=\"0\"\nACTION==\"add|change\", KERNEL==\"sd[a-z]*\", ATTR{{queue/scheduler}}=\"mq-deadline\"\nACTION==\"add|change\", KERNEL==\"sd[a-z]*\", ATTR{{queue/read_ahead_kb}}=\"1024\"\n", config.read_ahead_kb);
    crate::atomic_io::atomic_write_text(destination, &content, Some(0o644))?;
    Ok(Some(destination.to_path_buf()))
}

pub fn status(rule: impl AsRef<Path>) -> &'static str {
    if rule.as_ref().is_file() {
        "kyth"
    } else {
        "balanced"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clamps_and_renders_udev_rule() {
        let directory = tempdir().unwrap();
        let config_path = directory.path().join("io.toml");
        let rule = directory.path().join("io.rules");
        save(
            &config_path,
            &IoTuneConfig {
                profile: "kyth".into(),
                read_ahead_kb: 9_999,
            },
        )
        .unwrap();
        let config = load(&config_path);
        assert_eq!(config.read_ahead_kb, 4096);
        generate(&config, &rule).unwrap();
        assert!(std::fs::read_to_string(rule)
            .unwrap()
            .contains("read_ahead_kb}=\"4096\""));
    }
}
