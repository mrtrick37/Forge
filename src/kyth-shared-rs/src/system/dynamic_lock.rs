//! Trusted-device dynamic lock state and helpers.
//!
//! Ports the self-contained `kyth-dynamic-lock` launcher: config load
//! with grace clamping plus the arm/missing/grace state machine. Live
//! KDE Connect queries, session locking, and the poll loop stay with
//! the launcher binary. There is no shared `dynamic_lock.py` fixture.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const POLL_SECONDS: u64 = 5;
pub const DEFAULT_GRACE_SECONDS: u64 = 60;
pub const MIN_GRACE_SECONDS: u64 = 15;
pub const MAX_GRACE_SECONDS: u64 = 600;

/// Global run flag flipped by the termination handler.
pub static RUNNING: AtomicBool = AtomicBool::new(true);

/// Resolve the config path: `KYTH_DYNAMIC_LOCK_CONFIG` wins, otherwise
/// `~/.config/kyth-dynamic-lock.json`.
pub fn config_path(home: &Path, override_path: Option<&str>) -> PathBuf {
    if let Some(path) = override_path.filter(|path| !path.is_empty()) {
        PathBuf::from(path)
    } else {
        home.join(".config/kyth-dynamic-lock.json")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockConfig {
    pub enabled: bool,
    pub device_id: String,
    pub grace_seconds: u64,
}

fn parse_grace(value: Option<&serde_json::Value>) -> u64 {
    let seconds = match value {
        Some(serde_json::Value::Bool(flag)) => Some(i64::from(*flag)),
        Some(serde_json::Value::Number(number)) => number.as_i64().or_else(|| number.as_f64().map(|float| float as i64)),
        Some(serde_json::Value::String(text)) => text.trim().parse::<i64>().ok(),
        _ => None,
    };
    match seconds {
        Some(seconds) => seconds.clamp(MIN_GRACE_SECONDS as i64, MAX_GRACE_SECONDS as i64) as u64,
        None => DEFAULT_GRACE_SECONDS,
    }
}

/// Coerce `device_id` like `str(value or "")`: null, empty, false,
/// and zero all mean "no device".
fn device_string(value: Option<&serde_json::Value>) -> String {
    match value {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::String(text)) => text.trim().to_string(),
        Some(serde_json::Value::Bool(true)) => "True".to_string(),
        Some(serde_json::Value::Bool(false)) => String::new(),
        Some(serde_json::Value::Number(number)) => {
            if number.as_i64().is_some_and(|int| int == 0)
                || number.as_f64().is_some_and(|float| float == 0.0)
            {
                String::new()
            } else {
                number.to_string()
            }
        }
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// Load and normalize the daemon config. Missing, unreadable, or
/// non-object documents disable the daemon, mirroring the launcher.
pub fn load_config(path: &Path) -> LockConfig {
    let value: Option<serde_json::Value> =
        std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok());
    let table = value.and_then(|value| match value {
        serde_json::Value::Object(map) => Some(map),
        _ => None,
    });
    let device_id = device_string(table.as_ref().and_then(|map| map.get("device_id")));
    let enabled = table
        .as_ref()
        .and_then(|map| map.get("enabled"))
        .is_some_and(|value| value.as_bool().unwrap_or(false))
        && !device_id.is_empty();
    let grace_seconds = parse_grace(table.as_ref().and_then(|map| map.get("grace_seconds")));
    LockConfig { enabled, device_id, grace_seconds }
}

/// One KDE Connect availability answer: a broken query (`Unavailable`)
/// must never count as the device leaving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Unavailable,
    Present(bool),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Monitor {
    pub device_id: String,
    pub armed: bool,
    pub missing_since: Option<f64>,
}

impl Monitor {
    /// Advance the state machine one poll. Returns true when the session
    /// must be locked now.
    pub fn step(
        &mut self,
        config: &LockConfig,
        availability: Availability,
        now_monotonic: f64,
    ) -> bool {
        if !config.enabled {
            self.device_id.clear();
            self.armed = false;
            self.missing_since = None;
            return false;
        }
        if config.device_id != self.device_id {
            self.device_id = config.device_id.clone();
            self.armed = false;
            self.missing_since = None;
        }
        match availability {
            Availability::Unavailable => {
                self.missing_since = None;
            }
            Availability::Present(true) => {
                self.armed = true;
                self.missing_since = None;
            }
            Availability::Present(false) if self.armed => {
                if let Some(since) = self.missing_since {
                    if now_monotonic - since >= config.grace_seconds as f64 {
                        // Require the trusted device to return before
                        // another lock attempt.
                        self.armed = false;
                        self.missing_since = None;
                        return true;
                    }
                } else {
                    self.missing_since = Some(now_monotonic);
                }
            }
            Availability::Present(false) => {}
        }
        false
    }
}

pub fn keep_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(device: &str, grace: u64) -> LockConfig {
        LockConfig { enabled: true, device_id: device.to_string(), grace_seconds: grace }
    }

    #[test]
    fn loads_and_clamps_daemon_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kyth-dynamic-lock.json");
        std::fs::write(&path, "{\"enabled\": true, \"device_id\": \"  abc  \", \"grace_seconds\": 5}").unwrap();
        let loaded = load_config(&path);
        assert!(loaded.enabled);
        assert_eq!(loaded.device_id, "abc");
        assert_eq!(loaded.grace_seconds, MIN_GRACE_SECONDS);
        std::fs::write(&path, "{\"enabled\": true, \"device_id\": \"x\", \"grace_seconds\": \"nope\"}").unwrap();
        assert_eq!(load_config(&path).grace_seconds, DEFAULT_GRACE_SECONDS);
        std::fs::write(&path, "[1, 2]").unwrap();
        assert!(!load_config(&path).enabled);
        assert!(!load_config(&dir.path().join("missing.json")).enabled);
    }

    #[test]
    fn arms_on_sight_and_locks_after_grace() {
        let mut monitor = Monitor::default();
        let config = config("phone", 60);
        assert!(!monitor.step(&config, Availability::Present(false), 0.0));
        assert!(!monitor.armed);
        assert!(!monitor.step(&config, Availability::Present(true), 5.0));
        assert!(monitor.armed);
        assert!(!monitor.step(&config, Availability::Present(false), 10.0));
        assert!(!monitor.step(&config, Availability::Present(false), 69.0));
        assert!(monitor.step(&config, Availability::Present(false), 70.0));
        assert!(!monitor.armed);
        // Re-arm requires the device to return first.
        assert!(!monitor.step(&config, Availability::Present(false), 200.0));
        assert!(!monitor.step(&config, Availability::Present(true), 201.0));
        assert!(!monitor.step(&config, Availability::Present(false), 202.0));
        assert!(monitor.step(&config, Availability::Present(false), 202.0 + 60.0));
    }

    #[test]
    fn broken_queries_and_disables_reset_state() {
        let mut monitor = Monitor::default();
        let config = config("phone", 60);
        monitor.step(&config, Availability::Present(true), 0.0);
        monitor.step(&config, Availability::Unavailable, 5.0);
        assert!(monitor.armed);
        assert!(monitor.missing_since.is_none());
        let off = LockConfig { enabled: false, device_id: String::new(), grace_seconds: 60 };
        monitor.step(&off, Availability::Present(false), 6.0);
        assert!(!monitor.armed);
        assert_eq!(monitor.device_id, "");
    }
}
