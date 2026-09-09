//! Offline NUMA/X3D profile configuration.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumaConfig {
    pub profile: String,
    pub cpus: String,
}

impl Default for NumaConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            cpus: String::new(),
        }
    }
}

fn normalize(config: NumaConfig) -> NumaConfig {
    let cpus = config
        .cpus
        .chars()
        .all(|c| c.is_ascii_digit() || c == ',' || c == '-')
        .then_some(config.cpus)
        .unwrap_or_default();
    NumaConfig {
        profile: if config.profile == "gaming" {
            "gaming".into()
        } else {
            "balanced".into()
        },
        cpus,
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/numa.toml");
        }
    }
    PathBuf::from("/etc/kyth/numa.toml")
}

pub fn load(path: impl AsRef<Path>) -> NumaConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return NumaConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return NumaConfig::default();
    };
    normalize(NumaConfig {
        profile: value
            .get("profile")
            .and_then(toml::Value::as_str)
            .unwrap_or("balanced")
            .into(),
        cpus: value
            .get("cpus")
            .and_then(toml::Value::as_str)
            .unwrap_or("")
            .into(),
    })
}

pub fn save(path: impl AsRef<Path>, config: &NumaConfig) -> std::io::Result<()> {
    let config = normalize(config.clone());
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth NUMA X3D — offline\nprofile = {:?}\ncpus = {:?}\n",
            config.profile, config.cpus
        ),
        Some(0o600),
    )
}

pub fn effective_cpus(config: &NumaConfig, detected_ccd0: Option<&str>) -> String {
    if !config.cpus.is_empty() {
        config.cpus.clone()
    } else if config.profile == "gaming" {
        detected_ccd0.unwrap_or_default().into()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn validates_cpu_list_and_uses_detected_gaming_set() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("numa.toml");
        std::fs::write(&path, "profile = \"gaming\"\ncpus = \"bad cpu\"\n").unwrap();
        let config = load(&path);
        assert_eq!(effective_cpus(&config, Some("0-7")), "0-7");
        save(
            &path,
            &NumaConfig {
                profile: "gaming".into(),
                cpus: "2,4-6".into(),
            },
        )
        .unwrap();
        assert_eq!(effective_cpus(&load(&path), Some("0-7")), "2,4-6");
    }
}
