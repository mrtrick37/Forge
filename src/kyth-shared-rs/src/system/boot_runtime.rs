//! Port of `kyth_shared.system.boot_runtime` — runtime boot assertions.
//!
//! These checks are intentionally conservative.  A graphical boot is judged
//! by the display manager and DRM device rather than `graphical.target`: the
//! greenboot health service can run before that target is allowed to settle.

use std::path::Path;
use std::time::{Duration, Instant};

pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(300);
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(2);
const GRAPHICAL_TARGET: &str = "graphical.target";
const DISPLAY_MANAGER_UNITS: [&str; 2] = ["plasmalogin.service", "display-manager.service"];
const CRITICAL_UNITS: [&str; 5] = [
    "dbus-broker.service",
    "dbus.service",
    "display-manager.service",
    "plasmalogin.service",
    "NetworkManager.service",
];

#[derive(Debug, Clone)]
pub struct RuntimeCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

fn run_text(args: &[&str], timeout: Duration) -> Option<(bool, String)> {
    let argv = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    let output = super::process::run_bounded(&argv, timeout).ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Some((output.status.success(), text))
}

fn systemd_booted() -> bool {
    Path::new("/run/systemd/system").is_dir()
}

fn unit_active(unit: &str) -> bool {
    run_text(&["systemctl", "is-active", unit], Duration::from_secs(10))
        .is_some_and(|(ok, output)| ok && output.trim() == "active")
}

fn default_target() -> String {
    run_text(&["systemctl", "get-default"], Duration::from_secs(10))
        .filter(|(ok, _)| *ok)
        .map(|(_, output)| output.trim().to_string())
        .unwrap_or_default()
}

fn failed_units() -> Vec<String> {
    let Some((ok, output)) = run_text(
        &[
            "systemctl",
            "list-units",
            "--state=failed",
            "--no-legend",
            "--plain",
            "--no-pager",
        ],
        Duration::from_secs(15),
    ) else {
        return Vec::new();
    };
    if !ok {
        return Vec::new();
    }
    output
        .lines()
        .filter_map(|line| line.split_whitespace().next().map(str::to_string))
        .collect()
}

fn drm_devices() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir("/dev/dri") else {
        return Vec::new();
    };
    let mut devices: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            name.starts_with("card").then_some(name)
        })
        .collect();
    devices.sort();
    devices
}

fn software_compose_rescue() -> bool {
    let cmdline = std::fs::read_to_string("/proc/cmdline").unwrap_or_default();
    cmdline
        .split_whitespace()
        .any(|token| token == "nomodeset" || token == "kyth.live")
}

fn display_manager_active<F>(active: &F) -> bool
where
    F: Fn(&str) -> bool,
{
    DISPLAY_MANAGER_UNITS.iter().any(|unit| active(unit))
}

fn wait_until<F>(mut predicate: F, deadline: Duration, interval: Duration) -> bool
where
    F: FnMut() -> bool,
{
    let start = Instant::now();
    loop {
        if predicate() {
            return true;
        }
        if start.elapsed() >= deadline {
            return false;
        }
        std::thread::sleep(interval);
    }
}

/// Testable implementation.  Keeping the state readers injectable gives the
/// unit tests the same late-readiness coverage as the Python source without
/// waiting five minutes on a CI host.
pub fn runtime_checks_with<Booted, Active, Target, Failed, Devices, Rescue>(
    systemd_is_booted: Booted,
    unit_is_active: Active,
    default_target: Target,
    failed_units: Failed,
    drm_devices: Devices,
    software_rescue: Rescue,
    deadline: Duration,
    interval: Duration,
) -> Vec<RuntimeCheck>
where
    Booted: Fn() -> bool,
    Active: Fn(&str) -> bool,
    Target: Fn() -> String,
    Failed: Fn() -> Vec<String>,
    Devices: Fn() -> Vec<String>,
    Rescue: Fn() -> bool,
{
    if !systemd_is_booted() {
        return vec![RuntimeCheck {
            name: "Runtime assertions".to_string(),
            passed: true,
            detail: "skipped: not a systemd boot".to_string(),
        }];
    }

    let target = default_target();
    let expects_graphical = target == GRAPHICAL_TARGET;
    let rescue = software_rescue();
    let ready = || {
        (!expects_graphical || display_manager_active(&unit_is_active))
            && (!expects_graphical || rescue || !drm_devices().is_empty())
    };
    let _ = wait_until(ready, deadline, interval);

    let mut checks = Vec::new();
    if expects_graphical {
        let reached = display_manager_active(&unit_is_active);
        checks.push(RuntimeCheck {
            name: "Graphical session".to_string(),
            passed: reached,
            detail: if reached {
                "display manager active".to_string()
            } else {
                format!(
                    "display manager not reached within {:.0}s",
                    deadline.as_secs_f64()
                )
            },
        });
    } else {
        checks.push(RuntimeCheck {
            name: "Graphical session".to_string(),
            passed: true,
            detail: format!(
                "skipped: default target is {}",
                if target.is_empty() {
                    "unknown"
                } else {
                    &target
                }
            ),
        });
    }

    let devices = drm_devices();
    if expects_graphical {
        if rescue && devices.is_empty() {
            let dm_active = display_manager_active(&unit_is_active);
            checks.push(RuntimeCheck {
                name: "Display device".to_string(),
                passed: dm_active,
                detail: if dm_active {
                    "software-compose rescue (no DRM card; display manager active) — degraded, intentional nomodeset/live".to_string()
                } else {
                    "software-compose rescue but display manager inactive — check cmdline".to_string()
                },
            });
        } else {
            checks.push(RuntimeCheck {
                name: "Display device".to_string(),
                passed: !devices.is_empty(),
                detail: if devices.is_empty() {
                    "no DRM card device — GPU driver did not load".to_string()
                } else {
                    format!("/dev/dri: {}", devices.join(", "))
                },
            });
        }
    } else {
        checks.push(RuntimeCheck {
            name: "Display device".to_string(),
            passed: true,
            detail: format!(
                "skipped: default target is {}",
                if target.is_empty() {
                    "unknown"
                } else {
                    &target
                }
            ),
        });
    }

    let failed: Vec<String> = failed_units()
        .into_iter()
        .filter(|unit| CRITICAL_UNITS.contains(&unit.as_str()))
        .collect();
    checks.push(RuntimeCheck {
        name: "Critical units".to_string(),
        passed: failed.is_empty(),
        detail: if failed.is_empty() {
            "no critical unit failed".to_string()
        } else {
            format!("failed: {}", failed.join(", "))
        },
    });
    checks
}

pub fn boot_runtime_checks() -> Vec<RuntimeCheck> {
    boot_runtime_checks_with_deadline(DEFAULT_DEADLINE, DEFAULT_INTERVAL)
}

/// Run the same native runtime assertions with a caller-selected budget.
/// Interactive Hub health reporting uses this bounded variant so a missing
/// boot-health record cannot turn a read-only page into a five-minute wait.
pub fn boot_runtime_checks_with_deadline(
    deadline: Duration,
    interval: Duration,
) -> Vec<RuntimeCheck> {
    runtime_checks_with(
        systemd_booted,
        unit_active,
        default_target,
        failed_units,
        drm_devices,
        software_compose_rescue,
        deadline,
        interval,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_systemd_is_skipped() {
        let checks = runtime_checks_with(
            || false,
            |_| false,
            || "graphical.target".to_string(),
            Vec::new,
            Vec::new,
            || false,
            Duration::ZERO,
            Duration::ZERO,
        );
        assert_eq!(checks.len(), 1);
        assert!(checks[0].passed);
    }

    #[test]
    fn headless_boot_does_not_require_display() {
        let checks = runtime_checks_with(
            || true,
            |_| false,
            || "multi-user.target".to_string(),
            Vec::new,
            Vec::new,
            || false,
            Duration::ZERO,
            Duration::ZERO,
        );
        assert!(checks.iter().all(|check| check.passed));
    }

    #[test]
    fn failed_critical_unit_is_reported() {
        let checks = runtime_checks_with(
            || true,
            |unit| unit == "plasmalogin.service",
            || "graphical.target".to_string(),
            || {
                vec![
                    "plasmalogin.service".to_string(),
                    "cups.service".to_string(),
                ]
            },
            || vec!["card0".to_string()],
            || false,
            Duration::ZERO,
            Duration::ZERO,
        );
        let critical = checks
            .iter()
            .find(|check| check.name == "Critical units")
            .unwrap();
        assert!(!critical.passed);
        assert!(critical.detail.contains("plasmalogin.service"));
    }
}
