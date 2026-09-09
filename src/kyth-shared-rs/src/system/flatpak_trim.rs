//! Flatpak trim preference model.
//!
//! Unit/timer generation and activation remain service-owned; this module
//! ports the durable setting and status projection for native callers.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlatpakTrimConfig {
    pub enabled: bool,
}

impl Default for FlatpakTrimConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/flatpak-trim.toml");
        }
    }
    PathBuf::from("/etc/kyth/flatpak-trim.toml")
}

pub fn load(path: impl AsRef<Path>) -> FlatpakTrimConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return FlatpakTrimConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return FlatpakTrimConfig::default();
    };
    FlatpakTrimConfig {
        enabled: value
            .get("enabled")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true),
    }
}

pub fn save(path: impl AsRef<Path>, config: FlatpakTrimConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth flatpak trim — offline\nenabled = {}\n",
            config.enabled
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

pub fn render_service() -> &'static str {
    "[Unit]\nDescription=Kyth flatpak trim — remove unused runtimes\n[Service]\nType=oneshot\nExecStart=/usr/bin/flatpak uninstall --unused -y --noninteractive\nNice=19\nIOSchedulingClass=best-effort\nIOSchedulingPriority=7\n"
}

pub fn render_timer() -> &'static str {
    "[Unit]\nDescription=Kyth flatpak trim timer\n[Timer]\nOnCalendar=weekly\nPersistent=true\n[Install]\nWantedBy=timers.target\n"
}

pub fn generate(
    config: FlatpakTrimConfig,
    service: impl AsRef<Path>,
    timer: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let service = service.as_ref();
    let timer = timer.as_ref();
    if !config.enabled {
        for path in [service, timer] {
            match std::fs::remove_file(path) {
                Ok(()) | Err(_) => {}
            }
        }
        return Ok(None);
    }
    crate::atomic_io::atomic_write_text(service, render_service(), Some(0o644))?;
    crate::atomic_io::atomic_write_text(timer, render_timer(), Some(0o644))?;
    Ok(Some(service.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn defaults_to_enabled_and_round_trips() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("flatpak-trim.toml");
        assert_eq!(load(&path), FlatpakTrimConfig::default());
        save(&path, FlatpakTrimConfig { enabled: false }).unwrap();
        assert!(!load(&path).enabled);
    }
}
