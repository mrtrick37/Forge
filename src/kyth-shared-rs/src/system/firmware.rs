//! Port of `kyth_shared.system.firmware` — fwupd helpers.

use std::time::Duration;
use super::runtime_output::count_fwupd_updates;
use std::fs::OpenOptions;
use rustix::fs::{flock, FlockOperation};

fn run_with_timeout(cmd: &[String], timeout: Duration) -> Option<(i32, String)> {
    if cmd.is_empty() { return None; }
    let output = super::process::run_bounded(cmd, timeout).ok()?;
    let combined = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    Some((output.status.code().unwrap_or(-1), combined.trim().to_string()))
}

pub fn firmware_refresh_commands() -> Vec<Vec<String>> {
    vec![vec!["fwupdmgr".to_string(), "refresh".to_string(), "--force".to_string()]]
}
pub fn firmware_devices_command() -> Vec<String> { vec!["fwupdmgr".to_string(), "get-devices".to_string()] }
pub fn firmware_updates_command() -> Vec<String> { vec!["fwupdmgr".to_string(), "get-updates".to_string()] }
pub fn firmware_update_command() -> Vec<String> { vec!["fwupdmgr".to_string(), "update".to_string(), "--assume-yes".to_string(), "--no-reboot-check".to_string()] }

pub fn run_firmware_refresh(timeout: u64) -> (bool, String) {
    let cmd = firmware_refresh_commands()[0].clone();
    match run_with_timeout(&cmd, Duration::from_secs(timeout)) {
        Some((0, out)) => (true, out),
        Some((_, out)) if !out.is_empty() => (false, out),
        Some((_, _)) => (false, "".to_string()),
        None => (false, format!("fwupdmgr refresh timed out after {}s", timeout)),
    }
}

pub fn check_firmware_updates(timeout: u64) -> i32 {
    let cmd = firmware_updates_command();
    match run_with_timeout(&cmd, Duration::from_secs(timeout)) {
        Some((2, _)) => 0,
        Some((0, stdout)) if stdout.trim().is_empty() => 0,
        Some((0, stdout)) => count_fwupd_updates(&stdout) as i32,
        _ => 0,
    }
}

pub fn run_firmware_update(timeout: u64) -> (bool, String) {
    let cmd = firmware_update_command();
    match run_with_timeout(&cmd, Duration::from_secs(timeout)) {
        Some((0, out)) => (true, out),
        Some((_, out)) => (false, out),
        None => (false, format!("fwupdmgr update timed out after {}s", timeout)),
    }
}

/// Run the interactive `ujust firmware-update` flow through the same bounded
/// fwupd command definitions used by the update watcher.  The watcher owns
/// the cross-process lock; this user-triggered route owns no long-lived state
/// and therefore reports each failure directly to its caller.
pub fn firmware_update_recipe() -> Result<String, String> {
    let (refreshed, refresh_output) = run_firmware_refresh(60);
    if !refreshed {
        return Err(if refresh_output.is_empty() {
            "fwupdmgr refresh failed".to_string()
        } else {
            refresh_output
        });
    }
    let count = check_firmware_updates(20);
    if count <= 0 {
        return Ok("No firmware updates available.".to_string());
    }
    let (updated, output) = run_firmware_update(600);
    if updated {
        Ok(output)
    } else {
        Err(if output.is_empty() {
            "fwupdmgr update failed".to_string()
        } else {
            output
        })
    }
}

/// Refresh metadata, count pending devices, and stage updates while sharing
/// the same non-blocking lock used by Guardian and the Hub firmware probe.
/// A lock miss is intentionally a no-op: another owner will finish the batch.
pub fn stage_firmware_batch() -> (bool, i32, String) {
    let path = std::env::var_os("KYTH_FWUPD_LOCK").map(std::path::PathBuf::from).unwrap_or_else(|| "/run/kyth-fwupd.lock".into());
    let Some(parent) = path.parent() else { return (false, 0, String::new()); };
    if std::fs::create_dir_all(parent).is_err() { return (false, 0, String::new()); }
    let Ok(lock) = OpenOptions::new().create(true).write(true).open(path) else { return (false, 0, String::new()); };
    if flock(&lock, FlockOperation::NonBlockingLockExclusive).is_err() { return (false, 0, String::new()); }
    let _ = run_firmware_refresh(60);
    let count = check_firmware_updates(20);
    if count <= 0 {
        let _ = flock(&lock, FlockOperation::Unlock);
        return (false, 0, String::new());
    }
    let result = run_firmware_update(600);
    let _ = flock(&lock, FlockOperation::Unlock);
    (result.0, count, result.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commands() {
        assert_eq!(firmware_devices_command(), vec!["fwupdmgr","get-devices"]);
        assert_eq!(firmware_updates_command(), vec!["fwupdmgr","get-updates"]);
    }

    #[test]
    fn fwupd_count_is_nonnegative_when_no_updates_exist() {
        assert_eq!(count_fwupd_updates("No updates available\n"), 0);
    }
}
