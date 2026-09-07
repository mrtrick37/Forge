//! Battery configuration and health reads.
//!
//! Configuration writes use the shared atomic replacement helper. Sysfs health
//! reads are best-effort and never make a device mutation.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_START: i64 = 40;
const DEFAULT_STOP: i64 = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatteryConfig {
    pub charge_start: i64,
    pub charge_stop: i64,
    pub health_check: bool,
}

impl Default for BatteryConfig {
    fn default() -> Self {
        Self { charge_start: DEFAULT_START, charge_stop: DEFAULT_STOP, health_check: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BatteryHealth {
    pub capacity: String,
    pub cycles: String,
}

pub fn battery_config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join("kyth/battery.toml");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
        .join(".config/kyth/battery.toml")
}

fn clamp_config(config: BatteryConfig) -> BatteryConfig {
    BatteryConfig {
        charge_start: config.charge_start.clamp(20, 50),
        charge_stop: config.charge_stop.clamp(60, 100),
        health_check: config.health_check,
    }
}

pub fn load_battery(path: impl AsRef<Path>) -> BatteryConfig {
    let Ok(raw) = fs::read_to_string(path) else { return BatteryConfig::default(); };
    let Ok(value) = raw.parse::<toml::Value>() else { return BatteryConfig::default(); };
    let table = value.as_table();
    clamp_config(BatteryConfig {
        charge_start: table.and_then(|table| table.get("charge_start")).and_then(toml::Value::as_integer).unwrap_or(DEFAULT_START),
        charge_stop: table.and_then(|table| table.get("charge_stop")).and_then(toml::Value::as_integer).unwrap_or(DEFAULT_STOP),
        health_check: table.and_then(|table| table.get("health_check")).and_then(toml::Value::as_bool).unwrap_or(true),
    })
}

pub fn load_battery_default() -> BatteryConfig {
    load_battery(battery_config_path(None::<PathBuf>))
}

pub fn save_battery(path: impl AsRef<Path>, config: &BatteryConfig) -> std::io::Result<()> {
    let config = clamp_config(config.clone());
    let text = format!(
        "# Kyth battery — charge thresholds, offline\ncharge_start = {}\ncharge_stop = {}\nhealth_check = {}\n",
        config.charge_start, config.charge_stop, config.health_check
    );
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

pub fn read_battery_health_in(root: impl AsRef<Path>) -> BTreeMap<String, BatteryHealth> {
    let mut health = BTreeMap::new();
    let Ok(entries) = fs::read_dir(root) else { return health; };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("BAT") { continue; }
        let directory = entry.path();
        let read = |file: &str| fs::read_to_string(directory.join(file)).map(|value| value.trim().to_string()).unwrap_or_else(|_| "?".to_string());
        health.insert(name, BatteryHealth { capacity: read("capacity"), cycles: read("cycle_count") });
    }
    health
}

pub fn read_battery_health() -> BTreeMap<String, BatteryHealth> {
    read_battery_health_in("/sys/class/power_supply")
}

/// Default ledger path for periodic health snapshots.
pub const LEDGER_PATH: &str = "/var/cache/kyth/battery.jsonl";

/// Current UTC timestamp in ISO-8601 form, mirroring
/// `datetime.utcnow().isoformat()`.
pub fn utc_now_iso() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|span| span.as_secs() as libc::time_t)
        .unwrap_or(0);
    let mut broken = unsafe { std::mem::zeroed::<libc::tm>() };
    unsafe { libc::gmtime_r(&now, &mut broken) };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        broken.tm_year + 1900,
        broken.tm_mon + 1,
        broken.tm_mday,
        broken.tm_hour,
        broken.tm_min,
        broken.tm_sec
    )
}

/// Append one health snapshot line to the JSONL ledger, creating parent
/// directories as needed. Failures are swallowed by the caller.
pub fn append_ledger(
    path: &Path,
    health: &BTreeMap<String, BatteryHealth>,
    config: &BatteryConfig,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let entry = serde_json::json!({
        "health": health,
        "cfg": {
            "charge_start": config.charge_start,
            "charge_stop": config.charge_stop,
            "health_check": config.health_check,
        },
        "ts": utc_now_iso(),
    });
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(&entry).unwrap_or_default())
}

/// Write the charge-stop threshold to every battery's control file.
/// Missing files and write failures are skipped.
pub fn apply_threshold(stop: i64) {
    let Ok(entries) = fs::read_dir("/sys/class/power_supply") else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("BAT") {
            continue;
        }
        let _ = fs::write(entry.path().join("charge_control_end_threshold"), stop.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn loads_defaults_and_clamps_values() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("battery.toml");
        fs::write(&path, "charge_start = 5\ncharge_stop = 120\nhealth_check = false\n").unwrap();
        assert_eq!(load_battery(&path), BatteryConfig { charge_start: 20, charge_stop: 100, health_check: false });
        assert_eq!(load_battery(directory.path().join("missing.toml")), BatteryConfig::default());
    }

    #[test]
    fn ledger_appends_timestamped_health_snapshots() {
        let directory = tempdir().unwrap();
        let ledger = directory.path().join("nested/battery.jsonl");
        let mut health = BTreeMap::new();
        health.insert(
            "BAT0".to_string(),
            super::BatteryHealth { capacity: "87".to_string(), cycles: "12".to_string() },
        );
        super::append_ledger(&ledger, &health, &super::BatteryConfig::default()).unwrap();
        let line = fs::read_to_string(&ledger).unwrap();
        let entry: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(entry["cfg"]["charge_stop"], 80);
        assert_eq!(entry["health"]["BAT0"]["cycles"], "12");
        assert!(entry["ts"].as_str().is_some_and(|ts| ts.len() == 19));
        assert!(super::utc_now_iso().len() == 19);
    }

    #[test]
    fn saves_clamped_config_atomically() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("battery.toml");
        save_battery(&path, &BatteryConfig { charge_start: 1, charge_stop: 101, health_check: true }).unwrap();
        assert_eq!(load_battery(&path), BatteryConfig { charge_start: 20, charge_stop: 100, health_check: true });
    }

    #[test]
    fn reads_battery_sysfs_shape_without_real_sysfs() {
        let directory = tempdir().unwrap();
        let battery = directory.path().join("BAT0");
        fs::create_dir(&battery).unwrap();
        fs::write(battery.join("capacity"), "87\n").unwrap();
        fs::write(battery.join("cycle_count"), "123\n").unwrap();
        let result = read_battery_health_in(directory.path());
        assert_eq!(result["BAT0"], BatteryHealth { capacity: "87".into(), cycles: "123".into() });
    }
}
