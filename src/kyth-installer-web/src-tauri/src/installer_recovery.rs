//! Native transaction-status classification and support export for Rescue mode.
//!
//! The daemon owns durable-state reads and read-only diagnostics. The typed
//! root-only helper owns support-file export. The Python implementation is
//! retained only as a source compatibility fixture while packaging is retired.

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

const MAX_RECOVERY_PATH_BYTES: usize = 4096;
const MAX_SUPPORT_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SUPPORT_EXPORT_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct RecoveryGuidance {
    pub status: String,
    pub severity: String,
    pub message: String,
    pub bootable: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct RecoveryExportInput {
    pub usb_mount: String,
    pub log_path: String,
    pub transaction_path: String,
    pub failure_summary_path: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RecoveryExportResponse {
    pub ok: bool,
    pub dest: String,
    pub copied: Vec<String>,
}

const GUIDANCE: &[(&str, &str, &str, bool)] = &[
    ("", "unknown", "No install transaction recorded. Do not assume the disk is bootable.", false),
    ("started", "incomplete", "Install started but storage did not finish. Stay in this live session.", false),
    ("prepared", "incomplete", "Install prepared a plan but storage did not finish. Stay in this live session.", false),
    ("storage_complete", "unbootable", "The image is on disk but the installed system is not configured yet. Continue or rescue from this live session — do not reboot into the target.", false),
    ("image_installed", "unbootable", "Legacy journal: image written, configure unknown. Treat the target as not bootable until configure_complete.", false),
    ("configure_started", "unbootable", "Configuring the installed system was interrupted. Continue from this live session — the target is not bootable yet.", false),
    ("configure_complete", "almost", "The installed system is configured. Secure Boot enrollment may still be pending — check MOK staging before reboot.", false),
    ("secure_boot_staged", "ready", "Secure Boot enrollment is staged. Reboot and enroll the MOK if the firmware prompts.", true),
    ("complete", "ready", "Install finished. The target should be bootable.", true),
    ("failed", "failed", "Install failed. Use the log tail and transaction details below.", false),
];

pub(crate) fn rescue_guidance(status: Option<&str>) -> RecoveryGuidance {
    let status = status.unwrap_or_default();
    if let Some((_, severity, message, bootable)) = GUIDANCE.iter().find(|entry| entry.0 == status)
    {
        return RecoveryGuidance {
            status: status.to_string(),
            severity: (*severity).to_string(),
            message: (*message).to_string(),
            bootable: *bootable,
        };
    }
    RecoveryGuidance {
        status: status.to_string(),
        severity: "unknown".to_string(),
        message: format!(
            "Unrecognized transaction status {status:?}. Do not assume the disk is bootable."
        ),
        bootable: false,
    }
}

/// Extract the small booted/staged summary used by the Rescue UI from the
/// version-variable bootc status JSON contract.
pub(crate) fn bootc_status_summary(raw: &str) -> serde_json::Value {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return serde_json::json!({"booted": "", "staged": ""});
    };
    let status = value.get("status").unwrap_or(&value);
    let mut booted = status
        .get("booted")
        .or_else(|| status.get("bootedImage"))
        .or_else(|| value.get("booted"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut staged = status
        .get("staged")
        .or_else(|| status.get("stagedImage"))
        .or_else(|| value.get("staged"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    if let Some(deployments) = value
        .get("deployments")
        .and_then(serde_json::Value::as_array)
    {
        for deployment in deployments {
            let Some(deployment) = deployment.as_object() else {
                continue;
            };
            let image = deployment
                .get("image")
                .or_else(|| deployment.get("id"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if booted.is_empty()
                && deployment
                    .get("booted")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
            {
                booted = image.to_string();
            }
            if staged.is_empty()
                && deployment
                    .get("staged")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
            {
                staged = image.to_string();
            }
        }
    }
    serde_json::json!({
        "booted": booted.chars().take(256).collect::<String>(),
        "staged": staged.chars().take(256).collect::<String>(),
    })
}

fn safe_absolute_path(raw: &str, label: &str) -> Result<String, String> {
    let value = raw.trim();
    if value.is_empty()
        || value.len() > MAX_RECOVERY_PATH_BYTES
        || !value.starts_with('/')
        || value.contains("..")
        || value.contains("//")
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'+' | b':' | b'-')
        })
    {
        return Err(format!("{label} must be an absolute safe path."));
    }
    Ok(value.to_string())
}

fn copy_support_file(source: &Path, destination: &Path) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("could not inspect recovery file: {error}")),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Ok(false);
    }
    if metadata.len() > MAX_SUPPORT_FILE_BYTES {
        return Err(format!(
            "recovery file {} is larger than the supported export limit",
            source.display()
        ));
    }

    let mut input = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(source)
        .map_err(|error| format!("could not open recovery file: {error}"))?;
    let mut output = OpenOptions::new();
    output
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o644);
    let mut output = output
        .open(destination)
        .map_err(|error| format!("could not create recovery export: {error}"))?;
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = io::Read::read(&mut input, &mut buffer)
            .map_err(|error| format!("could not read recovery file: {error}"))?;
        if read == 0 {
            break;
        }
        copied = copied.saturating_add(read as u64);
        if copied > MAX_SUPPORT_FILE_BYTES {
            return Err(format!(
                "recovery file {} grew beyond the supported export limit",
                source.display()
            ));
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| format!("could not copy recovery file: {error}"))?;
    }
    output
        .flush()
        .map_err(|error| format!("could not flush recovery export: {error}"))?;
    output
        .sync_all()
        .map_err(|error| format!("could not sync recovery export: {error}"))?;
    fs::set_permissions(destination, fs::Permissions::from_mode(0o644))
        .map_err(|error| format!("could not secure recovery export: {error}"))?;
    Ok(true)
}

pub(crate) fn export_logs(input: RecoveryExportInput) -> Result<RecoveryExportResponse, String> {
    let mount = safe_absolute_path(&input.usb_mount, "USB mount")?;
    let mount = fs::canonicalize(&mount)
        .map_err(|error| format!("could not resolve USB mount: {error}"))?;
    if !mount.is_dir() {
        return Err("USB mount is not a directory".to_string());
    }

    let destination = mount.join("kyth-installer-logs");
    if let Ok(metadata) = fs::symlink_metadata(&destination) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("recovery export directory is not a safe directory".to_string());
        }
    }
    fs::create_dir_all(&destination)
        .map_err(|error| format!("could not create recovery export directory: {error}"))?;

    let sources = [
        ("log", safe_absolute_path(&input.log_path, "log path")?),
        (
            "transaction.json",
            safe_absolute_path(&input.transaction_path, "transaction path")?,
        ),
        (
            "failure.json",
            safe_absolute_path(&input.failure_summary_path, "failure summary path")?,
        ),
    ];
    let mut copied = Vec::new();
    let mut total_bytes = 0_u64;
    for (name, source) in sources {
        let target = destination.join(name);
        if let Ok(metadata) = fs::symlink_metadata(&target) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(format!(
                    "recovery export target {} is not a safe file",
                    target.display()
                ));
            }
        }
        if let Ok(metadata) = fs::symlink_metadata(Path::new(&source)) {
            total_bytes = total_bytes.saturating_add(metadata.len());
            if total_bytes > MAX_SUPPORT_EXPORT_BYTES {
                return Err("recovery export exceeds the aggregate size limit".to_string());
            }
        }
        if copy_support_file(Path::new(&source), &target)? {
            copied.push(name.to_string());
        }
    }
    if copied.is_empty() {
        return Err("No installer logs found to copy.".to_string());
    }
    Ok(RecoveryExportResponse {
        ok: true,
        dest: destination.to_string_lossy().into_owned(),
        copied,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn shared_recovery_fixture_matches_all_durable_statuses() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("../testdata/recovery_cases.json"))
                .expect("recovery parity fixture must be valid JSON");
        for case in cases {
            let status = case["status"].as_str().expect("status is a string");
            let guidance = rescue_guidance(Some(status));
            assert_eq!(
                guidance.severity,
                case["severity"].as_str().unwrap(),
                "{status}"
            );
            assert_eq!(
                guidance.bootable,
                case["bootable"].as_bool().unwrap(),
                "{status}"
            );
            assert_eq!(
                guidance.message,
                case["message"].as_str().unwrap(),
                "{status}"
            );
        }
        let unknown = rescue_guidance(Some("future_status"));
        assert!(!unknown.bootable);
        assert_eq!(unknown.severity, "unknown");
    }

    #[test]
    fn bootc_summary_handles_common_and_malformed_status_shapes() {
        let summary = bootc_status_summary(
            r#"{"deployments":[{"id":"old","booted":true},{"image":"new","staged":true}]}"#,
        );
        assert_eq!(summary["booted"], "old");
        assert_eq!(summary["staged"], "new");
        assert_eq!(
            bootc_status_summary("not-json"),
            serde_json::json!({"booted": "", "staged": ""})
        );
    }

    #[test]
    fn recovery_export_rejects_unsafe_mount() {
        assert!(export_logs(RecoveryExportInput {
            usb_mount: "/tmp/../etc".to_string(),
            log_path: "/run/kyth-installer/log".to_string(),
            transaction_path: "/run/kyth-installer/transaction.json".to_string(),
            failure_summary_path: "/run/kyth-installer/failure.json".to_string(),
        })
        .is_err());
    }

    #[test]
    fn recovery_export_skips_symlink_sources() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mount = directory.path().join("usb");
        let sources = directory.path().join("sources");
        fs::create_dir(&mount).expect("USB directory");
        fs::create_dir(&sources).expect("source directory");
        fs::write(sources.join("real-log"), "secret-safe-log").expect("source content");
        std::os::unix::fs::symlink(sources.join("real-log"), sources.join("log"))
            .expect("source symlink");
        assert!(export_logs(RecoveryExportInput {
            usb_mount: mount.to_string_lossy().into_owned(),
            log_path: sources.join("log").to_string_lossy().into_owned(),
            transaction_path: sources
                .join("transaction.json")
                .to_string_lossy()
                .into_owned(),
            failure_summary_path: sources.join("failure.json").to_string_lossy().into_owned(),
        })
        .is_err());
    }

    #[test]
    fn recovery_export_rejects_existing_symlink_target() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mount = directory.path().join("usb");
        let sources = directory.path().join("sources");
        fs::create_dir(&mount).expect("USB directory");
        fs::create_dir(&sources).expect("source directory");
        fs::write(sources.join("log"), "safe").expect("source content");
        fs::create_dir(mount.join("kyth-installer-logs")).expect("destination");
        std::os::unix::fs::symlink(sources.join("log"), mount.join("kyth-installer-logs/log"))
            .expect("destination symlink");
        assert!(export_logs(RecoveryExportInput {
            usb_mount: mount.to_string_lossy().into_owned(),
            log_path: sources.join("log").to_string_lossy().into_owned(),
            transaction_path: sources.join("missing").to_string_lossy().into_owned(),
            failure_summary_path: sources.join("missing2").to_string_lossy().into_owned(),
        })
        .is_err());
    }

    #[test]
    fn recovery_export_enforces_aggregate_limit() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mount = directory.path().join("usb");
        let sources = directory.path().join("sources");
        fs::create_dir(&mount).expect("USB directory");
        fs::create_dir(&sources).expect("source directory");
        let payload = vec![b'x'; (MAX_SUPPORT_EXPORT_BYTES / 2 + 1) as usize];
        for name in ["log", "transaction", "failure"] {
            fs::write(sources.join(name), &payload).expect("source content");
        }
        assert!(export_logs(RecoveryExportInput {
            usb_mount: mount.to_string_lossy().into_owned(),
            log_path: sources.join("log").to_string_lossy().into_owned(),
            transaction_path: sources.join("transaction").to_string_lossy().into_owned(),
            failure_summary_path: sources.join("failure").to_string_lossy().into_owned(),
        })
        .is_err());
    }
}
