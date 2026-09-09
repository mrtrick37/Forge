use rustix::fs::{flock, FlockOperation};
use std::fs::OpenOptions;
use std::time::Duration;

const DEFAULT_CONFIG: &str = "/etc/kyth/auto-update.toml";

fn run(program: &str, args: &[&str], timeout: Duration) -> Option<(bool, String)> {
    let mut argv = vec![program.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    let output = kyth_shared::system::process::run_bounded(&argv, timeout).ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .trim()
    .chars()
    .take(2000)
    .collect();
    Some((output.status.success(), text))
}

fn config() -> toml::Value {
    std::fs::read_to_string(DEFAULT_CONFIG)
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or_else(|| {
            toml::Value::Table(toml::toml! {
                auto_update = {
                    enabled = true,
                    rollout_ring = "follow-image",
                    quiet_hours_start = "02:00",
                    quiet_hours_end = "07:00",
                    skip_if_metered = true,
                    skip_if_gaming = true,
                    startup_grace_minutes = 20,
                    bootc_timeout = 1800,
                }
            })
        })
}

fn settings<'a>(config: &'a toml::Value) -> &'a toml::Value {
    config.get("auto_update").unwrap_or(config)
}

fn bool_setting(config: &toml::Value, key: &str, default: bool) -> bool {
    settings(config)
        .get(key)
        .and_then(toml::Value::as_bool)
        .unwrap_or(default)
}

fn integer_setting(config: &toml::Value, key: &str, default: i64) -> i64 {
    settings(config)
        .get(key)
        .and_then(toml::Value::as_integer)
        .unwrap_or(default)
}

fn string_setting<'a>(config: &'a toml::Value, key: &str, default: &'a str) -> &'a str {
    settings(config)
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or(default)
}

fn local_minute() -> Option<u32> {
    let (_, output) = run("date", &["+%H:%M"], Duration::from_secs(2))?;
    let (hour, minute) = output.trim().split_once(':')?;
    Some(hour.parse::<u32>().ok()?.saturating_mul(60) + minute.parse::<u32>().ok()?)
}

fn parse_minute(value: &str) -> Option<u32> {
    let (hour, minute) = value.split_once(':')?;
    Some(hour.parse::<u32>().ok()?.saturating_mul(60) + minute.parse::<u32>().ok()?)
}

fn skip_reason(config: &toml::Value) -> Option<String> {
    if !bool_setting(config, "enabled", true) {
        return Some("auto-update disabled in config".into());
    }
    if let (Some(now), Some(start), Some(end)) = (
        local_minute(),
        parse_minute(string_setting(config, "quiet_hours_start", "02:00")),
        parse_minute(string_setting(config, "quiet_hours_end", "07:00")),
    ) {
        let quiet = if start <= end {
            start <= now && now < end
        } else {
            now >= start || now < end
        };
        if quiet {
            return Some(format!(
                "quiet hours ({}–{})",
                string_setting(config, "quiet_hours_start", "02:00"),
                string_setting(config, "quiet_hours_end", "07:00")
            ));
        }
    }
    let grace = integer_setting(config, "startup_grace_minutes", 20).max(0) as f64 * 60.0;
    if grace > 0.0 {
        if let Ok(raw) = std::fs::read_to_string("/proc/uptime") {
            if raw
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<f64>().ok())
                .is_some_and(|uptime| uptime < grace)
            {
                return Some(format!(
                    "startup grace period ({} min remaining)",
                    ((grace
                        - raw
                            .split_whitespace()
                            .next()
                            .and_then(|value| value.parse::<f64>().ok())
                            .unwrap_or(0.0))
                    .ceil() as i64
                        + 59)
                        / 60
                ));
            }
        }
    }
    if bool_setting(config, "skip_if_gaming", true) {
        if let Some((_, output)) = run("ps", &["-eo", "args="], Duration::from_secs(3)) {
            let gaming = output.lines().any(|line| {
                let line = line.to_ascii_lowercase();
                ["gamescope", "wine", "wineserver", "pressure-vessel-wrap"]
                    .iter()
                    .any(|needle| line.contains(needle))
            });
            if gaming {
                return Some("gaming process detected (/proc-equivalent scan)".into());
            }
        }
    }
    if bool_setting(config, "skip_if_metered", true) {
        if let Some((true, output)) = run(
            "busctl",
            &[
                "get-property",
                "org.freedesktop.NetworkManager",
                "/org/freedesktop/NetworkManager",
                "org.freedesktop.NetworkManager",
                "Metered",
            ],
            Duration::from_secs(5),
        ) {
            if matches!(output.split_whitespace().last(), Some("1" | "3")) {
                return Some("network connection is metered".into());
            }
        }
    }
    None
}

fn flatpak_updates() -> i64 {
    let mut total = 0;
    for scope in ["--system", "--user"] {
        if let Some((true, output)) = run(
            "flatpak",
            &["remote-ls", "--updates", scope, "--columns=application"],
            Duration::from_secs(60),
        ) {
            total += output
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count() as i64;
        }
    }
    total
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn snapshot(
    result: &str,
    reason: Option<String>,
    output: String,
    image_ref: String,
    booted: String,
    staged: String,
    remote: String,
    flatpaks: i64,
) -> kyth_shared::system::update_status::UpdateSnapshot {
    snapshot_with_retry(
        result, reason, output, image_ref, booted, staged, remote, flatpaks, false,
    )
}

fn snapshot_with_retry(
    result: &str,
    reason: Option<String>,
    output: String,
    image_ref: String,
    booted: String,
    staged: String,
    remote: String,
    flatpaks: i64,
    retryable: bool,
) -> kyth_shared::system::update_status::UpdateSnapshot {
    kyth_shared::system::update_status::UpdateSnapshot {
        result: result.into(),
        reason,
        output,
        ts: now(),
        flatpak_updates: flatpaks,
        image_ref,
        booted_digest: booted,
        staged_digest: staged,
        remote_digest: remote,
        retryable,
    }
}

struct UpgradeResult {
    ok: bool,
    output: String,
    retryable: bool,
}

fn free_sysroot_bytes() -> Option<u64> {
    let output = run("df", &["-Pk", "/sysroot"], Duration::from_secs(5))?;
    free_bytes_from_df(&output.1)
}

fn free_bytes_from_df(output: &str) -> Option<u64> {
    output
        .lines()
        .last()?
        .split_whitespace()
        .nth(3)?
        .parse::<u64>()
        .ok()
        .map(|value| value.saturating_mul(1024))
}

fn retryable_upgrade_output(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("temporar")
        || lower.contains("try again")
        || lower.contains("not enough space")
        || lower.contains("no space left")
}

fn run_bootc_upgrade(timeout: Duration) -> UpgradeResult {
    if let Some(free) = free_sysroot_bytes() {
        const REQUIRED: u64 = 2 * 1024 * 1024 * 1024;
        if free < REQUIRED {
            return UpgradeResult {
                ok: false,
                output: format!(
                    "Not enough free disk space: {} MiB free, need 2048 MiB",
                    free / (1024 * 1024)
                ),
                retryable: true,
            };
        }
    }
    let Ok(lock) = OpenOptions::new()
        .create(true)
        .write(true)
        .open("/run/kyth-bootc.lock")
    else {
        return UpgradeResult {
            ok: false,
            output: "Could not open the bootc upgrade lock".into(),
            retryable: true,
        };
    };
    if flock(&lock, FlockOperation::NonBlockingLockExclusive).is_err() {
        return UpgradeResult {
            ok: false,
            output: "Another bootc upgrade is in progress; will retry on the next run".into(),
            retryable: true,
        };
    }
    let argv = ["bootc", "upgrade"]
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    let result = kyth_shared::system::process::run_bounded(&argv, timeout);
    let _ = flock(&lock, FlockOperation::Unlock);
    match result {
        Ok(output) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .trim()
            .chars()
            .take(2000)
            .collect::<String>();
            UpgradeResult {
                ok: output.status.success(),
                retryable: !output.status.success() && retryable_upgrade_output(&text),
                output: text,
            }
        }
        Err(error) => UpgradeResult {
            ok: false,
            output: if error.kind() == std::io::ErrorKind::TimedOut {
                format!("bootc upgrade timed out after {}s", timeout.as_secs())
            } else {
                format!("bootc upgrade could not start: {error}")
            },
            retryable: error.kind() == std::io::ErrorKind::TimedOut,
        },
    }
}

fn notify_users(os_staged: bool, flatpaks: i64) {
    let Some((true, sessions)) = run(
        "loginctl",
        &["list-sessions", "--no-legend", "--no-pager"],
        Duration::from_secs(5),
    ) else {
        return;
    };
    let (title, body) = if os_staged {
        (
            "KythOS update ready",
            if flatpaks > 0 {
                format!("New OS image staged; {flatpaks} Flatpak update(s) pending. Click to open System Hub.")
            } else {
                "New OS image staged. Restart when ready. Click to open System Hub.".into()
            },
        )
    } else {
        (
            "App updates available",
            format!("{flatpaks} Flatpak update(s) available. Click to open System Hub."),
        )
    };
    for uid in sessions
        .lines()
        .filter_map(|line| line.split_whitespace().nth(2))
    {
        let Some((true, passwd)) = run("getent", &["passwd", uid], Duration::from_secs(2)) else {
            continue;
        };
        let Some(user) = passwd.split(':').next() else {
            continue;
        };
        let args = [
            "-u",
            user,
            "--",
            "notify-send",
            "--app-name=KythOS",
            "--icon=software-update-available",
            "--urgency=normal",
            "--action=default=View Updates",
            "--wait",
            title,
            &body,
        ];
        if run("runuser", &args, Duration::from_secs(65))
            .is_some_and(|(_, output)| output.trim() == "default")
        {
            let _ = run(
                "runuser",
                &[
                    "-u",
                    user,
                    "--",
                    "/usr/bin/kyth-welcome-launch",
                    "--page",
                    "Update",
                ],
                Duration::from_secs(10),
            );
        }
    }
}

fn main() -> std::process::ExitCode {
    if !rustix::process::getuid().is_root() {
        eprintln!("kyth-update-watcher must run as root");
        return std::process::ExitCode::from(1);
    }
    let config = config();
    if let Some(reason) = skip_reason(&config) {
        let status = snapshot(
            "skipped",
            Some(reason),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            0,
        );
        let _ = kyth_shared::system::update_status::write_update_snapshot(&status);
        return std::process::ExitCode::SUCCESS;
    }
    let flatpaks = flatpak_updates();
    let (firmware_updated, firmware_count, firmware_output) =
        kyth_shared::system::firmware::stage_firmware_batch();
    let status_data = kyth_shared::system::bootc_query::fetch_status_data();
    let Some(status_data) = status_data else {
        let status = snapshot(
            "error",
            Some("Could not read bootc status.".into()),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            flatpaks,
        );
        let _ = kyth_shared::system::update_status::write_update_snapshot(&status);
        return std::process::ExitCode::from(1);
    };
    let image_ref = kyth_shared::system::bootc_query::image_reference_from_status(&status_data)
        .unwrap_or_default();
    let booted = kyth_shared::system::bootc_query::image_digest_from_status(&status_data, "booted")
        .unwrap_or_default();
    let staged = kyth_shared::system::bootc_query::image_digest_from_status(&status_data, "staged")
        .unwrap_or_default();
    let branch = kyth_shared::system::bootc_policy::branch_from_ref(Some(&image_ref))
        .unwrap_or_else(|| "latest".into());
    let ring = string_setting(&config, "rollout_ring", "follow-image");
    if let Some(reason) = kyth_shared::system::boot_health::rollout_policy_reason(&image_ref, ring)
    {
        let status = snapshot(
            "skipped",
            Some(reason),
            String::new(),
            image_ref,
            booted,
            staged,
            String::new(),
            flatpaks,
        );
        let _ = kyth_shared::system::update_status::write_update_snapshot(&status);
        return std::process::ExitCode::SUCCESS;
    }
    let check = kyth_shared::system::registry::check_registry_update_with_timeout(
        &status_data,
        &branch,
        kyth_shared::system::bootc_policy::REGISTRY,
        Duration::from_secs(30),
    );
    let remote = kyth_shared::system::registry::remote_digest_and_timestamp(&check.manifest_raw)
        .0
        .unwrap_or_default();
    if !remote.is_empty() {
        if let Some(reason) = kyth_shared::system::boot_health::quarantine_reason(
            &kyth_shared::system::boot_health::read_default_state(),
            &remote,
        ) {
            let status = snapshot(
                "quarantined",
                Some(reason),
                String::new(),
                image_ref,
                booted,
                staged,
                remote,
                flatpaks,
            );
            let _ = kyth_shared::system::update_status::write_update_snapshot(&status);
            return std::process::ExitCode::SUCCESS;
        }
    }
    if check.state == "uptodate" || (!staged.is_empty() && staged == remote) {
        let status = if firmware_updated {
            snapshot(
                "upgraded",
                None,
                if firmware_output.is_empty() {
                    format!("{firmware_count} firmware update(s) queued; reboot to flash")
                } else {
                    firmware_output
                },
                image_ref,
                booted,
                staged,
                remote,
                flatpaks,
            )
        } else {
            let detail = if firmware_output.is_empty() {
                check.detail
            } else {
                format!("{}\n{}", check.detail, firmware_output)
            };
            snapshot(
                "no_change",
                Some("Already up to date".into()),
                detail,
                image_ref,
                booted,
                staged,
                remote,
                flatpaks,
            )
        };
        let _ = kyth_shared::system::update_status::write_update_snapshot(&status);
        if firmware_updated {
            notify_users(true, flatpaks);
        } else if flatpaks > 0 {
            notify_users(false, flatpaks);
        }
        return std::process::ExitCode::SUCCESS;
    }
    if check.state != "available" {
        let detail = if firmware_output.is_empty() {
            check.detail.clone()
        } else {
            format!("{}\n{}", check.detail, firmware_output)
        };
        let status = snapshot(
            "error",
            Some(check.detail.clone()),
            detail,
            image_ref,
            booted,
            staged,
            remote,
            flatpaks,
        );
        let _ = kyth_shared::system::update_status::write_update_snapshot(&status);
        return std::process::ExitCode::from(1);
    }
    let timeout = integer_setting(&config, "bootc_timeout", 1800).max(1) as u64;
    let upgrade = run_bootc_upgrade(Duration::from_secs(timeout));
    if !upgrade.ok {
        let output = if firmware_output.is_empty() {
            upgrade.output.clone()
        } else {
            format!("{}\n{}", upgrade.output, firmware_output)
        };
        let status = snapshot_with_retry(
            "error",
            Some(if upgrade.retryable {
                format!("retryable: {}", upgrade.output)
            } else {
                upgrade.output.clone()
            }),
            output,
            image_ref,
            booted,
            staged,
            remote,
            flatpaks,
            upgrade.retryable,
        );
        let _ = kyth_shared::system::update_status::write_update_snapshot(&status);
        return std::process::ExitCode::from(1);
    }
    if !remote.is_empty() {
        let coordinator = kyth_shared::system::update_coordinator::UpdateCoordinator::new(
            kyth_shared::system::boot_health::DEFAULT_STATE_PATH,
        );
        let _ = coordinator.record_staged(
            &remote,
            kyth_shared::system::boot_health::image_ring(&image_ref).unwrap_or(ring),
            now(),
        );
    }
    let output = if firmware_output.is_empty() {
        upgrade.output
    } else {
        format!("{}\n{}", upgrade.output, firmware_output)
    };
    let status = snapshot(
        "upgraded",
        None,
        output,
        image_ref,
        booted,
        remote.clone(),
        remote,
        flatpaks,
    );
    let _ = kyth_shared::system::update_status::write_update_snapshot(&status);
    notify_users(true, flatpaks);
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_df_available_bytes() {
        assert_eq!(free_bytes_from_df("Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/root 100 40 60 40% /sysroot\n"), Some(60 * 1024));
    }

    #[test]
    fn classifies_only_transient_upgrade_failures_as_retryable() {
        assert!(retryable_upgrade_output("bootc upgrade timed out"));
        assert!(retryable_upgrade_output("No space left on device"));
        assert!(!retryable_upgrade_output("signature verification failed"));
    }
}
