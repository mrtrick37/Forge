//! Pure parsers and policy helpers for post-update runtime diagnostics.
//!
//! Command collection stays with the existing probe services. Keeping the
//! interpretation here lets Rust UIs and CLIs consume the same evidence
//! without duplicating subprocess or presentation logic.

use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverCheck {
    pub label: String,
    pub passed: bool,
    pub detail: String,
}

pub fn is_live_image_text(cmdline: &str) -> bool {
    cmdline.split_whitespace().any(|token| token == "kyth.live")
}

pub fn deployment_id_from_ostree(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        if !line.trim_start().starts_with('*') {
            return None;
        }
        let parts: Vec<_> = line.split_whitespace().collect();
        (parts.len() >= 3).then(|| format!("{}.{}", parts[1], parts[2]))
    })
}

/// Select the deployment identity using the same ostree → bootc → kernel
/// fallback used by the Python diagnostic collector.
pub fn deployment_id(
    ostree_output: Option<&str>,
    bootc_json: Option<&str>,
    kernel_release: &str,
) -> String {
    ostree_output
        .and_then(deployment_id_from_ostree)
        .or_else(|| bootc_json.and_then(bootc_digest_from_json))
        .unwrap_or_else(|| kernel_release.to_string())
}

pub fn bootc_digest_from_json(output: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(output).ok()?;
    value
        .get("status")?
        .get("booted")?
        .get("image")?
        .get("imageDigest")?
        .as_str()
        .filter(|digest| !digest.is_empty())
        .map(str::to_string)
}

pub fn driver_check(
    gpu_line: &str,
    modules: impl IntoIterator<Item = impl AsRef<str>>,
) -> DriverCheck {
    let lower = gpu_line.to_ascii_lowercase();
    let modules: HashSet<String> = modules
        .into_iter()
        .map(|module| module.as_ref().to_ascii_lowercase())
        .collect();
    let expectations = [
        ("nvidia", &["nvidia"][..], "NVIDIA"),
        ("amd", &["amdgpu"][..], "AMD"),
        ("ati", &["amdgpu"][..], "AMD"),
        ("intel", &["i915", "xe"][..], "Intel"),
    ];
    for (marker, expected, label) in expectations {
        if !lower.contains(marker) {
            continue;
        }
        let passed = expected.iter().any(|module| modules.contains(*module));
        return DriverCheck {
            label: label.to_string(),
            passed,
            detail: if passed {
                format!("{} loaded", expected.join("/"))
            } else {
                format!(
                    "{label} GPU detected but {} is not loaded",
                    expected.join("/")
                )
            },
        };
    }
    DriverCheck {
        label: "GPU drivers".to_string(),
        passed: true,
        detail: "Generic display controller active".to_string(),
    }
}

pub fn gpu_detected_check(lspci_available: bool, gpu_line: Option<&str>) -> DriverCheck {
    if !lspci_available {
        return DriverCheck {
            label: "GPU detected".into(),
            passed: false,
            detail: "lspci command not found".into(),
        };
    }
    match gpu_line.filter(|line| !line.trim().is_empty()) {
        Some(line) => DriverCheck {
            label: "GPU detected".into(),
            passed: true,
            detail: line.trim().into(),
        },
        None => DriverCheck {
            label: "GPU detected".into(),
            passed: false,
            detail: "no GPU found via lspci".into(),
        },
    }
}

pub fn vulkan_check(
    available: bool,
    timed_out: bool,
    succeeded: bool,
    warning: &str,
) -> DriverCheck {
    let detail = if !available {
        "vulkaninfo unavailable".to_string()
    } else if timed_out {
        format!("{warning} (timeout)")
    } else if succeeded {
        "responding".to_string()
    } else {
        warning.to_string()
    };
    DriverCheck {
        label: "Vulkan".into(),
        passed: available && !timed_out && succeeded,
        detail,
    }
}

/// Return deployment rollback failures and warnings without probing the host.
pub fn deployment_rollback(
    ostree_available: bool,
    ostree_output: Option<&str>,
    bootc_available: bool,
    bootc_succeeded: Option<bool>,
) -> (Vec<String>, Vec<String>) {
    let mut failures = Vec::new();
    let mut warnings = Vec::new();
    if ostree_available {
        match ostree_output {
            None => warnings.push("Failed to query ostree admin status.".into()),
            Some(output) => {
                let count = output.lines().filter(|line| deployment_line(line)).count();
                if count < 2 {
                    warnings.push("Rollback deployment not visible yet.".into());
                }
            }
        }
    } else if bootc_available {
        if bootc_succeeded != Some(true) {
            warnings.push("bootc status needs root; rollback was not checked here.".into());
        }
    } else {
        failures.push("No deployment tool found.".into());
    }
    (failures, warnings)
}

fn deployment_line(line: &str) -> bool {
    let mut fields = line.split_whitespace();
    let first = fields.next();
    let (name, _version) = match first {
        Some("*") => (fields.next(), fields.next()),
        Some(value) => (Some(value), fields.next()),
        None => (None, None),
    };
    name.is_some_and(|value| {
        value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    }) && _version.is_some()
}

pub fn login_session_check(loginctl_available: bool, command_succeeded: bool) -> DriverCheck {
    DriverCheck {
        label: "Login session".into(),
        passed: loginctl_available && command_succeeded,
        detail: if !loginctl_available {
            "loginctl command not found".into()
        } else if command_succeeded {
            "logind can see this session".into()
        } else {
            "session not visible through loginctl".into()
        },
    }
}

pub fn service_label(unit: &str) -> &str {
    match unit {
        "pipewire.service" => "PipeWire",
        "wireplumber.service" => "WirePlumber",
        "bluetooth.service" => "Bluetooth service",
        other => other,
    }
}

pub fn service_detail(active: bool, result: Option<&str>) -> (&'static str, String) {
    if active {
        ("healthy", "active".to_string())
    } else if result.is_some_and(|value| value.trim() == "success") {
        ("healthy", "completed successfully".to_string())
    } else {
        ("warning", "not active".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_live_image_and_deployments() {
        assert!(is_live_image_text("quiet splash kyth.live"));
        assert!(!is_live_image_text("quiet splash"));
        assert_eq!(
            deployment_id_from_ostree("  kyth 44\n* kyth 44.2\n"),
            Some("kyth.44.2".into())
        );
        assert_eq!(
            bootc_digest_from_json(
                r#"{"status":{"booted":{"image":{"imageDigest":"sha256:abc"}}}}"#
            ),
            Some("sha256:abc".into())
        );
    }

    #[test]
    fn evaluates_expected_gpu_modules() {
        let amd = driver_check("01:00.0 VGA compatible controller: AMD", ["amdgpu"]);
        assert!(amd.passed);
        let nvidia = driver_check("01:00.0 3D controller: NVIDIA", ["nouveau"]);
        assert!(!nvidia.passed);
        assert_eq!(
            service_detail(false, Some("success")),
            ("healthy", "completed successfully".into())
        );
    }

    #[test]
    fn projects_remaining_runtime_statuses() {
        assert_eq!(
            deployment_id(
                None,
                Some(r#"{"status":{"booted":{"image":{"imageDigest":"sha256:x"}}}}"#),
                "6.1"
            ),
            "sha256:x"
        );
        assert_eq!(deployment_id(None, Some("{}"), "6.1"), "6.1");
        assert!(gpu_detected_check(true, Some("AMD Radeon")).passed);
        assert_eq!(
            vulkan_check(true, true, false, "Vulkan probe failed").detail,
            "Vulkan probe failed (timeout)"
        );
        assert_eq!(
            deployment_rollback(true, Some("* kyth 44\n"), false, None).1,
            vec!["Rollback deployment not visible yet."]
        );
        assert!(login_session_check(true, true).passed);
    }
}
