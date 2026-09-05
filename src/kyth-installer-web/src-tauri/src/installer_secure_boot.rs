//! Pure Secure Boot/MOK decision model.
//!
//! No password, certificate contents, firmware access, or subprocess result
//! is handled here. The Python service performs those privileged operations
//! and feeds their bounded observations into the same state model.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MOKUTIL: &str = "/usr/bin/mokutil";
const CERTIFICATE: &str = "/usr/share/kyth/secureboot/kyth-secureboot.der";
const MAX_MOK_PASSWORD_BYTES: usize = 512;
const MOK_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SecureBootInput {
    #[serde(default = "default_kernel")]
    pub kernel: String,
    #[serde(default)]
    pub force_stage: bool,
    #[serde(default)]
    pub certificate_present: bool,
    #[serde(default)]
    pub mokutil_present: bool,
    #[serde(default = "default_unknown")]
    pub secure_boot: String,
    #[serde(default = "default_unknown")]
    pub enrolled: String,
    #[serde(default = "default_unknown")]
    pub pending: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct SecureBootPlan {
    pub state: String,
    pub action: String,
    pub requires_password: bool,
    pub requires_reboot_confirmation: bool,
    pub message: String,
    pub executor: &'static str,
}

/// The only secret-bearing input accepted by the privileged helper.
///
/// The password is read from the helper's stdin payload and is never copied
/// into a command-line argument, an event, or an error message. The helper
/// intentionally owns the certificate and mokutil paths so callers cannot
/// turn this operation into an arbitrary file or process bridge.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SecureBootStageInput {
    #[serde(default = "default_kernel")]
    pub kernel: String,
    #[serde(default)]
    pub force_stage: bool,
    #[serde(default)]
    pub password: String,
}

fn default_kernel() -> String {
    "fedora".to_string()
}
fn default_unknown() -> String {
    "unknown".to_string()
}

pub(crate) fn build_plan(input: SecureBootInput) -> Result<SecureBootPlan, String> {
    let kernel = input.kernel.trim().to_ascii_lowercase();
    let secure_boot = input.secure_boot.trim().to_ascii_lowercase();
    let enrolled = input.enrolled.trim().to_ascii_lowercase();
    let pending = input.pending.trim().to_ascii_lowercase();
    if !matches!(kernel.as_str(), "fedora" | "cachy") {
        return Err(format!("unsupported kernel flavor: {kernel}"));
    }
    let skipped = |message: &str| SecureBootPlan {
        state: "skipped".to_string(),
        action: "none".to_string(),
        requires_password: false,
        requires_reboot_confirmation: false,
        message: message.to_string(),
        executor: "kyth-installer-exec",
    };
    if kernel != "cachy" && !input.force_stage {
        return Ok(skipped(
            "standard KythOS kernel does not require custom MOK enrollment",
        ));
    }
    if !input.certificate_present {
        return Ok(skipped(
            "KythOS Secure Boot certificate is not present in the live image",
        ));
    }
    if !input.mokutil_present {
        return Ok(skipped("mokutil is not available in the live image"));
    }
    if secure_boot == "disabled" {
        return Ok(skipped(
            "Secure Boot is disabled; MOK enrollment is not required",
        ));
    }
    if secure_boot != "enabled" {
        return Ok(SecureBootPlan {
            state: "unknown".to_string(),
            action: "probe".to_string(),
            requires_password: false,
            requires_reboot_confirmation: false,
            message: "Secure Boot state must be checked by the privileged service".to_string(),
            executor: "kyth-installer-exec",
        });
    }
    if enrolled == "yes" {
        return Ok(SecureBootPlan {
            state: "enrolled".to_string(),
            action: "none".to_string(),
            requires_password: false,
            requires_reboot_confirmation: false,
            message: "KythOS Secure Boot key is already enrolled".to_string(),
            executor: "kyth-installer-exec",
        });
    }
    if pending == "yes" {
        return Ok(SecureBootPlan {
            state: "pending".to_string(),
            action: "none".to_string(),
            requires_password: false,
            requires_reboot_confirmation: true,
            message: "KythOS Secure Boot enrollment is pending confirmation on the next boot"
                .to_string(),
            executor: "kyth-installer-exec",
        });
    }
    Ok(SecureBootPlan {
        state: "ready".to_string(),
        action: "import-certificate".to_string(),
        requires_password: true,
        requires_reboot_confirmation: true,
        message: "The privileged service may stage KythOS MOK enrollment".to_string(),
        executor: "kyth-installer-exec",
    })
}

pub(crate) fn classify_import(exit_code: i32) -> &'static str {
    if exit_code == 0 {
        "staged"
    } else {
        "failed"
    }
}

fn command_text(args: &[&str]) -> Result<String, String> {
    let output = Command::new(MOKUTIL)
        .args(args)
        .output()
        .map_err(|error| format!("could not inspect Secure Boot state: {error}"))?;
    if !output.status.success() {
        return Err("Secure Boot probe failed".to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn contains_key(args: &[&str]) -> bool {
    command_text(args)
        .map(|output| output.contains("KythOS Secure Boot"))
        .unwrap_or(false)
}

fn secure_boot_state() -> &'static str {
    match command_text(&["--sb-state"]) {
        Ok(output) if output.contains("SecureBoot enabled") => "enabled",
        Ok(_) => "disabled",
        Err(_) => "unknown",
    }
}

fn stage_certificate(
    password: &str,
    cancel_requested: impl Fn() -> bool,
) -> Result<&'static str, String> {
    let mut child = Command::new(MOKUTIL)
        .args(["--import", CERTIFICATE, "--stdin-passwd"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("could not stage Secure Boot enrollment: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(error) = stdin.write_all(format!("{password}\n").as_bytes()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "could not provide Secure Boot enrollment input: {error}"
            ));
        }
    }

    let deadline = Instant::now() + MOK_TIMEOUT;
    loop {
        if cancel_requested() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(
                "Installation cancelled by user. Disk changes may have already started."
                    .to_string(),
            );
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(classify_import(status.code().unwrap_or(1))),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Secure Boot enrollment timed out".to_string());
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "could not wait for Secure Boot enrollment: {error}"
                ));
            }
        }
    }
}

/// Probe firmware state and, when required, stage the fixed KythOS MOK.
pub(crate) fn stage(input: SecureBootStageInput) -> Result<SecureBootPlan, String> {
    stage_with_cancellation(input, || false)
}

/// Probe firmware state and stage the fixed KythOS MOK while honoring job
/// cancellation during the bounded `mokutil` wait.
pub(crate) fn stage_with_cancellation(
    input: SecureBootStageInput,
    cancel_requested: impl Fn() -> bool,
) -> Result<SecureBootPlan, String> {
    if cancel_requested() {
        return Err(
            "Installation cancelled by user. Disk changes may have already started.".to_string(),
        );
    }
    if input.password.len() > MAX_MOK_PASSWORD_BYTES || input.password.contains(['\0', '\n', '\r'])
    {
        return Err("Secure Boot password is empty or contains unsupported characters".to_string());
    }
    let certificate_present = Path::new(CERTIFICATE).is_file();
    let mokutil_present = Path::new(MOKUTIL).is_file();
    let state = secure_boot_state();
    let enrolled = if mokutil_present && contains_key(&["--list-enrolled"]) {
        "yes"
    } else {
        "no"
    };
    let pending = if mokutil_present && contains_key(&["--list-new"]) {
        "yes"
    } else {
        "no"
    };
    let plan = build_plan(SecureBootInput {
        kernel: input.kernel,
        force_stage: input.force_stage,
        certificate_present,
        mokutil_present,
        secure_boot: state.to_string(),
        enrolled: enrolled.to_string(),
        pending: pending.to_string(),
    })?;
    if plan.action != "import-certificate" {
        return Ok(plan);
    }
    match stage_certificate(&input.password, cancel_requested)? {
        "staged" => Ok(SecureBootPlan {
            state: "staged".to_string(),
            action: "import-certificate".to_string(),
            requires_password: false,
            requires_reboot_confirmation: true,
            message: "KythOS Secure Boot enrollment is staged for confirmation on the next boot"
                .to_string(),
            executor: "kyth-installer-exec",
        }),
        _ => Ok(SecureBootPlan {
            state: "failed".to_string(),
            action: "import-certificate".to_string(),
            requires_password: false,
            requires_reboot_confirmation: false,
            message: "KythOS Secure Boot enrollment could not be staged".to_string(),
            executor: "kyth-installer-exec",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cachy() -> SecureBootInput {
        SecureBootInput {
            kernel: "cachy".to_string(),
            force_stage: false,
            certificate_present: true,
            mokutil_present: true,
            secure_boot: "enabled".to_string(),
            enrolled: "no".to_string(),
            pending: "no".to_string(),
        }
    }

    #[test]
    fn plans_import_without_handling_the_password() {
        let plan = build_plan(cachy()).expect("MOK plan should validate");
        assert_eq!(plan.action, "import-certificate");
        assert!(plan.requires_password);
        assert!(plan.requires_reboot_confirmation);
        assert!(!plan.message.contains("password"));
    }

    #[test]
    fn classifies_existing_states_and_non_custom_kernel() {
        assert_eq!(
            build_plan(SecureBootInput {
                kernel: "fedora".to_string(),
                ..cachy()
            })
            .unwrap()
            .state,
            "skipped"
        );
        assert_eq!(
            build_plan(SecureBootInput {
                enrolled: "yes".to_string(),
                ..cachy()
            })
            .unwrap()
            .state,
            "enrolled"
        );
        assert_eq!(
            build_plan(SecureBootInput {
                pending: "yes".to_string(),
                ..cachy()
            })
            .unwrap()
            .state,
            "pending"
        );
        assert_eq!(classify_import(0), "staged");
        assert_eq!(classify_import(1), "failed");
    }

    #[test]
    fn matches_shared_decision_fixture() {
        #[derive(Deserialize)]
        struct Case {
            input: SecureBootInput,
            expected: Expected,
        }
        #[derive(Deserialize)]
        struct Expected {
            state: String,
            action: String,
        }

        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../testdata/secure_boot_cases.json"))
                .expect("secure boot fixture should be valid");
        for case in cases {
            let plan = build_plan(case.input).expect("fixture input should validate");
            assert_eq!(plan.state, case.expected.state);
            assert_eq!(plan.action, case.expected.action);
        }
    }

    #[test]
    fn cancellation_is_checked_before_firmware_access() {
        let error = stage_with_cancellation(
            SecureBootStageInput {
                kernel: "cachy".into(),
                force_stage: true,
                password: "secret".into(),
            },
            || true,
        )
        .expect_err("cancelled Secure Boot work must not probe firmware");
        assert!(error.contains("cancelled"));
        assert!(!error.contains("secret"));
    }
}
