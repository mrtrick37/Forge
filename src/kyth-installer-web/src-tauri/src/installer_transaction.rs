//! Transaction-state schema, decoder, and durable writer.
//!
//! The compatibility service supplies only support-safe state. The native
//! helper owns the atomic replace and fsync boundary when installed; Python
//! retains a compatibility writer for environments without the helper.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::installer_recovery::{rescue_guidance, RecoveryGuidance};

fn default_schema_version() -> u32 {
    1
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct TransactionSource {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub digest: String,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub target_ref: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct TransactionState {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub transaction_id: String,
    #[serde(default)]
    pub job_id: Option<u64>,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub lifecycle: String,
    #[serde(default)]
    pub install_mode: String,
    #[serde(default)]
    pub disk: String,
    #[serde(default)]
    pub target_partition: String,
    #[serde(default)]
    pub source: TransactionSource,
    #[serde(default)]
    pub checks: Vec<Value>,
    #[serde(default)]
    pub partition_steps: Vec<Value>,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub recovery_required: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct DecodedTransaction {
    pub state: TransactionState,
    pub guidance: RecoveryGuidance,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TransactionWriteInput {
    pub path: String,
    pub state: TransactionState,
}

fn safe_transaction_path(raw: &str) -> Result<PathBuf, String> {
    let path = Path::new(raw.trim());
    if !path.is_absolute()
        || path.as_os_str().len() > 4096
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("transaction path must be an absolute safe path".to_string());
    }
    Ok(path.to_path_buf())
}

fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .map_err(|error| format!("could not open transaction directory: {error}"))?
        .sync_all()
        .map_err(|error| format!("could not sync transaction directory: {error}"))
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "transaction path has no parent directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create transaction directory: {error}"))?;
    let metadata = fs::symlink_metadata(parent)
        .map_err(|error| format!("could not inspect transaction directory: {error}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("transaction directory must be a real directory".to_string());
    }
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("could not secure transaction directory: {error}"))?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("transaction path must be a regular file".to_string());
        }
    }

    let temporary = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("json")
    ));
    let mut file = OpenOptions::new();
    file.write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW);
    let mut file = file
        .open(&temporary)
        .map_err(|error| format!("could not open temporary transaction state: {error}"))?;
    serde_json::to_writer_pretty(&mut file, value)
        .map_err(|error| format!("could not encode transaction state: {error}"))?;
    file.write_all(b"\n")
        .map_err(|error| format!("could not finish transaction state: {error}"))?;
    file.flush()
        .map_err(|error| format!("could not flush transaction state: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("could not sync transaction state: {error}"))?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("could not secure transaction state: {error}"))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("could not replace transaction state: {error}"))?;
    sync_directory(parent)
}

pub(crate) fn write_request(input: TransactionWriteInput) -> Result<(), String> {
    let path = safe_transaction_path(&input.path)?;
    let value = serde_json::to_value(input.state)
        .map_err(|error| format!("could not encode transaction state: {error}"))?;
    write_json(&path, &value)
}

/// Persist a support-safe failure marker using the same durable atomic writer
/// as the transaction itself. The failure file contains no credentials.
pub(crate) fn write_failure_summary(
    path: &str,
    state: &TransactionState,
    message: &str,
) -> Result<(), String> {
    let path = safe_transaction_path(path)?;
    let mut value = serde_json::to_value(state)
        .map_err(|error| format!("could not encode failure summary: {error}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "failure summary state is not an object".to_string())?;
    object.insert("status".to_string(), Value::String("failed".to_string()));
    object.insert("lifecycle".to_string(), Value::String("failed".to_string()));
    object.insert("message".to_string(), Value::String(message.to_string()));
    object.insert("recovery_required".to_string(), Value::Bool(true));
    write_json(&path, &value)
}

pub(crate) fn decode(input: &str) -> Result<DecodedTransaction, String> {
    let state: TransactionState = serde_json::from_str(input)
        .map_err(|error| format!("invalid transaction state: {error}"))?;
    if state.schema_version != 1 {
        return Err(format!(
            "unsupported transaction state schema: {}",
            state.schema_version
        ));
    }
    let guidance = rescue_guidance(Some(&state.status));
    Ok(DecodedTransaction { state, guidance })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn shared_transaction_fixture_decodes_and_classifies() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("../testdata/transaction_cases.json"))
                .expect("transaction parity fixture must be valid JSON");
        for case in cases {
            let name = case["name"].as_str().expect("fixture case needs a name");
            if case.get("error_contains").is_some() {
                let error = decode(&case["json"].to_string()).expect_err("invalid state must fail");
                assert!(
                    error.contains(case["error_contains"].as_str().unwrap()),
                    "{name}: {error}"
                );
                continue;
            }
            let decoded =
                decode(&case["json"].to_string()).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(
                decoded.state.status,
                case["expected"]["status"].as_str().unwrap(),
                "{name}"
            );
            assert_eq!(
                decoded.state.phase,
                case["expected"]["phase"].as_str().unwrap(),
                "{name}"
            );
            assert_eq!(
                decoded.state.disk,
                case["expected"]["disk"].as_str().unwrap(),
                "{name}"
            );
            assert_eq!(
                decoded.state.source.digest,
                case["expected"]["source_digest"].as_str().unwrap(),
                "{name}"
            );
            assert_eq!(
                decoded.guidance.severity,
                case["expected"]["severity"].as_str().unwrap(),
                "{name}"
            );
            assert_eq!(
                decoded.guidance.bootable,
                case["expected"]["bootable"].as_bool().unwrap(),
                "{name}"
            );
        }
    }

    #[test]
    fn writes_transaction_state_atomically_and_durably() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("transaction.json");
        let state: TransactionState = serde_json::from_value(serde_json::json!({
            "status": "partitioning",
            "phase": "storage",
            "lifecycle": "partitioning",
            "disk": "/dev/sda",
        }))
        .expect("transaction state");
        write_request(TransactionWriteInput {
            path: path.to_string_lossy().into_owned(),
            state,
        })
        .expect("transaction state should be written");
        let decoded = decode(&std::fs::read_to_string(path).expect("written state"))
            .expect("written state should decode");
        assert_eq!(decoded.state.status, "partitioning");
        assert_eq!(decoded.state.disk, "/dev/sda");
    }

    #[test]
    fn writes_failure_summary_with_recovery_marker_and_history() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("failure.json");
        let state: TransactionState = serde_json::from_value(serde_json::json!({
            "transaction_id": "native-test",
            "job_id": 7,
            "status": "storage_complete",
            "phase": "image",
            "lifecycle": "installing",
            "checks": [{"name": "power", "status": "pass"}],
            "partition_steps": [{"kind": "format_filesystem", "status": "completed"}]
        }))
        .expect("transaction state");
        write_failure_summary(path.to_str().unwrap(), &state, "native storage failed")
            .expect("failure summary should be durable");
        let value: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(value["status"], "failed");
        assert_eq!(value["recovery_required"], true);
        assert_eq!(value["job_id"], 7);
        assert_eq!(value["checks"][0]["name"], "power");
        assert_eq!(value["partition_steps"][0]["status"], "completed");
        assert!(!value.to_string().contains("password"));
    }

    #[test]
    fn rejects_unsafe_transaction_paths() {
        let state: TransactionState =
            serde_json::from_value(serde_json::json!({})).expect("default transaction state");
        let error = write_request(TransactionWriteInput {
            path: "../transaction.json".to_string(),
            state,
        })
        .expect_err("relative traversal path must fail");
        assert!(error.contains("absolute safe path"));
    }
}
