//! Offline Distrobox cache configuration model.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistroboxCacheConfig {
    pub enabled: bool,
    pub size: String,
    pub ccache_size: String,
}

impl Default for DistroboxCacheConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            size: "4G".into(),
            ccache_size: "10G".into(),
        }
    }
}

fn normalize(config: DistroboxCacheConfig) -> DistroboxCacheConfig {
    DistroboxCacheConfig {
        enabled: config.enabled,
        size: match config.size.as_str() {
            "2G" | "8G" => config.size,
            _ => "4G".into(),
        },
        ccache_size: match config.ccache_size.as_str() {
            "5G" | "20G" => config.ccache_size,
            _ => "10G".into(),
        },
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/distrobox-cache.toml");
        }
    }
    PathBuf::from("/etc/kyth/distrobox-cache.toml")
}

pub fn load(path: impl AsRef<Path>) -> DistroboxCacheConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return DistroboxCacheConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return DistroboxCacheConfig::default();
    };
    normalize(DistroboxCacheConfig {
        enabled: value
            .get("enabled")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        size: value
            .get("size")
            .and_then(toml::Value::as_str)
            .unwrap_or("4G")
            .into(),
        ccache_size: value
            .get("ccache_size")
            .and_then(toml::Value::as_str)
            .unwrap_or("10G")
            .into(),
    })
}

pub fn save(path: impl AsRef<Path>, config: &DistroboxCacheConfig) -> std::io::Result<()> {
    let config = normalize(config.clone());
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth distrobox cache — offline\nenabled = {}\nsize = {:?}\nccache_size = {:?}\n",
            config.enabled, config.size, config.ccache_size
        ),
        Some(0o600),
    )
}

pub fn status(service: impl AsRef<Path>) -> &'static str {
    if service.as_ref().is_file() {
        "enabled"
    } else {
        "off"
    }
}

pub fn render_tmpfiles() -> &'static str {
    "# Kyth distrobox cache — generated\nd /run/kyth-distrobox-cache 0755 1000 1000 -\n"
}

pub fn render_service(config: &DistroboxCacheConfig) -> String {
    let config = normalize(config.clone());
    format!("[Unit]\nDescription=Kyth distrobox cache — tmpfs for ccache/cargo\nAfter=local-fs.target\n[Service]\nType=oneshot\nRemainAfterExit=yes\nExecStart=/bin/sh -c 'mkdir -p /run/kyth-distrobox-cache && mount -t tmpfs -o size={},mode=0755 tmpfs /run/kyth-distrobox-cache && mkdir -p /run/kyth-distrobox-cache/ccache /run/kyth-distrobox-cache/cargo && ccache --max-size={} 2>/dev/null || true'\nExecStop=/bin/sh -c 'umount /run/kyth-distrobox-cache 2>/dev/null || true'\n[Install]\nWantedBy=multi-user.target\n", config.size, config.ccache_size)
}

pub fn generate(
    config: &DistroboxCacheConfig,
    tmpfiles: impl AsRef<Path>,
    service: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
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
    crate::atomic_io::atomic_write_text(tmpfiles, render_tmpfiles(), Some(0o644))?;
    crate::atomic_io::atomic_write_text(service, &render_service(config), Some(0o644))?;
    Ok(Some(service.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clamps_cache_sizes_and_round_trips() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("distrobox-cache.toml");
        std::fs::write(
            &path,
            "enabled = true\nsize = \"99G\"\nccache_size = \"1G\"\n",
        )
        .unwrap();
        assert_eq!(
            load(&path),
            DistroboxCacheConfig {
                enabled: true,
                size: "4G".into(),
                ccache_size: "10G".into()
            }
        );
        save(
            &path,
            &DistroboxCacheConfig {
                enabled: true,
                size: "8G".into(),
                ccache_size: "20G".into(),
            },
        )
        .unwrap();
        assert_eq!(load(&path).size, "8G");
    }
}
