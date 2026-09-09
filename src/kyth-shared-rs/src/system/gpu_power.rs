//! Offline GPU power preference model.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuPowerConfig {
    pub profile: String,
    pub dpm: String,
}

impl Default for GpuPowerConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            dpm: "auto".into(),
        }
    }
}

fn normalize(config: GpuPowerConfig) -> GpuPowerConfig {
    GpuPowerConfig {
        profile: if config.profile == "kyth" {
            "kyth".into()
        } else {
            "balanced".into()
        },
        dpm: match config.dpm.as_str() {
            "high" | "low" => config.dpm,
            _ => "auto".into(),
        },
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/gpu-power.toml");
        }
    }
    PathBuf::from("/etc/kyth/gpu-power.toml")
}

pub fn load(path: impl AsRef<Path>) -> GpuPowerConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return GpuPowerConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return GpuPowerConfig::default();
    };
    normalize(GpuPowerConfig {
        profile: value
            .get("profile")
            .and_then(toml::Value::as_str)
            .unwrap_or("balanced")
            .into(),
        dpm: value
            .get("dpm")
            .and_then(toml::Value::as_str)
            .unwrap_or("auto")
            .into(),
    })
}

pub fn save(path: impl AsRef<Path>, config: &GpuPowerConfig) -> std::io::Result<()> {
    let config = normalize(config.clone());
    let text = format!(
        "# Kyth GPU power — offline\nprofile = {:?}\ndpm = {:?}\n",
        config.profile, config.dpm
    );
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

pub fn status(path: impl AsRef<Path>) -> String {
    load(path).profile
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn defaults_and_round_trips_power_profile() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("gpu-power.toml");
        assert_eq!(load(&path), GpuPowerConfig::default());
        save(
            &path,
            &GpuPowerConfig {
                profile: "kyth".into(),
                dpm: "high".into(),
            },
        )
        .unwrap();
        assert_eq!(status(&path), "kyth");
    }
}
