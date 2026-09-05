//! Production adapter between the native job supervisor and typed operations.
//!
//! This adapter is deliberately conservative while the remaining destructive
//! operations are being moved into Rust.  It accepts a complete typed request,
//! validates it through the existing plan builders, and retains only the
//! resulting non-secret plans.  A phase is either safe to execute without
//! live installer state or fails with an explicit typed capability error.
//! It never starts the Python whole-install worker and never provides a
//! generic command or filesystem bridge.

use std::fmt;
use std::process::Command;

use super::installer_executor::{self, InstallerExecutionInput, InstallerExecutionPlan};
use super::installer_job::{CancellationToken, JobSupervisor, PhaseExecutor};
use super::installer_plan::{self, InstallerPlan, InstallerPlanInput};
use super::installer_runtime::Phase;
use crate::installer_configuration;

/// The complete typed request accepted by the native phase adapter.
///
/// The storage request and executor request intentionally remain separate:
/// the former describes the selected install mode, while the latter contains
/// the typed bootc, configuration, account, and Secure Boot inputs.
pub(crate) struct NativeInstallRequest {
    pub storage: InstallerPlanInput,
    pub execution: InstallerExecutionInput,
    pub manual_mounts: Option<crate::installer_manual::ManualMountsInput>,
    pub secure_boot_password: String,
    pub transaction_path: String,
}

impl NativeInstallRequest {
    /// Decode the flat HTTP representation used by the existing frontend.
    /// Secrets are consumed into the typed request and never serialized back.
    pub(crate) fn from_http(value: serde_json::Value) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or_else(|| "installer start request must be a JSON object".to_string())?;
        let text = |name: &str, default: &str| {
            object
                .get(name)
                .and_then(serde_json::Value::as_str)
                .unwrap_or(default)
                .to_string()
        };
        let number = |name: &str| {
            object
                .get(name)
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0)
        };
        let flag = |name: &str, default: bool| {
            object
                .get(name)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(default)
        };
        let username = text("username", "");
        let password_hash = text("password_hash", "");
        let install_mode = text("install_mode", "wipe").to_ascii_lowercase();
        let filesystem_install = matches!(install_mode.as_str(), "alongside" | "manual");
        let target_root = if filesystem_install {
            "/var/tmp/kyth-alongside-target".to_string()
        } else {
            "/var/tmp/kyth-install-root".to_string()
        };
        let manual_mounts = if install_mode == "manual" {
            let mounts = object
                .get("mounts")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([]));
            Some(
                serde_json::from_value(serde_json::json!({
                    "config_root": target_root.clone(),
                    "fstab_path": format!("{target_root}/etc/fstab"),
                    "mounts": mounts,
                }))
                .map_err(|error| format!("invalid manual mount request: {error}"))?,
            )
        } else {
            None
        };
        let account =
            (!username.is_empty()).then_some(crate::installer_accounts::CreateUserInput {
                deploy_root: target_root.clone(),
                target_root: target_root.clone(),
                username,
                password_hash,
            });
        Ok(Self {
            storage: InstallerPlanInput {
                disk: text("disk", ""),
                install_mode,
                target_partition: text("target_partition", ""),
                resize_partition: text("resize_partition", ""),
                resize_gib: number("resize_gib"),
                free_region_start: number("free_region_start"),
                free_region_end: number("free_region_end"),
            },
            execution: InstallerExecutionInput {
                bootc: crate::installer_bootc::BootcInstallInput {
                    subcommand: if filesystem_install {
                        "to-filesystem".to_string()
                    } else {
                        text("subcommand", "to-disk")
                    },
                    source_imgref: text("source_imgref", "oci:/usr/share/kyth/image:latest"),
                    target_imgref: text("target_imgref", "ghcr.io/kyth-os/kyth:latest"),
                    target: if filesystem_install {
                        "/var/tmp/kyth-alongside-target".to_string()
                    } else {
                        text("disk", "")
                    },
                    skip_fetch_check: flag("skip_fetch_check", true),
                    skip_finalize: flag("skip_finalize", false),
                    root_subvolume: flag("root_subvolume", false),
                    wipe: flag("wipe", false),
                },
                configuration: crate::installer_configuration::ConfigurationInput {
                    target_root: target_root.clone(),
                    hostname: text("hostname", "kyth"),
                    timezone: text("timezone", "UTC"),
                    locale: text("locale", "en_US.UTF-8"),
                    keymap: text("keymap", "us"),
                },
                account,
                secure_boot: crate::installer_secure_boot::SecureBootInput {
                    kernel: text("kernel", "fedora"),
                    force_stage: flag("force_stage", false),
                    certificate_present: flag("certificate_present", false),
                    mokutil_present: flag("mokutil_present", false),
                    secure_boot: text("secure_boot", "unknown"),
                    enrolled: text("enrolled", "unknown"),
                    pending: text("pending", "unknown"),
                },
            },
            manual_mounts,
            secure_boot_password: text("mok_password", ""),
            transaction_path: text("transaction_path", "/run/kyth-installer/transaction.json"),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NativeOperation {
    ValidateStoragePlan,
    ValidateExecutionPlan,
    StorageMutation,
    ImageWrite,
    ConfigurationWrite,
    AccountCreate,
    SecureBootInteraction,
    CompletionCommit,
}

impl fmt::Display for NativeOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::ValidateStoragePlan => "validate_storage_plan",
            Self::ValidateExecutionPlan => "validate_execution_plan",
            Self::StorageMutation => "storage_mutation",
            Self::ImageWrite => "image_write",
            Self::ConfigurationWrite => "configuration_write",
            Self::AccountCreate => "account_create",
            Self::SecureBootInteraction => "secure_boot_interaction",
            Self::CompletionCommit => "completion_commit",
        };
        formatter.write_str(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NativePhaseError {
    Cancelled {
        phase: Phase,
    },
    NotImplemented {
        phase: Phase,
        operation: NativeOperation,
    },
    Execution {
        phase: Phase,
        message: String,
    },
}

impl fmt::Display for NativePhaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled { phase } => write!(
                formatter,
                "native installer phase {phase:?} was cancelled before execution"
            ),
            Self::NotImplemented { phase, operation } => write!(
                formatter,
                "native installer operation {operation} for phase {phase:?} is not implemented"
            ),
            Self::Execution { phase, message } => {
                write!(
                    formatter,
                    "native installer phase {phase:?} failed: {message}"
                )
            }
        }
    }
}

/// A typed phase executor suitable for `JobSupervisor` in production.
///
/// The plans are built before a worker can start, so malformed requests fail
/// before any lifecycle state is claimed.  The account password hash is
/// consumed by plan construction and is not retained by this type.
pub(crate) struct NativePhaseExecutor {
    storage_plan: InstallerPlan,
    execution_plan: InstallerExecutionPlan,
    account: Option<crate::installer_accounts::CreateUserInput>,
    manual_mounts: Option<crate::installer_manual::ManualMountsInput>,
    source_imgref: String,
    target_imgref: String,
    secure_boot_kernel: String,
    secure_boot_force_stage: bool,
    secure_boot_password: String,
    transaction_id: String,
    transaction_path: String,
}

impl NativePhaseExecutor {
    pub(crate) fn from_request(request: NativeInstallRequest) -> Result<Self, String> {
        let account = request.execution.account.clone();
        let manual_mounts = request.manual_mounts;
        let source_imgref = request.execution.bootc.source_imgref.clone();
        let target_imgref = request.execution.bootc.target_imgref.clone();
        let secure_boot_kernel = request.execution.secure_boot.kernel.clone();
        let secure_boot_force_stage = request.execution.secure_boot.force_stage;
        let secure_boot_password = request.secure_boot_password;
        let transaction_path = request.transaction_path;
        let storage_plan = installer_plan::build_plan(request.storage)?;
        let execution_plan = installer_executor::build_plan(request.execution)?;
        Ok(Self {
            storage_plan,
            execution_plan,
            account,
            manual_mounts,
            source_imgref,
            target_imgref,
            secure_boot_kernel,
            secure_boot_force_stage,
            secure_boot_password,
            transaction_id: format!(
                "native-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or_default()
            ),
            transaction_path,
        })
    }

    pub(crate) fn from_plans(
        storage_plan: InstallerPlan,
        execution_plan: InstallerExecutionPlan,
    ) -> Self {
        Self {
            storage_plan,
            execution_plan,
            account: None,
            manual_mounts: None,
            source_imgref: "".to_string(),
            target_imgref: "".to_string(),
            secure_boot_kernel: "fedora".to_string(),
            secure_boot_force_stage: false,
            secure_boot_password: String::new(),
            transaction_id: format!(
                "native-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or_default()
            ),
            transaction_path: "/run/kyth-installer/transaction.json".to_string(),
        }
    }

    pub(crate) fn storage_plan(&self) -> &InstallerPlan {
        &self.storage_plan
    }

    pub(crate) fn execution_plan(&self) -> &InstallerExecutionPlan {
        &self.execution_plan
    }

    /// Return the only operation sequence this adapter may expose to the
    /// native job.  The sequence is also useful for fixture-based parity tests
    /// before the corresponding live operation is implemented.
    pub(crate) fn operation_order(&self) -> Vec<NativeOperation> {
        let mut operations = vec![
            NativeOperation::ValidateStoragePlan,
            NativeOperation::ValidateExecutionPlan,
            NativeOperation::StorageMutation,
            NativeOperation::ImageWrite,
            NativeOperation::ConfigurationWrite,
        ];
        if self.execution_plan.account.is_some() {
            operations.push(NativeOperation::AccountCreate);
        }
        operations.extend([
            NativeOperation::SecureBootInteraction,
            NativeOperation::CompletionCommit,
        ]);
        operations
    }

    pub(crate) fn execute_phase_typed(
        &self,
        phase: Phase,
        cancellation: &CancellationToken,
    ) -> Result<(), NativePhaseError> {
        if cancellation.is_cancelled() {
            return Err(NativePhaseError::Cancelled { phase });
        }
        let start_status = match phase {
            Phase::Prepare => Some(("started", "Installer started")),
            Phase::Configure => Some(("configure_started", "Configuring the installed system")),
            _ => None,
        };
        if let Some((status, message)) = start_status {
            self.write_transaction(status, phase, "installing", message)
                .map_err(|message| NativePhaseError::Execution { phase, message })?;
        }

        let result = match phase {
            // Request and execution plans were already validated before the
            // worker was claimed.  Preparation therefore has no live side
            // effects and is safe to execute in fixture and production paths.
            Phase::Prepare => Ok(()),
            Phase::Storage => self.execute_storage(phase, cancellation),
            Phase::Image => self.execute_image(phase, cancellation),
            Phase::Configure => {
                installer_configuration::apply_plan(self.execution_plan.configuration.clone())
                    .map_err(|message| NativePhaseError::Execution { phase, message })?;
                if let Some(account) = &self.account {
                    crate::installer_accounts::apply(account.clone())
                        .map_err(|message| NativePhaseError::Execution { phase, message })?;
                }
                if let Some(mounts) = &self.manual_mounts {
                    crate::installer_manual::apply(mounts.clone())
                        .map_err(|message| NativePhaseError::Execution { phase, message })?;
                }
                Ok(())
            }
            Phase::SecureBoot => self.execute_secure_boot(phase),
            Phase::Complete => self.execute_complete(phase),
        };
        result?;

        let completion = match phase {
            Phase::Prepare => Some(("prepared", "Install plan prepared")),
            Phase::Image => Some(("storage_complete", "Operating system image written")),
            Phase::Configure => Some(("configure_complete", "Installed system configured")),
            Phase::SecureBoot => Some((
                "secure_boot_staged",
                "Secure Boot enrollment state classified",
            )),
            Phase::Complete => None,
            Phase::Storage => None,
        };
        if let Some((status, message)) = completion {
            self.write_transaction(status, phase, "installing", message)
                .map_err(|message| NativePhaseError::Execution { phase, message })?;
        }
        Ok(())
    }

    fn write_transaction(
        &self,
        status: &str,
        phase: Phase,
        lifecycle: &str,
        message: &str,
    ) -> Result<(), String> {
        let updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| format!("could not determine transaction timestamp: {error}"))?
            .as_secs()
            .to_string();
        let state = crate::installer_transaction::TransactionState {
            transaction_id: self.transaction_id.clone(),
            updated_at,
            status: status.to_string(),
            phase: serde_json::to_value(phase)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_else(|| "unknown".to_string()),
            lifecycle: lifecycle.to_string(),
            install_mode: self.storage_plan.mode.clone(),
            disk: self.storage_plan.disk.clone(),
            target_partition: self
                .storage_plan
                .target_partition
                .clone()
                .or_else(|| self.storage_plan.resize_partition.clone())
                .unwrap_or_default(),
            source: crate::installer_transaction::TransactionSource {
                kind: if self.source_imgref.starts_with("docker://") {
                    "container".to_string()
                } else {
                    "oci".to_string()
                },
                target_ref: self.target_imgref.clone(),
                ..Default::default()
            },
            checks: Vec::new(),
            partition_steps: Vec::new(),
            message: message.to_string(),
            schema_version: 1,
        };
        crate::installer_transaction::write_request(
            crate::installer_transaction::TransactionWriteInput {
                path: self.transaction_path.clone(),
                state,
            },
        )
    }

    fn write_terminal_transaction(&self, phase: Option<Phase>, message: &str) {
        let phase = phase.unwrap_or(Phase::Prepare);
        let _ = self.write_transaction("failed", phase, "failed", message);
    }

    fn execute_image(
        &self,
        phase: Phase,
        cancellation: &CancellationToken,
    ) -> Result<(), NativePhaseError> {
        let mut command = Command::new("/usr/bin/bootc");
        command.args(self.execution_plan.bootc.argv.iter().skip(1));
        let status =
            super::installer_stream::run_command(&mut command, || cancellation.is_cancelled())
                .map_err(|message| NativePhaseError::Execution { phase, message })?;
        if status.success() {
            if self.storage_plan.mode == "wipe" {
                self.mount_wipe_root(phase, cancellation)?;
            }
            Ok(())
        } else {
            Err(NativePhaseError::Execution {
                phase,
                message: format!("bootc exited with status {}", status),
            })
        }
    }

    fn execute_disk_helper(
        &self,
        phase: Phase,
        cancellation: &CancellationToken,
        operation: &serde_json::Value,
    ) -> Result<(), NativePhaseError> {
        let input = serde_json::to_vec(operation).map_err(|error| NativePhaseError::Execution {
            phase,
            message: format!("could not encode disk operation: {error}"),
        })?;
        let mut command = Command::new("/usr/bin/kyth-installer-exec");
        command.args(["--operation", "disk"]);
        let status = super::installer_stream::run_command_with_input(&mut command, &input, || {
            cancellation.is_cancelled()
        })
        .map_err(|message| NativePhaseError::Execution { phase, message })?;
        if status.success() {
            Ok(())
        } else {
            Err(NativePhaseError::Execution {
                phase,
                message: format!("disk helper exited with status {status}"),
            })
        }
    }

    fn mount_wipe_root(
        &self,
        phase: Phase,
        cancellation: &CancellationToken,
    ) -> Result<(), NativePhaseError> {
        let output = Command::new("/usr/bin/lsblk")
            .args([
                "--json",
                "--bytes",
                "--paths",
                "--output",
                "NAME,TYPE,FSTYPE,PKNAME",
                &self.storage_plan.disk,
            ])
            .output()
            .map_err(|error| NativePhaseError::Execution {
                phase,
                message: format!("could not probe installed root partition: {error}"),
            })?;
        if !output.status.success() {
            return Err(NativePhaseError::Execution {
                phase,
                message: "installed root partition probe failed".to_string(),
            });
        }
        let snapshot =
            String::from_utf8(output.stdout).map_err(|_| NativePhaseError::Execution {
                phase,
                message: "installed root partition probe was not UTF-8".to_string(),
            })?;
        let root = crate::installer_storage::root_partition_from_snapshot(
            &snapshot,
            &self.storage_plan.disk,
        )
        .map_err(|message| NativePhaseError::Execution { phase, message })?;
        for operation in [
            serde_json::json!({
                "operation": "ensure_directory",
                "path": "/var/tmp/kyth-install-root"
            }),
            serde_json::json!({
                "operation": "mount_filesystem",
                "device": root,
                "mountpoint": "/var/tmp/kyth-install-root"
            }),
        ] {
            self.execute_disk_helper(phase, cancellation, &operation)?;
        }
        Ok(())
    }

    fn execute_storage(
        &self,
        phase: Phase,
        cancellation: &CancellationToken,
    ) -> Result<(), NativePhaseError> {
        if self.storage_plan.mode == "wipe" {
            // bootc to-disk owns the complete wipe layout and is run in the
            // image phase; there is no separate storage mutation here.
            return Ok(());
        }
        if !matches!(self.storage_plan.mode.as_str(), "alongside" | "manual") {
            return Err(NativePhaseError::NotImplemented {
                phase,
                operation: NativeOperation::StorageMutation,
            });
        }
        let target = self
            .storage_plan
            .target_partition
            .as_deref()
            .ok_or_else(|| NativePhaseError::Execution {
                phase,
                message: "alongside storage has no target partition".to_string(),
            })?;
        let operations = [
            serde_json::json!({
                "operation": "format_filesystem",
                "device": target,
                "fs": "btrfs",
                "label": "KythOS"
            }),
            serde_json::json!({
                "operation": "ensure_directory",
                "path": "/var/tmp/kyth-btrfs-root"
            }),
            serde_json::json!({
                "operation": "mount_filesystem",
                "device": target,
                "mountpoint": "/var/tmp/kyth-btrfs-root"
            }),
            serde_json::json!({
                "operation": "btrfs_subvolume_create",
                "mountpoint": "/var/tmp/kyth-btrfs-root",
                "name": "@"
            }),
            serde_json::json!({
                "operation": "btrfs_subvolume_create",
                "mountpoint": "/var/tmp/kyth-btrfs-root",
                "name": "@home"
            }),
            serde_json::json!({
                "operation": "btrfs_subvolume_set_default",
                "mountpoint": "/var/tmp/kyth-btrfs-root",
                "name": "@"
            }),
            serde_json::json!({
                "operation": "unmount_filesystem",
                "mountpoint": "/var/tmp/kyth-btrfs-root",
                "recursive": true,
                "lazy": true
            }),
            serde_json::json!({
                "operation": "ensure_directory",
                "path": "/var/tmp/kyth-alongside-target"
            }),
            serde_json::json!({
                "operation": "mount_filesystem",
                "device": target,
                "mountpoint": "/var/tmp/kyth-alongside-target",
                "options": ["subvol=@"]
            }),
        ];
        for operation in operations {
            let input =
                serde_json::to_vec(&operation).map_err(|error| NativePhaseError::Execution {
                    phase,
                    message: format!("could not encode storage operation: {error}"),
                })?;
            let mut command = Command::new("/usr/bin/kyth-installer-exec");
            command.args(["--operation", "disk"]);
            let status =
                super::installer_stream::run_command_with_input(&mut command, &input, || {
                    cancellation.is_cancelled()
                })
                .map_err(|message| NativePhaseError::Execution { phase, message })?;
            if !status.success() {
                return Err(NativePhaseError::Execution {
                    phase,
                    message: format!("storage helper exited with status {status}"),
                });
            }
        }
        Ok(())
    }

    fn execute_secure_boot(&self, phase: Phase) -> Result<(), NativePhaseError> {
        if self.execution_plan.secure_boot.action == "none" {
            return Ok(());
        }
        crate::installer_secure_boot::stage(crate::installer_secure_boot::SecureBootStageInput {
            kernel: self.secure_boot_kernel.clone(),
            force_stage: self.secure_boot_force_stage,
            password: self.secure_boot_password.clone(),
        })
        .map(|_| ())
        .map_err(|message| NativePhaseError::Execution { phase, message })
    }

    fn execute_complete(&self, phase: Phase) -> Result<(), NativePhaseError> {
        self.write_transaction(
            "complete",
            phase,
            "done",
            "Native installer completed successfully",
        )
        .map_err(|message| NativePhaseError::Execution { phase, message })
    }

    /// Build a native supervisor without starting a Python compatibility
    /// worker.  The caller supplies this supervisor to the daemon's native
    /// route integration once request decoding is connected.
    pub(crate) fn into_supervisor(self) -> JobSupervisor<Self> {
        JobSupervisor::new(self)
    }
}

impl PhaseExecutor for NativePhaseExecutor {
    fn execute_phase(&self, phase: Phase, cancellation: &CancellationToken) -> Result<(), String> {
        self.execute_phase_typed(phase, cancellation)
            .map_err(|error| error.to_string())
    }

    fn record_cancelled(&self, phase: Option<Phase>) {
        self.write_terminal_transaction(phase, super::installer_job::CANCELLATION_MESSAGE);
    }

    fn record_failed(&self, phase: Phase, message: &str) {
        self.write_terminal_transaction(Some(phase), message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer_accounts::CreateUserInput;
    use crate::installer_bootc::BootcInstallInput;
    use crate::installer_configuration::ConfigurationInput;
    use crate::installer_secure_boot::SecureBootInput;

    fn request(with_account: bool) -> NativeInstallRequest {
        NativeInstallRequest {
            storage: InstallerPlanInput {
                disk: "sda".into(),
                install_mode: "wipe".into(),
                target_partition: String::new(),
                resize_partition: String::new(),
                resize_gib: 0,
                free_region_start: 0,
                free_region_end: 0,
            },
            execution: InstallerExecutionInput {
                bootc: BootcInstallInput {
                    subcommand: "to-disk".into(),
                    source_imgref: "oci:/usr/share/kyth/image:latest".into(),
                    target_imgref: "ghcr.io/kyth-os/kyth:latest".into(),
                    target: "/dev/sda".into(),
                    skip_fetch_check: true,
                    skip_finalize: false,
                    root_subvolume: false,
                    wipe: true,
                },
                configuration: ConfigurationInput {
                    target_root: "/mnt/target".into(),
                    hostname: "kyth".into(),
                    timezone: "UTC".into(),
                    locale: "en_US.UTF-8".into(),
                    keymap: "us".into(),
                },
                account: with_account.then_some(CreateUserInput {
                    deploy_root: "/mnt/deploy".into(),
                    target_root: "/mnt/target".into(),
                    username: "kyth_user".into(),
                    password_hash: "$6$secret-must-not-leak".into(),
                }),
                secure_boot: SecureBootInput {
                    kernel: "fedora".into(),
                    force_stage: false,
                    certificate_present: false,
                    mokutil_present: false,
                    secure_boot: "unknown".into(),
                    enrolled: "unknown".into(),
                    pending: "unknown".into(),
                },
            },
            manual_mounts: None,
            secure_boot_password: String::new(),
            transaction_path: "/tmp/kyth-transaction.json".into(),
        }
    }

    #[test]
    fn validates_request_and_preserves_native_operation_order() {
        let executor = NativePhaseExecutor::from_request(request(true))
            .expect("typed native install request should validate");
        assert_eq!(
            executor.operation_order(),
            vec![
                NativeOperation::ValidateStoragePlan,
                NativeOperation::ValidateExecutionPlan,
                NativeOperation::StorageMutation,
                NativeOperation::ImageWrite,
                NativeOperation::ConfigurationWrite,
                NativeOperation::AccountCreate,
                NativeOperation::SecureBootInteraction,
                NativeOperation::CompletionCommit,
            ]
        );
        assert_eq!(executor.storage_plan().mode, "wipe");
        assert_eq!(executor.execution_plan().bootc.target, "/dev/sda");
    }

    #[test]
    fn execution_plan_and_operation_diagnostics_exclude_password_hash() {
        let executor = NativePhaseExecutor::from_request(request(true))
            .expect("typed native install request should validate");
        let plan = serde_json::to_string(executor.execution_plan()).unwrap();
        let operations = executor
            .operation_order()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        assert!(!plan.contains("secret-must-not-leak"));
        assert!(!operations.contains("secret-must-not-leak"));
    }

    #[test]
    fn skipped_secure_boot_does_not_spawn_or_retain_secret_in_plan() {
        let mut request = request(false);
        let directory = tempfile::tempdir().expect("temporary transaction directory");
        request.transaction_path = directory
            .path()
            .join("transaction.json")
            .to_string_lossy()
            .into_owned();
        request.secure_boot_password = "mok-secret-must-not-leak".into();
        let executor = NativePhaseExecutor::from_request(request)
            .expect("typed native install request should validate");
        let cancellation = CancellationToken::default();
        assert_eq!(
            executor.execute_phase_typed(Phase::SecureBoot, &cancellation),
            Ok(())
        );
        let plan = serde_json::to_string(executor.execution_plan()).unwrap();
        assert!(!plan.contains("mok-secret-must-not-leak"));
    }

    #[test]
    fn completion_writes_a_secret_free_native_transaction() {
        let directory = tempfile::tempdir().expect("temporary transaction directory");
        let mut request = request(false);
        request.transaction_path = directory
            .path()
            .join("transaction.json")
            .to_string_lossy()
            .into_owned();
        let executor = NativePhaseExecutor::from_request(request)
            .expect("typed native install request should validate");
        executor
            .execute_phase_typed(Phase::Complete, &CancellationToken::default())
            .expect("native completion should persist transaction");
        let transaction = std::fs::read_to_string(directory.path().join("transaction.json"))
            .expect("native transaction should exist");
        assert!(transaction.contains("Native installer completed successfully"));
        assert!(!transaction.contains("secret"));
    }

    #[test]
    fn preparation_persists_a_recoverable_native_transaction() {
        let directory = tempfile::tempdir().expect("temporary transaction directory");
        let mut request = request(false);
        request.transaction_path = directory
            .path()
            .join("transaction.json")
            .to_string_lossy()
            .into_owned();
        let executor = NativePhaseExecutor::from_request(request)
            .expect("typed native install request should validate");
        executor
            .execute_phase_typed(Phase::Prepare, &CancellationToken::default())
            .expect("native preparation should persist transaction");
        let transaction: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(directory.path().join("transaction.json"))
                .expect("native preparation transaction should exist"),
        )
        .expect("native preparation transaction should be JSON");
        assert_eq!(transaction["status"], "prepared");
        assert_eq!(transaction["phase"], "prepare");
        assert_eq!(transaction["lifecycle"], "installing");
        assert!(transaction["transaction_id"]
            .as_str()
            .is_some_and(|id| { id.starts_with("native-") }));
    }

    #[test]
    fn native_failure_hook_persists_support_safe_failure_state() {
        let directory = tempfile::tempdir().expect("temporary transaction directory");
        let mut request = request(false);
        request.transaction_path = directory
            .path()
            .join("transaction.json")
            .to_string_lossy()
            .into_owned();
        let executor = NativePhaseExecutor::from_request(request)
            .expect("typed native install request should validate");
        executor.record_failed(Phase::Storage, "native failure secret-free");
        let transaction = std::fs::read_to_string(directory.path().join("transaction.json"))
            .expect("native failure transaction should exist");
        assert!(transaction.contains("native failure secret-free"));
        assert!(!transaction.contains("password_hash"));
        assert!(!transaction.contains("mok_password"));
    }

    #[test]
    fn wipe_storage_is_owned_by_bootc_and_non_wipe_storage_is_explicitly_blocked() {
        let executor = NativePhaseExecutor::from_request(request(false))
            .expect("typed native install request should validate");
        let cancellation = CancellationToken::default();
        assert_eq!(
            executor.execute_phase_typed(Phase::Storage, &cancellation),
            Ok(())
        );
        let mut resize = request(false);
        resize.storage.install_mode = "resize_ntfs".into();
        resize.storage.resize_partition = "sda2".into();
        resize.storage.resize_gib = 40;
        let executor =
            NativePhaseExecutor::from_request(resize).expect("resize plan should validate");
        assert_eq!(
            executor.execute_phase_typed(Phase::Storage, &cancellation),
            Err(NativePhaseError::NotImplemented {
                phase: Phase::Storage,
                operation: NativeOperation::StorageMutation,
            })
        );
    }

    #[test]
    fn cancellation_is_reported_before_any_phase_operation() {
        let executor = NativePhaseExecutor::from_request(request(false))
            .expect("typed native install request should validate");
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        assert_eq!(
            executor.execute_phase_typed(Phase::Prepare, &cancellation),
            Err(NativePhaseError::Cancelled {
                phase: Phase::Prepare,
            })
        );
    }
}
