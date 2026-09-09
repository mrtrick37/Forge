//! Performance regression gate configuration and ledger comparison.

use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerfGateConfig {
    pub threshold: i64,
    pub enabled: bool,
}

impl Default for PerfGateConfig {
    fn default() -> Self {
        Self {
            threshold: 10,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PerfGateResult {
    pub enabled: bool,
    pub pass: bool,
    pub threshold: Option<i64>,
    pub last: Option<f64>,
    pub current: Option<f64>,
    pub delta: Option<f64>,
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/perf-gate.toml");
        }
    }
    PathBuf::from("/etc/kyth/perf-gate.toml")
}

pub fn load(path: impl AsRef<Path>) -> PerfGateConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return PerfGateConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return PerfGateConfig::default();
    };
    PerfGateConfig {
        threshold: value
            .get("threshold")
            .and_then(toml::Value::as_integer)
            .unwrap_or(10)
            .clamp(1, 20),
        enabled: value
            .get("enabled")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true),
    }
}

pub fn save(path: impl AsRef<Path>, config: PerfGateConfig) -> std::io::Result<()> {
    let config = PerfGateConfig {
        threshold: config.threshold.clamp(1, 20),
        ..config
    };
    let text = format!(
        "# Kyth perf gate — offline\nthreshold = {}\nenabled = {}\n",
        config.threshold, config.enabled
    );
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

fn last_p95(ledger: impl AsRef<Path>) -> Option<f64> {
    let raw = std::fs::read_to_string(ledger).ok()?;
    raw.lines().rev().take(10).find_map(|line| {
        serde_json::from_str::<Value>(line)
            .ok()?
            .get("p95")?
            .as_f64()
    })
}

pub fn check(
    config: PerfGateConfig,
    current_ms: Option<f64>,
    ledger: impl AsRef<Path>,
) -> PerfGateResult {
    if !config.enabled {
        return PerfGateResult {
            enabled: false,
            pass: true,
            threshold: None,
            last: None,
            current: None,
            delta: None,
        };
    }
    let last = last_p95(ledger);
    let delta = last.zip(current_ms).map(|(last, current)| {
        if last != 0.0 {
            (current - last) / last * 100.0
        } else {
            0.0
        }
    });
    PerfGateResult {
        enabled: true,
        pass: delta.is_none_or(|value| value <= config.threshold as f64),
        threshold: Some(config.threshold),
        last,
        current: current_ms,
        delta: delta.map(|value| (value * 100.0).round() / 100.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clamps_config_and_reads_recent_ledger_value() {
        let directory = tempdir().unwrap();
        let config_path = directory.path().join("perf-gate.toml");
        std::fs::write(&config_path, "threshold = 999\nenabled = false\n").unwrap();
        assert_eq!(
            load(&config_path),
            PerfGateConfig {
                threshold: 20,
                enabled: false
            }
        );
        let ledger = directory.path().join("ledger.jsonl");
        std::fs::write(&ledger, "{\"p95\": 100}\n{\"p95\": 120}\n").unwrap();
        let result = check(PerfGateConfig::default(), Some(130.0), &ledger);
        assert_eq!(result.last, Some(120.0));
        assert_eq!(result.delta, Some(8.33));
        assert!(result.pass);
    }

    #[test]
    fn disabled_gate_always_passes() {
        let directory = tempdir().unwrap();
        let result = check(
            PerfGateConfig {
                threshold: 1,
                enabled: false,
            },
            Some(9_999.0),
            directory.path().join("missing"),
        );
        assert!(result.pass);
        assert!(!result.enabled);
    }
}
