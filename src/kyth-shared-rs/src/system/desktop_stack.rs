//! Port of `kyth_shared.system.desktop_stack` — display stack checks.
//!
//! These are diagnostic checks, not rollback triggers.  A greeter-only boot
//! has no user bus yet, so user-scoped portal and PipeWire units are advisory.

use serde::Serialize;
use std::env;
use std::path::Path;
use std::time::Duration;

const REQUIRED_PORTAL_PATHS: [&str; 2] = [
    "/usr/libexec/xdg-desktop-portal",
    "/usr/libexec/xdg-desktop-portal-kde",
];
const OPTIONAL_PORTAL_BINS: [&str; 2] = ["xdg-desktop-portal", "xdg-desktop-portal-kde"];
const PORTAL_UNITS: [&str; 1] = ["xdg-desktop-portal.service"];
const KDE_PORTAL_UNITS: [&str; 2] = [
    "plasma-xdg-desktop-portal-kde.service",
    "xdg-desktop-portal-kde.service",
];
const PIPEWIRE_UNITS: [&str; 2] = ["pipewire.service", "wireplumber.service"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StackCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
    pub advisory: bool,
}

fn run_text(args: &[&str], timeout: Duration) -> Option<(bool, String)> {
    let argv = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    let output = super::process::run_bounded(&argv, timeout).ok()?;
    Some((
        output.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    ))
}

fn user_unit_active(unit: &str) -> bool {
    run_text(
        &["systemctl", "--user", "is-active", unit],
        Duration::from_secs(8),
    )
    .is_some_and(|(ok, output)| ok && output.trim() == "active")
}

fn command_exists(name: &str) -> bool {
    let path = env::var_os("PATH").unwrap_or_default();
    env::split_paths(&path).any(|dir| dir.join(name).is_file())
}

fn portal_binaries() -> (bool, String) {
    let present: Vec<&str> = REQUIRED_PORTAL_PATHS
        .iter()
        .copied()
        .filter(|path| Path::new(path).exists())
        .collect();
    if !present.is_empty() {
        return (true, present.join(", "));
    }
    let found: Vec<&str> = OPTIONAL_PORTAL_BINS
        .iter()
        .copied()
        .filter(|name| command_exists(name))
        .collect();
    if !found.is_empty() {
        return (true, format!("bins: {}", found.join(", ")));
    }
    if Path::new("/usr/lib64/libexec/xdg-desktop-portal-kde").exists()
        || Path::new("/usr/lib/xdg-desktop-portal-kde").exists()
    {
        return (true, "kde portal libexec present".to_string());
    }
    (
        false,
        "xdg-desktop-portal / kde backend not found on image".to_string(),
    )
}

fn any_active<F>(units: &[&str], active: &F) -> bool
where
    F: Fn(&str) -> bool,
{
    units.iter().any(|unit| active(unit))
}

/// Return structured checks so the Hub can distinguish failures from
/// advisory session warnings.  This keeps the Rust bridge aligned with the
/// Python `StackCheck` contract instead of returning only active unit names.
pub fn desktop_stack_checks() -> Vec<StackCheck> {
    let (portal_ok, portal_detail) = portal_binaries();
    let mut checks = vec![StackCheck {
        name: "Portal packages".to_string(),
        passed: portal_ok,
        detail: if portal_ok {
            portal_detail
        } else {
            format!("{} — install xdg-desktop-portal-kde", portal_detail)
        },
        advisory: false,
    }];

    if env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        checks.push(StackCheck {
            name: "User desktop session".to_string(),
            passed: true,
            detail: "skipped: no user session bus (greeter / SSH / image build)".to_string(),
            advisory: true,
        });
        return checks;
    }

    let session_type = env::var("XDG_SESSION_TYPE")
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    match session_type.as_str() {
        "wayland" => {
            let display = env::var("WAYLAND_DISPLAY").unwrap_or_default();
            checks.push(StackCheck {
                name: "Wayland display".to_string(),
                passed: !display.trim().is_empty(),
                detail: if display.trim().is_empty() {
                    "Wayland session without WAYLAND_DISPLAY".to_string()
                } else {
                    format!("WAYLAND_DISPLAY={}", display.trim())
                },
                advisory: false,
            });
        }
        "x11" => checks.push(StackCheck {
            name: "Wayland display".to_string(),
            passed: false,
            detail: "X11 session — KythOS ships Plasma Wayland only. Ctrl+Alt+F3, then journalctl -u plasmalogin -b".to_string(),
            advisory: false,
        }),
        _ => checks.push(StackCheck {
            name: "Wayland display".to_string(),
            passed: true,
            detail: format!("session type {} — Wayland checks skipped", if session_type.is_empty() { "unknown" } else { &session_type }),
            advisory: true,
        }),
    }

    let portal = |unit: &str| user_unit_active(unit);
    for (name, units, ok_detail, fail_detail) in [
        (
            "xdg-desktop-portal",
            PORTAL_UNITS.as_slice(),
            "xdg-desktop-portal.service active",
            "xdg-desktop-portal.service not active — screen share / Flatpak dialogs may fail",
        ),
        (
            "KDE portal backend",
            KDE_PORTAL_UNITS.as_slice(),
            "plasma/xdg-desktop-portal-kde active",
            "KDE portal backend not active — restart: systemctl --user restart xdg-desktop-portal-kde",
        ),
    ] {
        let passed = any_active(units, &portal);
        checks.push(StackCheck {
            name: name.to_string(),
            passed,
            detail: if passed { ok_detail.to_string() } else { fail_detail.to_string() },
            advisory: true,
        });
    }

    for unit in PIPEWIRE_UNITS {
        let passed = user_unit_active(unit);
        let name = if unit.starts_with("pipewire") {
            "PipeWire"
        } else {
            "WirePlumber"
        };
        checks.push(StackCheck {
            name: name.to_string(),
            passed,
            detail: if passed {
                format!("{} active", unit)
            } else {
                format!("{} not active — audio/capture degraded", unit)
            },
            advisory: true,
        });
    }
    checks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_probe_captures_output_and_status() {
        let result = run_text(&["sh", "-c", "printf active"], Duration::from_secs(1)).unwrap();
        assert_eq!(result, (true, "active".into()));
    }

    #[test]
    fn check_shape_is_serializable() {
        let check = StackCheck {
            name: "test".to_string(),
            passed: true,
            detail: "ok".to_string(),
            advisory: true,
        };
        let json = serde_json::to_value(check).unwrap();
        assert_eq!(json["advisory"], true);
    }
}
