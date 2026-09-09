//! Offline graphics-driver preference model.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverConfig {
    pub gpu: String,
    pub mesa_git: String,
}

impl Default for DriverConfig {
    fn default() -> Self {
        Self {
            gpu: "auto".into(),
            mesa_git: "off".into(),
        }
    }
}

fn normalize(config: DriverConfig) -> DriverConfig {
    DriverConfig {
        gpu: match config.gpu.as_str() {
            "nvidia" | "open" | "amd" => config.gpu,
            _ => "auto".into(),
        },
        mesa_git: if config.mesa_git == "on" {
            "on".into()
        } else {
            "off".into()
        },
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/driver.toml");
        }
    }
    PathBuf::from("/etc/kyth/driver.toml")
}

pub fn load(path: impl AsRef<Path>) -> DriverConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return DriverConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return DriverConfig::default();
    };
    normalize(DriverConfig {
        gpu: value
            .get("gpu")
            .and_then(toml::Value::as_str)
            .unwrap_or("auto")
            .into(),
        mesa_git: value
            .get("mesa_git")
            .and_then(toml::Value::as_str)
            .unwrap_or("off")
            .into(),
    })
}

pub fn save(path: impl AsRef<Path>, config: &DriverConfig) -> std::io::Result<()> {
    let config = normalize(config.clone());
    let text = format!(
        "# Kyth driver helper\ngpu = {:?}\nmesa_git = {:?}\n",
        config.gpu, config.mesa_git
    );
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn normalizes_driver_choices() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("driver.toml");
        std::fs::write(&path, "gpu = \"mystery\"\nmesa_git = \"yes\"\n").unwrap();
        assert_eq!(load(&path), DriverConfig::default());
        save(
            &path,
            &DriverConfig {
                gpu: "amd".into(),
                mesa_git: "on".into(),
            },
        )
        .unwrap();
        assert_eq!(load(&path).gpu, "amd");
    }
}
