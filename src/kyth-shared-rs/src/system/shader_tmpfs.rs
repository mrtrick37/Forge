//! Offline Mesa shader-cache tmpfs preference and unit rendering.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderTmpfsConfig {
    pub enabled: bool,
    pub size: String,
}

impl Default for ShaderTmpfsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            size: "2G".into(),
        }
    }
}

fn normalize(config: ShaderTmpfsConfig) -> ShaderTmpfsConfig {
    ShaderTmpfsConfig {
        size: matches!(config.size.as_str(), "1G" | "2G" | "4G")
            .then_some(config.size)
            .unwrap_or_else(|| "2G".into()),
        ..config
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/shader-tmpfs.toml");
        }
    }
    PathBuf::from("/etc/kyth/shader-tmpfs.toml")
}

pub fn load(path: impl AsRef<Path>) -> ShaderTmpfsConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return ShaderTmpfsConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return ShaderTmpfsConfig::default();
    };
    normalize(ShaderTmpfsConfig {
        enabled: value
            .get("enabled")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        size: value
            .get("size")
            .and_then(toml::Value::as_str)
            .unwrap_or("2G")
            .into(),
    })
}

pub fn save(path: impl AsRef<Path>, config: &ShaderTmpfsConfig) -> std::io::Result<()> {
    let config = normalize(config.clone());
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth shader tmpfs — offline\nenabled = {}\nsize = {:?}\n",
            config.enabled, config.size
        ),
        Some(0o600),
    )
}

pub fn generate(
    config: &ShaderTmpfsConfig,
    tmpfiles: impl AsRef<Path>,
    service: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let config = normalize(config.clone());
    let tmpfiles = tmpfiles.as_ref();
    let service = service.as_ref();
    if !config.enabled {
        for path in [tmpfiles, service] {
            match std::fs::remove_file(path) {
                Ok(()) | Err(_) => {}
            }
        }
        return Ok(None);
    }
    crate::atomic_io::atomic_write_text(
        tmpfiles,
        "# Kyth shader tmpfs — generated\nd /run/kyth-shader 0755 - - -\n",
        Some(0o644),
    )?;
    let content = format!("[Unit]\nDescription=Kyth shader tmpfs — Mesa cache on tmpfs\nAfter=local-fs.target\n[Service]\nType=oneshot\nRemainAfterExit=yes\nExecStart=/bin/sh -c 'mkdir -p /run/kyth-shader && mount -t tmpfs -o size={},mode=0755 tmpfs /run/kyth-shader'\nExecStop=/bin/sh -c 'umount /run/kyth-shader 2>/dev/null || true'\n[Install]\nWantedBy=multi-user.target\n", config.size);
    crate::atomic_io::atomic_write_text(service, &content, Some(0o644))?;
    Ok(Some(service.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clamps_size_and_reversibly_renders_units() {
        let directory = tempdir().unwrap();
        let config_path = directory.path().join("shader.toml");
        let tmpfiles = directory.path().join("shader.conf");
        let service = directory.path().join("shader.service");
        save(
            &config_path,
            &ShaderTmpfsConfig {
                enabled: true,
                size: "8G".into(),
            },
        )
        .unwrap();
        let config = load(&config_path);
        assert_eq!(config.size, "2G");
        generate(&config, &tmpfiles, &service).unwrap();
        assert!(service.exists());
        generate(&ShaderTmpfsConfig::default(), &tmpfiles, &service).unwrap();
        assert!(!service.exists());
    }
}
