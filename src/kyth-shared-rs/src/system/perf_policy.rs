//! Deterministic performance-policy selection.
//!
//! This is the side-effect-free part of the AI performance daemon. Sampling
//! hardware, invoking an optional local model, and applying privileged
//! settings remain outside the shared crate.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const POLICY_TTL_S: u64 = 30;
pub const DEFAULT_SCX_FOR_GAMING: &str = "scx_rusty";
pub const DEFAULT_SCX_FOR_DESKTOP: &str = "scx_bpfland";

/// Parse the `avg10=` field from `/proc/pressure/cpu` or cgroup pressure text.
pub fn pressure_avg10(text: &str) -> Option<f64> {
    text.split_whitespace()
        .find_map(|part| part.strip_prefix("avg10=")?.parse::<f64>().ok())
}

/// Parse a battery capacity file, preserving the daemon's integer contract.
pub fn battery_percent(text: &str) -> Option<i64> {
    text.trim().parse::<i64>().ok()
}

/// Normalize the successful `powerprofilesctl get` output used by the daemon.
pub fn power_profile(success: bool, stdout: &str) -> String {
    if success && !stdout.trim().is_empty() {
        stdout.trim().to_ascii_lowercase()
    } else {
        "unknown".into()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerfSample {
    pub is_gaming: bool,
    pub pressure_some_avg10: f64,
    pub power_profile: String,
    pub battery_percent: Option<i64>,
    pub has_nvidia: bool,
    pub has_amd: bool,
    #[serde(default)]
    pub hdr_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerfPolicy {
    pub scx: String,
    pub sysctl: BTreeMap<String, String>,
    pub gpu_power: String,
    pub reason: String,
    pub ttl: u64,
}

impl PerfPolicy {
    fn new(scx: &str, sysctl: &[(&str, &str)], gpu_power: &str, reason: String) -> Self {
        Self {
            scx: scx.to_string(),
            sysctl: sysctl
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect(),
            gpu_power: gpu_power.to_string(),
            reason,
            ttl: POLICY_TTL_S,
        }
    }

    pub fn as_value(&self) -> Value {
        json!({
            "scx": self.scx,
            "sysctl": self.sysctl,
            "gpu_power": self.gpu_power,
            "reason": self.reason,
            "ttl": self.ttl,
        })
    }
}

pub fn choose_policy(sample: &PerfSample) -> PerfPolicy {
    if sample.is_gaming {
        let mut gpu = if sample.power_profile != "power-saver"
            && (sample.battery_percent.is_none_or(|value| value > 30))
        {
            "high"
        } else {
            "auto"
        };
        if sample.has_nvidia && sample.power_profile == "performance" {
            gpu = "high";
        }
        return PerfPolicy::new(
            DEFAULT_SCX_FOR_GAMING,
            &[
                ("vm.swappiness", "10"),
                ("kernel.sched_latency_ns", "8000000"),
            ],
            gpu,
            "gaming active — scx_rusty + low swappiness".to_string(),
        );
    }

    if sample.pressure_some_avg10 > 40.0 {
        return PerfPolicy::new(
            "scx_lavd",
            &[("vm.swappiness", "15")],
            "auto",
            format!("high pressure {:.1} — lavd", sample.pressure_some_avg10),
        );
    }

    if sample.power_profile == "power-saver"
        || sample.battery_percent.is_some_and(|value| value < 20)
    {
        return PerfPolicy::new(
            "none",
            &[("vm.swappiness", "60")],
            "low",
            "battery saver — scx none".to_string(),
        );
    }

    PerfPolicy::new(
        DEFAULT_SCX_FOR_DESKTOP,
        &[("vm.swappiness", "30")],
        "auto",
        "desktop — bpfland balanced".to_string(),
    )
}

pub fn should_rollback(
    _previous: &PerfPolicy,
    current_fps_p95_ms: Option<f64>,
    baseline_p95_ms: Option<f64>,
) -> bool {
    match (current_fps_p95_ms, baseline_p95_ms) {
        (Some(current), Some(baseline)) => current > baseline * 1.10,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PerfSample {
        PerfSample {
            is_gaming: false,
            pressure_some_avg10: 0.0,
            power_profile: "balanced".into(),
            battery_percent: None,
            has_nvidia: false,
            has_amd: true,
            hdr_active: false,
        }
    }

    #[test]
    fn gaming_prefers_low_latency_and_plugged_gpu_power() {
        let policy = choose_policy(&PerfSample {
            is_gaming: true,
            power_profile: "balanced".into(),
            ..sample()
        });
        assert_eq!(policy.scx, "scx_rusty");
        assert_eq!(policy.gpu_power, "high");
        assert_eq!(policy.sysctl.get("vm.swappiness"), Some(&"10".to_string()));
        assert_eq!(policy.ttl, POLICY_TTL_S);
    }

    #[test]
    fn gaming_battery_and_nvidia_overrides_match_python_rules() {
        let battery = choose_policy(&PerfSample {
            is_gaming: true,
            power_profile: "balanced".into(),
            battery_percent: Some(30),
            ..sample()
        });
        assert_eq!(battery.gpu_power, "auto");

        let nvidia = choose_policy(&PerfSample {
            is_gaming: true,
            power_profile: "performance".into(),
            battery_percent: Some(10),
            has_nvidia: true,
            ..sample()
        });
        assert_eq!(nvidia.gpu_power, "high");
    }

    #[test]
    fn pressure_and_battery_take_priority_over_desktop_default() {
        let pressure = choose_policy(&PerfSample {
            pressure_some_avg10: 40.1,
            ..sample()
        });
        assert_eq!(pressure.scx, "scx_lavd");
        assert!(pressure.reason.contains("40.1"));

        let saver = choose_policy(&PerfSample {
            power_profile: "power-saver".into(),
            ..sample()
        });
        assert_eq!(saver.scx, "none");
        assert_eq!(saver.gpu_power, "low");
    }

    #[test]
    fn serializes_and_applies_ten_percent_rollback_gate() {
        let policy = choose_policy(&sample());
        assert_eq!(policy.as_value()["ttl"], POLICY_TTL_S);
        assert!(!should_rollback(&policy, Some(110.0), Some(100.0)));
        assert!(should_rollback(&policy, Some(110.1), Some(100.0)));
        assert!(!should_rollback(&policy, None, Some(100.0)));
    }

    #[test]
    fn parses_read_only_daemon_inputs_without_collecting_them() {
        assert_eq!(pressure_avg10("some avg10=0.12 avg60=0.30"), Some(0.12));
        assert_eq!(battery_percent(" 42\n"), Some(42));
        assert_eq!(power_profile(true, " Performance\n"), "performance");
        assert_eq!(power_profile(false, "performance"), "unknown");
        assert_eq!(pressure_avg10("some avg60=0.30"), None);
    }
}
