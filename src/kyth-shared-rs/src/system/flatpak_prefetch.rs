//! Offline Flatpak prefetch schedule preference.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatpakPrefetchConfig {
    pub enabled: bool,
    pub time: String,
}

impl Default for FlatpakPrefetchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            time: "02:00".into(),
        }
    }
}

fn normalize_time(value: &str) -> String {
    if value.contains(':') && value.len() <= 5 {
        value.into()
    } else {
        "02:00".into()
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/flatpak-prefetch.toml");
        }
    }
    PathBuf::from("/etc/kyth/flatpak-prefetch.toml")
}

pub fn load(path: impl AsRef<Path>) -> FlatpakPrefetchConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return FlatpakPrefetchConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return FlatpakPrefetchConfig::default();
    };
    FlatpakPrefetchConfig {
        enabled: value
            .get("enabled")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        time: normalize_time(
            value
                .get("time")
                .and_then(toml::Value::as_str)
                .unwrap_or("02:00"),
        ),
    }
}

pub fn save(path: impl AsRef<Path>, config: &FlatpakPrefetchConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth flatpak prefetch — offline\nenabled = {}\ntime = {:?}\n",
            config.enabled,
            normalize_time(&config.time)
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
    "[Unit]\nDescription=Kyth flatpak prefetch — off-peak\n[Service]\nType=oneshot\nExecStart=/usr/bin/flatpak update --no-deploy -y\nNice=10\nIOSchedulingClass=best-effort\nIOSchedulingPriority=7\n"
}

pub fn render_timer(config: &FlatpakPrefetchConfig) -> String {
    let mut parts = config.time.split(':');
    let hour = parts.next().unwrap_or("02");
    let minute = parts.next().unwrap_or("00");
    format!("[Unit]\nDescription=Kyth flatpak prefetch timer\n[Timer]\nOnCalendar=*-*-* {hour}:{minute}:00\nPersistent=true\n[Install]\nWantedBy=timers.target\n")
}

pub fn generate(
    config: &FlatpakPrefetchConfig,
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
    crate::atomic_io::atomic_write_text(timer, &render_timer(config), Some(0o644))?;
    Ok(Some(service.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn defaults_and_normalizes_schedule() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("prefetch.toml");
        std::fs::write(&path, "enabled = true\ntime = \"invalid\"\n").unwrap();
        assert_eq!(
            load(&path),
            FlatpakPrefetchConfig {
                enabled: true,
                time: "02:00".into()
            }
        );
        save(
            &path,
            &FlatpakPrefetchConfig {
                enabled: true,
                time: "23:30".into(),
            },
        )
        .unwrap();
        assert_eq!(load(&path).time, "23:30");
    }
}
