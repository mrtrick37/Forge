//! Rust-owned execution helper for destructive installer operations.
//!
//! The daemon invokes this helper with typed operations on stdin. The helper
//! validates and projects each request with the same Rust plans used by the
//! native daemon. Scalar operations use fixed executable paths; streaming
//! operations are spawned and waited here so parent-death cancellation cannot
//! orphan a long-running utility.

mod installer_accounts;
mod installer_alongside;
mod installer_bootc;
mod installer_configuration;
mod installer_disk;
#[allow(dead_code)]
mod installer_journal;
mod installer_manual;
mod installer_orchestration;
#[allow(dead_code)]
mod installer_plan;
mod installer_probe;
#[allow(dead_code)]
mod installer_recovery;
#[allow(dead_code)]
mod installer_secure_boot;
#[allow(dead_code)]
mod installer_storage;
mod installer_stream;
#[allow(dead_code)]
mod installer_transaction;

use std::io::{self, Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode, ExitStatus, Stdio};

const MAX_OPERATION_BYTES: usize = 64 * 1024;

#[derive(serde::Deserialize)]
#[serde(tag = "kind", content = "request", rename_all = "snake_case")]
enum StreamingOperationInput {
    BootcInstall(installer_bootc::BootcInstallInput),
    Disk(installer_disk::DiskOperationInput),
}

fn read_operation_bytes() -> Result<Vec<u8>, String> {
    let mut input = Vec::new();
    io::stdin()
        .take((MAX_OPERATION_BYTES + 1) as u64)
        .read_to_end(&mut input)
        .map_err(|error| format!("could not read installer operation: {error}"))?;
    decode_operation_bytes(&input)
}

fn decode_operation_bytes(input: &[u8]) -> Result<Vec<u8>, String> {
    if input.len() > MAX_OPERATION_BYTES {
        return Err("installer operation is too large".to_string());
    }
    serde_json::from_slice::<serde_json::Value>(input)
        .map_err(|error| format!("invalid installer operation JSON: {error}"))?;
    Ok(input.to_vec())
}

fn operation_args_valid(args: &[String]) -> bool {
    args.len() == 2
        && args[0] == "--operation"
        && matches!(
            args[1].as_str(),
            "bootc-install"
                | "disk"
                | "journal-validate"
                | "journal-target"
                | "journal-commit"
                | "transaction-write"
                | "configuration-write"
                | "fstab-append"
                | "recovery-export"
                | "stream"
                | "secure-boot-plan"
                | "secure-boot-stage"
                | "reboot"
                | "orchestration"
                | "power-check"
                | "uuid-probe"
                | "alongside-home"
                | "manual-mounts"
                | "create-user"
        )
}

fn parent_death_signal() -> io::Result<()> {
    let parent_pid = unsafe { libc::getppid() };
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::getppid() } != parent_pid {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "installer parent exited during child setup",
        ));
    }
    Ok(())
}

fn exit_code(status: ExitStatus) -> ExitCode {
    ExitCode::from(
        status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or(1),
    )
}

fn run_stream(input: &[u8]) -> Result<ExitCode, String> {
    let operation = serde_json::from_slice::<StreamingOperationInput>(input)
        .map_err(|error| format!("invalid streaming operation JSON: {error}"))?;
    let (argv, description) = match operation {
        StreamingOperationInput::BootcInstall(input) => (
            installer_bootc::build_plan(input)?.argv,
            "bootc image installation",
        ),
        StreamingOperationInput::Disk(input) => {
            if !matches!(
                &input,
                installer_disk::DiskOperationInput::FilesystemResize { .. }
            ) {
                return Err("streaming disk operation must be a filesystem resize".to_string());
            }
            let plan = installer_disk::build_plan(input)?;
            (plan.argv, "filesystem resize")
        }
    };
    let executable = argv
        .first()
        .ok_or_else(|| format!("{description} plan was empty"))?;
    let mut command = Command::new(executable);
    command.args(&argv[1..]).stdin(Stdio::inherit());
    unsafe {
        command.pre_exec(parent_death_signal);
    }
    let status = installer_stream::run_command(&mut command, || false)
        .map_err(|error| format!("{description}: {error}"))?;
    Ok(exit_code(status))
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    if unsafe { libc::geteuid() } != 0 {
        return Err("kyth-installer-exec must run as root".to_string());
    }
    if !operation_args_valid(args) {
        return Err("unsupported installer operation".to_string());
    }

    let input = read_operation_bytes()?;
    if args[1] == "recovery-export" {
        let input = serde_json::from_slice::<installer_recovery::RecoveryExportInput>(&input)
            .map_err(|error| format!("invalid recovery export JSON: {error}"))?;
        let response = installer_recovery::export_logs(input)?;
        println!(
            "{}",
            serde_json::to_string(&response)
                .map_err(|error| format!("could not encode recovery export response: {error}"))?
        );
        return Ok(ExitCode::SUCCESS);
    }
    if args[1] == "stream" {
        return run_stream(&input);
    }
    if args[1] == "journal-validate" {
        let input = serde_json::from_slice::<installer_journal::JournalValidationInput>(&input)
            .map_err(|error| format!("invalid journal validation JSON: {error}"))?;
        println!("{}", installer_journal::validate_request(input));
        return Ok(ExitCode::SUCCESS);
    }
    if args[1] == "journal-target" {
        let input = serde_json::from_slice::<installer_journal::JournalTargetInput>(&input)
            .map_err(|error| format!("invalid journal target JSON: {error}"))?;
        println!("{}", installer_journal::validate_target_request(input));
        return Ok(ExitCode::SUCCESS);
    }
    if args[1] == "journal-commit" {
        let input = serde_json::from_slice::<installer_journal::JournalCommitInput>(&input)
            .map_err(|error| format!("invalid journal commit JSON: {error}"))?;
        let response = installer_journal::commit_request(input)?;
        println!(
            "{}",
            serde_json::to_string(&response)
                .map_err(|error| format!("could not encode journal response: {error}"))?
        );
        return Ok(ExitCode::SUCCESS);
    }
    if args[1] == "transaction-write" {
        let input = serde_json::from_slice::<installer_transaction::TransactionWriteInput>(&input)
            .map_err(|error| format!("invalid transaction write JSON: {error}"))?;
        installer_transaction::write_request(input)?;
        return Ok(ExitCode::SUCCESS);
    }
    if args[1] == "configuration-write" {
        let input = serde_json::from_slice::<installer_configuration::ConfigurationInput>(&input)
            .map_err(|error| format!("invalid configuration JSON: {error}"))?;
        let plan = installer_configuration::build_plan(input)?;
        installer_configuration::apply_plan(plan)?;
        return Ok(ExitCode::SUCCESS);
    }
    if args[1] == "uuid-probe" {
        let input = serde_json::from_slice::<installer_probe::UuidInput>(&input)
            .map_err(|error| format!("invalid UUID probe JSON: {error}"))?;
        let uuid = installer_probe::lookup_uuid(input)?;
        println!("{}", serde_json::json!({"uuid": uuid}));
        return Ok(ExitCode::SUCCESS);
    }
    if args[1] == "alongside-home" {
        let input = serde_json::from_slice::<installer_alongside::AlongsideHomeInput>(&input)
            .map_err(|error| format!("invalid alongside-home JSON: {error}"))?;
        let response = installer_alongside::apply(input)?;
        println!(
            "{}",
            serde_json::to_string(&response)
                .map_err(|error| format!("could not encode alongside-home response: {error}"))?
        );
        return Ok(ExitCode::SUCCESS);
    }
    if args[1] == "manual-mounts" {
        let input = serde_json::from_slice::<installer_manual::ManualMountsInput>(&input)
            .map_err(|error| format!("invalid manual mounts JSON: {error}"))?;
        let response = installer_manual::apply(input)?;
        println!(
            "{}",
            serde_json::to_string(&response)
                .map_err(|error| format!("could not encode manual mounts response: {error}"))?
        );
        return Ok(ExitCode::SUCCESS);
    }
    if args[1] == "create-user" {
        let input = serde_json::from_slice::<installer_accounts::CreateUserInput>(&input)
            .map_err(|error| format!("invalid create-user JSON: {error}"))?;
        installer_accounts::apply(input)?;
        return Ok(ExitCode::SUCCESS);
    }
    if args[1] == "fstab-append" {
        let input = serde_json::from_slice::<installer_configuration::FstabAppendInput>(&input)
            .map_err(|error| format!("invalid fstab append JSON: {error}"))?;
        installer_configuration::append_fstab(input)?;
        return Ok(ExitCode::SUCCESS);
    }
    if args[1] == "secure-boot-plan" {
        let input = serde_json::from_slice::<installer_secure_boot::SecureBootInput>(&input)
            .map_err(|error| format!("invalid Secure Boot JSON: {error}"))?;
        let plan = installer_secure_boot::build_plan(input)?;
        println!(
            "{}",
            serde_json::to_string(&plan)
                .map_err(|error| format!("could not encode Secure Boot plan: {error}"))?
        );
        return Ok(ExitCode::SUCCESS);
    }
    if args[1] == "secure-boot-stage" {
        let input = serde_json::from_slice::<installer_secure_boot::SecureBootStageInput>(&input)
            .map_err(|error| format!("invalid Secure Boot staging JSON: {error}"))?;
        let plan = installer_secure_boot::stage(input)?;
        println!(
            "{}",
            serde_json::to_string(&plan).map_err(|error| format!(
                "could not encode Secure Boot staging response: {error}"
            ))?
        );
        return Ok(ExitCode::SUCCESS);
    }
    if args[1] == "reboot" {
        let status = Command::new("/usr/bin/systemctl")
            .args(["reboot", "--no-block"])
            .status()
            .map_err(|error| format!("could not request installer reboot: {error}"))?;
        return Ok(exit_code(status));
    }
    if args[1] == "orchestration" {
        let input = serde_json::from_slice::<installer_orchestration::OrchestrationInput>(&input)
            .map_err(|error| format!("invalid orchestration JSON: {error}"))?;
        let response = installer_orchestration::apply(input)?;
        println!(
            "{}",
            serde_json::to_string(&response)
                .map_err(|error| format!("could not encode orchestration response: {error}"))?
        );
        return Ok(ExitCode::SUCCESS);
    }
    if args[1] == "power-check" {
        println!(
            "{}",
            serde_json::to_string(&installer_orchestration::power_check())
                .map_err(|error| format!("could not encode power response: {error}"))?
        );
        return Ok(ExitCode::SUCCESS);
    }
    let (argv, needs_confirmation, operation) = match args[1].as_str() {
        "bootc-install" => {
            let input = serde_json::from_slice::<installer_bootc::BootcInstallInput>(&input)
                .map_err(|error| format!("invalid bootc install operation JSON: {error}"))?;
            let plan = installer_bootc::build_plan(input)?;
            (plan.argv, false, "bootc install")
        }
        "disk" => {
            let input = serde_json::from_slice::<installer_disk::DiskOperationInput>(&input)
                .map_err(|error| format!("invalid disk operation JSON: {error}"))?;
            let backup_path = input.backup_path().map(str::to_owned);
            let plan = installer_disk::build_plan(input)?;
            if let Some(backup_path) = backup_path {
                let mut command = Command::new(&plan.argv[0]);
                command.args(&plan.argv[1..]);
                let status = command
                    .status()
                    .map_err(|error| format!("could not execute disk operation: {error}"))?;
                if status.success() {
                    installer_disk::sync_backup(&backup_path)?;
                }
                return Ok(ExitCode::from(
                    status
                        .code()
                        .and_then(|code| u8::try_from(code).ok())
                        .unwrap_or(1),
                ));
            }
            (plan.argv, plan.needs_confirmation, "disk operation")
        }
        _ => unreachable!("operation_args_valid checked the operation"),
    };
    // Do not resolve a root command through an inherited PATH. The plan still
    // exposes the logical `bootc` argv for parity, while the executor pins the
    // installed binary location.
    let executable = argv
        .first()
        .ok_or_else(|| format!("{operation} plan was empty"))?;
    // All executable paths come from the fixed Rust operation plan.
    let mut command = Command::new(executable);
    command.args(&argv[1..]);
    if needs_confirmation {
        command.stdin(Stdio::piped());
        // Ensure a helper cancellation cannot leave an interactive child
        // running after the parent process has gone away.
        let mut child = unsafe { command.pre_exec(parent_death_signal).spawn() }
            .map_err(|error| format!("could not spawn {operation}: {error}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(error) = stdin.write_all(b"Yes\n") {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("could not confirm {operation}: {error}"));
            }
        }
        let status = child
            .wait()
            .map_err(|error| format!("could not wait for {operation}: {error}"))?;
        return Ok(exit_code(status));
    }
    // exec(2) keeps the historical scalar-operation behavior. Streaming
    // operations use run_stream(), which owns a child and applies the same
    // parent-death cancellation signal before waiting for it.
    let error = unsafe { command.pre_exec(parent_death_signal).exec() };
    Err(format!("could not execute {operation}: {error}"))
}

fn main() -> ExitCode {
    match run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_typed_bootc_operation_is_accepted() {
        assert!(!operation_args_valid(&[
            "--operation".into(),
            "other".into()
        ]));
        assert!(!operation_args_valid(&["--operation".into()]));
        assert!(operation_args_valid(&[
            "--operation".into(),
            "bootc-install".into()
        ]));
        assert!(operation_args_valid(&["--operation".into(), "disk".into()]));
        assert!(operation_args_valid(&[
            "--operation".into(),
            "journal-validate".into()
        ]));
        assert!(operation_args_valid(&[
            "--operation".into(),
            "journal-target".into()
        ]));
        assert!(operation_args_valid(&[
            "--operation".into(),
            "journal-commit".into()
        ]));
        assert!(operation_args_valid(&[
            "--operation".into(),
            "transaction-write".into()
        ]));
        assert!(operation_args_valid(&[
            "--operation".into(),
            "configuration-write".into()
        ]));
        assert!(operation_args_valid(&[
            "--operation".into(),
            "stream".into()
        ]));
        assert!(operation_args_valid(&[
            "--operation".into(),
            "recovery-export".into()
        ]));
        assert!(operation_args_valid(&[
            "--operation".into(),
            "fstab-append".into()
        ]));
        assert!(operation_args_valid(&[
            "--operation".into(),
            "secure-boot-plan".into()
        ]));
        assert!(operation_args_valid(&[
            "--operation".into(),
            "secure-boot-stage".into()
        ]));
        assert!(operation_args_valid(&[
            "--operation".into(),
            "reboot".into()
        ]));
        assert!(operation_args_valid(&[
            "--operation".into(),
            "orchestration".into()
        ]));
        assert!(operation_args_valid(&[
            "--operation".into(),
            "power-check".into()
        ]));
        assert!(operation_args_valid(&[
            "--operation".into(),
            "uuid-probe".into()
        ]));
        assert!(operation_args_valid(&[
            "--operation".into(),
            "alongside-home".into()
        ]));
        assert!(operation_args_valid(&[
            "--operation".into(),
            "create-user".into()
        ]));
    }

    #[test]
    fn operation_input_is_bounded() {
        assert!(decode_operation_bytes(&vec![b' '; MAX_OPERATION_BYTES + 1]).is_err());
        assert!(decode_operation_bytes(b"not-json").is_err());
        assert!(serde_json::from_slice::<installer_bootc::BootcInstallInput>(
            br#"{"subcommand":"to-disk","source_imgref":"oci:/image","target_imgref":"kyth:latest","target":"/dev/sda","wipe":true}"#,
        ).is_ok());
    }

    #[test]
    fn streaming_rejects_non_streamable_disk_operations_before_spawn() {
        let request = serde_json::json!({
            "kind": "disk",
            "request": {
                "operation": "format_filesystem",
                "device": "/dev/sda3",
                "fs": "btrfs",
                "label": "KythOS"
            }
        });
        let input = serde_json::to_vec(&request).expect("stream request should serialize");
        assert!(run_stream(&input).is_err());
    }
}
