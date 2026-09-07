//! Native owner of digest-aware boot health, quarantine, and rollback.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kyth_shared::system::boot_health::{
    BootHealthState, DEFAULT_FAILURE_THRESHOLD, DEFAULT_STATE_PATH,
};
use kyth_shared::system::boot_runtime::boot_runtime_checks;
use kyth_shared::system::bootc_query::{fetch_status_data, image_digest_from_status};
use kyth_shared::system::process::{redact_sensitive_text, run_bounded};
use kyth_shared::system::update_coordinator::UpdateCoordinator;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or(0)
}

fn current_digest() -> io::Result<String> {
    fetch_status_data()
        .and_then(|value| image_digest_from_status(&value, "booted"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "could not determine booted image digest",
            )
        })
}

fn current_boot_id() -> String {
    fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

fn state_document(state: &BootHealthState) -> serde_json::Value {
    let mut document = serde_json::to_value(state).expect("BootHealthState serializes");
    document
        .as_object_mut()
        .expect("BootHealthState is an object")
        .insert("schema_version".into(), serde_json::json!(1));
    document
}

fn summary(state: &BootHealthState) -> String {
    let mut detail = format!(
        "status={} current={} last-healthy={} failures={} quarantined={}",
        state.status,
        if state.current_digest.is_empty() {
            "unknown"
        } else {
            &state.current_digest
        },
        if state.last_healthy_digest.is_empty() {
            "unknown"
        } else {
            &state.last_healthy_digest
        },
        state.failures,
        state.quarantined.len(),
    );
    if !state.last_rollback_error.is_empty() {
        detail.push_str(&format!(" rollback_error={:?}", state.last_rollback_error));
    }
    detail
}

fn run_rollback() -> io::Result<(i32, String)> {
    let output = run_bounded(
        &["/usr/bin/bootc".into(), "rollback".into()],
        Duration::from_secs(60),
    )?;
    let detail = redact_sensitive_text(&format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    ))
    .trim()
    .to_string();
    Ok((output.status.code().unwrap_or(1), detail))
}

fn maybe_rollback(
    coordinator: &UpdateCoordinator,
    before: &BootHealthState,
    updated: &BootHealthState,
    digest: &str,
) -> io::Result<()> {
    if !updated.quarantined.contains_key(digest)
        || before.quarantined.contains_key(digest)
        || updated.rollback_attempted_for == digest
    {
        return Ok(());
    }
    let (code, detail) = match run_rollback() {
        Ok(result) => result,
        Err(error) => {
            let detail = error.to_string();
            let _ = coordinator.note_rollback_attempted(digest, Some(&detail), now())?;
            eprintln!("bootc rollback errored for quarantined digest {digest}: {detail}");
            return Ok(());
        }
    };
    let error = (code != 0).then(|| {
        if detail.is_empty() {
            format!("exit {code}")
        } else {
            detail.clone()
        }
    });
    if let Some(error) = &error {
        eprintln!("bootc rollback failed for quarantined digest {digest}: {error}");
    } else {
        println!("Rolled back from quarantined digest {digest} — takes effect next boot");
    }
    coordinator.note_rollback_attempted(digest, error.as_deref(), now())?;
    Ok(())
}

fn required_checks() -> Vec<(String, bool, String)> {
    let status = fetch_status_data();
    let digest = status
        .as_ref()
        .and_then(|value| image_digest_from_status(value, "booted"))
        .unwrap_or_default();
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let os_id = os_release
        .lines()
        .find_map(|line| {
            line.strip_prefix("ID=")
                .map(|value| value.trim_matches(['"', '\'']).to_string())
        })
        .unwrap_or_default();
    let mut checks = vec![
        (
            "KythOS identity".into(),
            os_id == "kythos",
            format!("ID={}", if os_id.is_empty() { "missing" } else { &os_id }),
        ),
        (
            "bootc deployment".into(),
            !digest.is_empty(),
            if digest.is_empty() {
                "booted digest unavailable".into()
            } else {
                digest.clone()
            },
        ),
        (
            "bootc executable".into(),
            Path::new("/usr/bin/bootc").is_file(),
            "/usr/bin/bootc present".into(),
        ),
        (
            "Plasma shell".into(),
            Path::new("/usr/bin/plasmashell").is_file(),
            "/usr/bin/plasmashell present".into(),
        ),
        (
            "NetworkManager unit".into(),
            Path::new("/usr/lib/systemd/system/NetworkManager.service").is_file(),
            "NetworkManager unit present".into(),
        ),
    ];
    let kernel = fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    let module_path = format!("/usr/lib/modules/{kernel}");
    checks.push((
        "kernel modules".into(),
        Path::new(&module_path).is_dir(),
        format!("module tree for {kernel}"),
    ));
    let verifier = run_bounded(
        &["/usr/bin/kyth-boot-verify".into()],
        Duration::from_secs(30),
    );
    let measured = match verifier {
        Err(_) => (true, "kyth-boot-verify not present — skipped".into()),
        Ok(output) if output.status.code() == Some(2) => (
            false,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ),
        Ok(output) => (
            true,
            format!(
                "kyth-boot-verify: {}",
                String::from_utf8_lossy(&output.stdout).trim()
            ),
        ),
    };
    checks.push(("Measured boot".into(), measured.0, measured.1));
    checks.extend(
        boot_runtime_checks()
            .into_iter()
            .map(|check| (check.name, check.passed, check.detail)),
    );
    checks
}

fn parse_args() -> Result<(PathBuf, String, Vec<String>), String> {
    let mut args = env::args().skip(1);
    let mut state = PathBuf::from(DEFAULT_STATE_PATH);
    let mut command = None;
    let mut rest = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--state" {
            state = PathBuf::from(args.next().ok_or("--state requires a path")?);
        } else if command.is_none() {
            command = Some(arg);
        } else {
            rest.push(arg);
        }
    }
    Ok((state, command.ok_or("a command is required")?, rest))
}

fn usage() {
    eprintln!("usage: kyth-boot-health [--state PATH] <check|status [--json]|mark-healthy|record-failure --reason TEXT [--threshold N]|clear-quarantine --digest DIGEST|retry-rollback --digest DIGEST>");
}

fn execute() -> io::Result<bool> {
    let (path, command, args) =
        parse_args().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let coordinator = UpdateCoordinator::new(path);
    let result: io::Result<bool> = match command.as_str() {
        "check" => {
            let checks = required_checks();
            for (name, passed, detail) in &checks {
                println!("{} {name}: {detail}", if *passed { "PASS" } else { "FAIL" });
            }
            Ok(checks.iter().all(|(_, passed, _)| *passed))
        }
        "status" => {
            let state = coordinator.read();
            if args.iter().any(|arg| arg == "--json") {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&state_document(&state)).unwrap()
                );
            } else {
                println!("{}", summary(&state));
            }
            Ok(true)
        }
        "mark-healthy" => {
            let digest = current_digest()?;
            coordinator.mark_healthy(&digest, now())?;
            println!("Marked {digest} healthy");
            Ok(true)
        }
        "record-failure" => {
            let reason = args
                .windows(2)
                .find(|pair| pair[0] == "--reason")
                .map(|pair| pair[1].clone())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "--reason is required")
                })?;
            let threshold = args
                .windows(2)
                .find(|pair| pair[0] == "--threshold")
                .and_then(|pair| pair[1].parse::<i64>().ok())
                .unwrap_or(DEFAULT_FAILURE_THRESHOLD)
                .max(1);
            let digest = current_digest()?;
            let before = coordinator.read();
            let updated = coordinator.record_failure(
                &digest,
                &current_boot_id(),
                &reason,
                threshold,
                now(),
            )?;
            println!("{}", summary(&updated));
            maybe_rollback(&coordinator, &before, &updated, &digest)?;
            Ok(true)
        }
        "clear-quarantine" => {
            let digest = args
                .windows(2)
                .find(|pair| pair[0] == "--digest")
                .map(|pair| pair[1].clone())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "--digest is required")
                })?;
            let was_quarantined = coordinator.read().quarantined.contains_key(&digest);
            coordinator.clear_quarantine(&digest, now())?;
            println!(
                "{}",
                if was_quarantined {
                    format!("Cleared quarantine for {digest}")
                } else {
                    format!("Digest {digest} was not quarantined")
                }
            );
            Ok(true)
        }
        "retry-rollback" => {
            let digest = args
                .windows(2)
                .find(|pair| pair[0] == "--digest")
                .map(|pair| pair[1].clone())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "--digest is required")
                })?;
            let state = coordinator.read();
            if !state.quarantined.contains_key(&digest) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Digest {digest} is not quarantined — nothing to retry"),
                ));
            }
            coordinator.transaction(|mut current| {
                if current.rollback_attempted_for == digest {
                    current.rollback_attempted_for.clear();
                    current.last_rollback_error.clear();
                    current.last_rollback_at = 0;
                }
                current
            })?;
            let (code, detail) = run_rollback()?;
            let error = (code != 0).then(|| {
                if detail.is_empty() {
                    format!("exit {code}")
                } else {
                    detail
                }
            });
            coordinator.note_rollback_attempted(&digest, error.as_deref(), now())?;
            if let Some(error) = error {
                eprintln!("bootc rollback still failed for {digest}: {error}");
                Ok(false)
            } else {
                println!("Rolled back from quarantined digest {digest} — takes effect next boot");
                Ok(true)
            }
        }
        _ => {
            usage();
            Ok(false)
        }
    };
    result
}

fn main() -> ExitCode {
    match execute() {
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
            eprintln!("ERROR: {error}");
            usage();
            ExitCode::from(2)
        }
        result => finish(result),
    }
}

fn finish(result: io::Result<bool>) -> ExitCode {
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            eprintln!("ERROR: {error}");
            ExitCode::from(1)
        }
    }
}
