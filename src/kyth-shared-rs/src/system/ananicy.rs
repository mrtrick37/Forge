//! Offline Ananicy gaming preset and explicit rule rendering.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnanicyConfig {
    pub profile: String,
    pub nice: i64,
    pub ioclass: String,
}

impl Default for AnanicyConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            nice: -12,
            ioclass: "realtime".into(),
        }
    }
}

fn normalize(config: AnanicyConfig) -> AnanicyConfig {
    AnanicyConfig {
        profile: if config.profile.eq_ignore_ascii_case("kyth") {
            "kyth".into()
        } else {
            "balanced".into()
        },
        nice: config.nice.clamp(-20, 0),
        ioclass: match config.ioclass.as_str() {
            "best-effort" => "best-effort",
            "idle" => "idle",
            _ => "realtime",
        }
        .into(),
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/ananicy.toml");
        }
    }
    PathBuf::from("/etc/kyth/ananicy.toml")
}

pub fn load(path: impl AsRef<Path>) -> AnanicyConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return AnanicyConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return AnanicyConfig::default();
    };
    normalize(AnanicyConfig {
        profile: value
            .get("profile")
            .and_then(toml::Value::as_str)
            .unwrap_or("balanced")
            .into(),
        nice: value
            .get("nice")
            .and_then(toml::Value::as_integer)
            .unwrap_or(-12),
        ioclass: value
            .get("ioclass")
            .and_then(toml::Value::as_str)
            .unwrap_or("realtime")
            .into(),
    })
}

pub fn save(path: impl AsRef<Path>, config: &AnanicyConfig) -> std::io::Result<()> {
    let config = normalize(config.clone());
    let text = format!(
        "# Kyth ananicy — offline\nprofile = {:?}\nnice = {}\nioclass = {:?}\n",
        config.profile, config.nice, config.ioclass
    );
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

pub fn generate(
    config: &AnanicyConfig,
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
    let content = format!("# Kyth ananicy gaming — generated\n{{\"name\":\"gaming.slice\",\"type\":\"cgroup\",\"nice\":{},\"ioclass\":{:?},\"sched\":\"batch\"}}\n", config.nice, config.ioclass);
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
    fn normalizes_config_and_renders_rule() {
        let directory = tempdir().unwrap();
        let config_path = directory.path().join("ananicy.toml");
        let rule = directory.path().join("99-kyth-gaming.conf");
        std::fs::write(
            &config_path,
            "profile = \"kyth\"\nnice = -99\nioclass = \"bad\"\n",
        )
        .unwrap();
        let config = load(&config_path);
        assert_eq!(
            config,
            AnanicyConfig {
                profile: "kyth".into(),
                nice: -20,
                ioclass: "realtime".into()
            }
        );
        assert_eq!(generate(&config, &rule).unwrap(), Some(rule.clone()));
        assert!(std::fs::read_to_string(rule)
            .unwrap()
            .contains("\"nice\":-20"));
    }

    #[test]
    fn balanced_removes_rule() {
        let directory = tempdir().unwrap();
        let rule = directory.path().join("rule");
        std::fs::write(&rule, "old").unwrap();
        assert_eq!(generate(&AnanicyConfig::default(), &rule).unwrap(), None);
        assert_eq!(status(&rule), "balanced");
    }
}
