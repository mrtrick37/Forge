//! Native Guardian storage maintenance action.

use std::path::Path;
use std::time::Duration;

fn run(program: &str, args: &[&str], timeout: Duration) -> Result<std::process::Output, String> {
    let mut argv = vec![program.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    crate::system::process::run_bounded(&argv, timeout)
        .map_err(|error| format!("{program} could not run: {error}"))
}

fn pressure_low() -> bool {
    for path in ["/proc/pressure/cpu", "/sys/fs/cgroup/cpu.pressure"] {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        if let Some(value) = text
            .split_whitespace()
            .find_map(|part| part.strip_prefix("avg10=")?.parse::<f64>().ok())
        {
            return value < 20.0;
        }
    }
    true
}

fn on_ac() -> bool {
    let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") else {
        return true;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("BAT") {
            continue;
        }
        if std::fs::read_to_string(entry.path().join("status"))
            .is_ok_and(|status| status.trim() == "Discharging")
        {
            return false;
        }
    }
    true
}

fn gaming_active() -> bool {
    run(
        "pgrep",
        &[
            "-f",
            "kyth-game-boost|kyth-game-launch|gamemoderun|gamescope",
        ],
        Duration::from_secs(3),
    )
    .is_ok_and(|output| output.status.success())
}

fn scrub_active() -> bool {
    ["/", "/var", "/home"].iter().any(|mount| {
        run(
            "btrfs",
            &["scrub", "status", mount],
            Duration::from_secs(10),
        )
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains("running"))
    })
}

pub fn run_maintenance() -> Result<String, String> {
    if !pressure_low() {
        return Ok("Storage maintenance skipped: CPU pressure is high.".into());
    }
    if !on_ac() {
        return Ok("Storage maintenance skipped: system is on battery.".into());
    }
    if gaming_active() {
        return Ok("Storage maintenance skipped: a game is active.".into());
    }
    if scrub_active() {
        return Ok("Storage maintenance skipped: a scrub is already running.".into());
    }

    let mut failures = Vec::new();
    for mount in ["/", "/home", "/var"] {
        if Path::new(mount).exists()
            && !run(
                "btrfs",
                &["scrub", "start", "-B", mount],
                Duration::from_secs(3600),
            )
            .is_ok_and(|output| output.status.success())
        {
            failures.push(format!("scrub {mount}"));
        }
    }
    if !run(
        "btrfs",
        &["balance", "start", "-dusage=50", "-musage=50", "/"],
        Duration::from_secs(1800),
    )
    .is_ok_and(|output| output.status.success())
    {
        failures.push("balance /".into());
    }
    if failures.is_empty() {
        Ok("Storage maintenance complete.".into())
    } else {
        Ok(format!(
            "Storage maintenance completed with skipped operations: {}.",
            failures.join(", ")
        ))
    }
}
