use rustix::fs::{flock, FlockOperation};
use std::fs::OpenOptions;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const REQUIRED_FREE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const DEFAULT_CONFIG: &str = "/etc/kyth/auto-update.toml";

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn rollout_ring() -> String {
    std::fs::read_to_string(DEFAULT_CONFIG)
        .ok()
        .and_then(|text| text.parse::<toml::Value>().ok())
        .and_then(|value| value.get("auto_update").cloned().or(Some(value)))
        .and_then(|value| {
            value
                .get("rollout_ring")
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "follow-image".into())
}

fn free_bytes(path: &str) -> Option<u64> {
    let stat = rustix::fs::statvfs(path).ok()?;
    Some(stat.f_bavail.saturating_mul(stat.f_frsize))
}

fn output_text(output: &std::process::Output) -> String {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    kyth_shared::system::process::redact_sensitive_text(
        kyth_shared::system::process::strip_ansi(text.trim()).as_str(),
    )
    .chars()
    .take(2000)
    .collect()
}

fn run_upgrade() -> Result<String, String> {
    if let Some(free) = free_bytes("/sysroot") {
        if free < REQUIRED_FREE_BYTES {
            return Err(format!(
                "Not enough free disk space: {} MiB free, need 2048 MiB",
                free / (1024 * 1024)
            ));
        }
    }
    let lock = OpenOptions::new()
        .create(true)
        .write(true)
        .open("/run/kyth-bootc.lock")
        .map_err(|error| format!("Could not open the bootc upgrade lock: {error}"))?;
    flock(&lock, FlockOperation::NonBlockingLockExclusive).map_err(|_| {
        "Another bootc upgrade is in progress; will retry on the next run".to_string()
    })?;
    let argv = vec!["/usr/bin/bootc".to_string(), "upgrade".to_string()];
    let result = kyth_shared::system::process::run_bounded(&argv, Duration::from_secs(3600));
    let _ = flock(&lock, FlockOperation::Unlock);
    match result {
        Ok(output) if output.status.success() => Ok(output_text(&output)),
        Ok(output) => Err(output_text(&output)),
        Err(error) => Err(if error.kind() == std::io::ErrorKind::TimedOut {
            "bootc upgrade timed out; retry later".into()
        } else {
            format!("bootc upgrade could not start: {error}")
        }),
    }
}

fn upgrade() -> Result<String, String> {
    if !rustix::process::getuid().is_root() {
        return Err("kyth-safe-upgrade must run as root".into());
    }
    let status = kyth_shared::system::bootc_query::fetch_status_data()
        .ok_or_else(|| "Could not determine the booted image status".to_string())?;
    let reference = kyth_shared::system::bootc_query::image_reference_from_status(&status)
        .ok_or_else(|| "Could not determine the booted image reference".to_string())?;
    let ring = rollout_ring();
    if let Some(reason) = kyth_shared::system::boot_health::rollout_policy_reason(&reference, &ring)
    {
        return Err(format!("Update blocked by rollout policy: {reason}"));
    }
    let branch = kyth_shared::system::bootc_policy::branch_from_ref(Some(&reference))
        .unwrap_or_else(|| "latest".into());
    let check = kyth_shared::system::registry::check_registry_update_with_timeout(
        &status,
        &branch,
        kyth_shared::system::bootc_policy::REGISTRY,
        Duration::from_secs(30),
    );
    let remote = kyth_shared::system::safe_upgrade_policy::remote_digest_for_safe_upgrade(
        &check.state,
        &check.detail,
        check.remote_probe_failed,
        &check.manifest_raw,
    )?;
    let state = kyth_shared::system::boot_health::read_default_state();
    if let Some(remote) = remote.as_deref() {
        if let Some(reason) = kyth_shared::system::boot_health::quarantine_reason(&state, remote) {
            return Err(format!("Update blocked: {reason}"));
        }
    }
    let booted = kyth_shared::system::bootc_query::image_digest_from_status(&status, "booted");
    let staged = kyth_shared::system::bootc_query::image_digest_from_status(&status, "staged");
    if remote
        .as_deref()
        .is_some_and(|digest| booted.as_deref() == Some(digest))
    {
        return Ok("KythOS is already running the latest allowed digest.".into());
    }
    if remote
        .as_deref()
        .is_some_and(|digest| staged.as_deref() == Some(digest))
    {
        return kyth_shared::system::boot_finalize::finalize_staged(false).map(|detail| {
            if detail.is_empty() {
                "Latest digest was promoted to the next boot.".into()
            } else {
                detail
            }
        });
    }
    if kyth_shared::system::bootc_query::active_operation().is_some() {
        return Err("Another bootc upgrade is in progress; retry later".into());
    }
    let detail = run_upgrade()?;
    let after = kyth_shared::system::bootc_query::fetch_status_data();
    let staged_after = after.as_ref().and_then(|data| {
        kyth_shared::system::bootc_query::image_digest_from_status(data, "staged")
    });
    let staged_quarantine_reason = staged_after.as_deref().and_then(|digest| {
        let state = kyth_shared::system::boot_health::read_default_state();
        kyth_shared::system::boot_health::quarantine_reason(&state, digest)
    });
    let staged_digest = kyth_shared::system::safe_upgrade_policy::validate_staged_digest(
        remote.as_deref(),
        staged_after.as_deref(),
        staged_quarantine_reason.as_deref(),
    )
    .map_err(|error| {
        if detail.is_empty() || error.starts_with("Update blocked:") {
            error
        } else {
            detail.clone()
        }
    })?;
    let coordinator = kyth_shared::system::update_coordinator::UpdateCoordinator::new(
        kyth_shared::system::boot_health::DEFAULT_STATE_PATH,
    );
    coordinator
        .record_staged(
            &staged_digest,
            kyth_shared::system::boot_health::image_ring(&reference).unwrap_or(&ring),
            now(),
        )
        .map_err(|error| format!("Could not persist staged update state: {error}"))?;
    let finalized = kyth_shared::system::boot_finalize::finalize_staged(false)?;
    let degraded_note = if remote.is_none() {
        format!("Remote manifest preflight unavailable; bootc staged digest {staged_digest}.")
    } else {
        String::new()
    };
    Ok(if finalized.is_empty() {
        if !degraded_note.is_empty() {
            degraded_note
        } else if detail.is_empty() {
            "Update staged and promoted to the next boot.".into()
        } else {
            detail
        }
    } else if degraded_note.is_empty() {
        finalized
    } else {
        format!("{finalized} {degraded_note}")
    })
}

fn main() -> std::process::ExitCode {
    if std::env::args().nth(1).is_some() {
        eprintln!("kyth-safe-upgrade accepts no arguments");
        return std::process::ExitCode::from(64);
    }
    match upgrade() {
        Ok(detail) => {
            if !detail.is_empty() {
                println!("{detail}");
            }
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::from(1)
        }
    }
}
