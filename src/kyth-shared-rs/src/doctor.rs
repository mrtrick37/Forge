//! Read-only KythOS health scoring.
//!
//! This is the portable portion of `kyth_shared.doctor`: deterministic score
//! calculation is separated from the small amount of local evidence
//! collection so callers and tests can supply fixtures without touching a
//! live desktop or running repair commands.

use serde::Serialize;
use std::path::Path;

use crate::system::desktop_stack::StackCheck;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DoctorInputs {
    pub has_cachy_kernel: bool,
    pub hardware_capabilities: Option<Vec<String>>,
    pub zram_configured: bool,
    pub btrfs_root: bool,
    pub scx_active: bool,
    pub desktop_stack: Vec<StackCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorReport {
    pub score: u8,
    pub checks: Vec<String>,
    pub suggestions: Vec<String>,
}

/// Score supplied evidence using the same weights and remediation wording as
/// the Python doctor. Desktop-stack checks are informational and only
/// non-advisory failures reduce the score.
pub fn evaluate(inputs: &DoctorInputs) -> DoctorReport {
    let mut score: i16 = 0;
    let mut checks = Vec::new();
    let mut suggestions = Vec::new();

    if inputs.has_cachy_kernel {
        checks.push("kernel: cachy (opt-in)".to_string());
    } else {
        checks.push("kernel: fedora (default)".to_string());
        suggestions.push("For v3: just build-base cachy".to_string());
    }
    score += 20;

    if let Some(capabilities) = inputs
        .hardware_capabilities
        .as_ref()
        .filter(|caps| !caps.is_empty())
    {
        let displayed = capabilities
            .iter()
            .take(2)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        checks.push(format!("v3: {displayed}"));
        score += 20;
    } else {
        checks.push("v3: unknown".to_string());
        suggestions.push("Run kyth-probe --system".to_string());
    }

    if inputs.zram_configured {
        checks.push("zram: yes".to_string());
        score += 20;
    } else {
        checks.push("zram: no".to_string());
        suggestions.push("Enable zram: systemctl enable --now kyth-zram-swap.service".to_string());
    }

    if inputs.btrfs_root {
        checks.push("btrfs: yes".to_string());
        score += 20;
    } else {
        checks.push("btrfs: no".to_string());
    }

    checks.push(format!(
        "scx: {}",
        if inputs.scx_active {
            "active"
        } else {
            "inactive (opt-in)"
        }
    ));
    score += 20;
    if !inputs.scx_active {
        suggestions.push("Try scx: kyth-scx set lavd".to_string());
    }

    let hard_failures = inputs
        .desktop_stack
        .iter()
        .filter(|check| !check.passed && !check.advisory)
        .collect::<Vec<_>>();
    let soft_failures = inputs
        .desktop_stack
        .iter()
        .filter(|check| !check.passed && check.advisory)
        .collect::<Vec<_>>();
    if hard_failures.is_empty() {
        checks.push("desktop-stack: packages ok".to_string());
    } else {
        score = (score - 15).max(0);
        checks.push(format!(
            "desktop-stack: {}",
            hard_failures
                .iter()
                .map(|check| check.detail.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        ));
        suggestions.push(
            "Ensure xdg-desktop-portal + xdg-desktop-portal-kde are on the image".to_string(),
        );
    }
    for failure in soft_failures {
        checks.push(format!(
            "desktop-stack warn: {}: {}",
            failure.name, failure.detail
        ));
        if failure.name.to_ascii_lowercase().contains("portal") {
            suggestions.push("Restart portals: systemctl --user restart xdg-desktop-portal xdg-desktop-portal-kde".to_string());
        } else if matches!(failure.name.as_str(), "PipeWire" | "WirePlumber") {
            suggestions.push(
                "Restart audio: systemctl --user restart pipewire pipewire-pulse wireplumber"
                    .to_string(),
            );
        }
    }

    DoctorReport {
        score: score.clamp(0, 100) as u8,
        checks,
        suggestions,
    }
}

fn has_cachy_kernel() -> bool {
    std::fs::read_dir("/usr/lib/modules")
        .map(|entries| {
            entries
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().contains("cachy"))
        })
        .unwrap_or(false)
}

fn hardware_capabilities() -> Option<Vec<String>> {
    let cached = crate::system::probe::read_section("hardware-summary")
        .and_then(|value| value.get("capabilities")?.as_array().cloned())
        .map(|values| {
            values
                .into_iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty());
    cached.or_else(|| {
        crate::system::hardware_view::get_hardware_view_summary().map(|view| view.capabilities)
    })
}

/// Collect the same local, read-only evidence used by the doctor CLI.
pub fn collect_report() -> DoctorReport {
    let btrfs_root = std::fs::read_to_string("/proc/mounts")
        .map(|mounts| mounts.contains("btrfs"))
        .unwrap_or(false);
    evaluate(&DoctorInputs {
        has_cachy_kernel: has_cachy_kernel(),
        hardware_capabilities: hardware_capabilities(),
        zram_configured: Path::new("/usr/lib/systemd/zram-generator.conf").exists()
            || Path::new("/etc/systemd/zram-generator.conf").exists(),
        btrfs_root,
        scx_active: Path::new("/sys/kernel/sched_ext/state").exists(),
        desktop_stack: crate::system::desktop_stack::desktop_stack_checks(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack_check(name: &str, passed: bool, advisory: bool) -> StackCheck {
        StackCheck {
            name: name.into(),
            passed,
            detail: "test detail".into(),
            advisory,
        }
    }

    #[test]
    fn healthy_inputs_score_one_hundred() {
        let report = evaluate(&DoctorInputs {
            has_cachy_kernel: true,
            hardware_capabilities: Some(vec![
                "gpu.amd".into(),
                "gaming.lowlatency".into(),
                "extra".into(),
            ]),
            zram_configured: true,
            btrfs_root: true,
            scx_active: true,
            desktop_stack: vec![stack_check("Portal packages", true, false)],
        });
        assert_eq!(report.score, 100);
        assert!(report
            .checks
            .contains(&"v3: gpu.amd, gaming.lowlatency".to_string()));
        assert!(report.suggestions.is_empty());
    }

    #[test]
    fn hard_stack_failure_reduces_score_and_suggests_repair() {
        let report = evaluate(&DoctorInputs {
            hardware_capabilities: None,
            desktop_stack: vec![stack_check("Portal packages", false, false)],
            ..Default::default()
        });
        assert_eq!(report.score, 25);
        assert!(report
            .checks
            .iter()
            .any(|check| check.starts_with("desktop-stack: test detail")));
        assert!(report
            .suggestions
            .iter()
            .any(|suggestion| suggestion.contains("xdg-desktop-portal")));
    }

    #[test]
    fn advisory_audio_failure_does_not_reduce_score() {
        let report = evaluate(&DoctorInputs {
            desktop_stack: vec![stack_check("PipeWire", false, true)],
            ..Default::default()
        });
        assert_eq!(report.score, 40);
        assert!(report
            .suggestions
            .iter()
            .any(|suggestion| suggestion.contains("Restart audio")));
    }
}
