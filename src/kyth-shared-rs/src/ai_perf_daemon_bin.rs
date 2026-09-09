//! Native replacement for the former `kyth_shared.ai_perf_daemon` launcher.
//!
//! Sampling is deliberately conservative and best-effort: policy selection is
//! deterministic, while missing desktop services, hardware files, or write
//! permissions leave the daemon alive and let the next TTL-bound cycle retry.

use std::env;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kyth_shared::system::gaming_activity::{active_uids_from_loginctl, is_gaming_process};
use kyth_shared::system::perf_policy::{
    battery_percent, choose_policy, power_profile, pressure_avg10, PerfPolicy, PerfSample,
};
use kyth_shared::system::process::run_bounded;

const LOOP_INTERVAL: Duration = Duration::from_secs(5);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);
const SCX_LOADER_CONF: &str = "/etc/scx/scx_loader.conf";
const SYSCTL_CONF: &str = "/etc/sysctl.d/99-kyth-ai.conf";
const TTL_MARKER: &str = "/run/kyth-ai-perfd-ttl";
const SCX_MARKER: &str = "/run/kyth-ai-perfd-scx";

fn command(program: &str, args: &[&str], timeout: Duration) -> Option<(bool, String)> {
    let mut argv = vec![program.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    let output = run_bounded(&argv, timeout).ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Some((output.status.success(), text))
}

fn read_first(paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| fs::read_to_string(path).ok())
}

fn process_gaming_active() -> bool {
    let Ok(entries) = fs::read_dir("/proc") else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name().to_string_lossy().to_string();
        name.chars().all(|character| character.is_ascii_digit())
            && fs::read_link(entry.path().join("exe"))
                .ok()
                .and_then(|path| path.to_str().map(str::to_owned))
                .is_some_and(|path| is_gaming_process(&path))
    })
}

fn gamescope_session_active() -> bool {
    let current_uid = unsafe { libc::getuid() };
    let mut uids = vec![current_uid];
    if let Some((true, output)) = command(
        "loginctl",
        &["list-sessions", "--no-legend", "--no-pager"],
        Duration::from_secs(5),
    ) {
        uids.extend(active_uids_from_loginctl(&output));
    }
    uids.sort_unstable();
    uids.dedup();
    uids.into_iter().any(|uid| {
        let path = format!("/run/user/{uid}/gamescope-session.lock");
        Path::new(&path).is_file()
    })
}

fn detect_gaming() -> bool {
    // Preserve the Python daemon's fast process scan before the more costly
    // session lookup.  Gamescope is checked for all visible user sessions.
    process_gaming_active() || gamescope_session_active()
}

fn hardware_caps() -> (bool, bool) {
    if let Ok(evaluation) = kyth_shared::system::hardware_policy::evaluate_system() {
        let has_nvidia = evaluation
            .capabilities
            .iter()
            .any(|cap| cap == "gpu.nvidia");
        let has_amd = evaluation.capabilities.iter().any(|cap| cap == "gpu.amd");
        return (has_nvidia, has_amd);
    }

    // A missing policy file must not turn into a false healthy state.  These
    // module markers are a bounded fallback for the daemon's GPU hint only.
    (
        Path::new("/sys/module/nvidia").exists(),
        Path::new("/sys/module/amdgpu").exists(),
    )
}

fn collect_sample() -> PerfSample {
    let (has_nvidia, has_amd) = hardware_caps();
    let pressure = read_first(&["/proc/pressure/cpu", "/sys/fs/cgroup/cpu.pressure"])
        .and_then(|text| pressure_avg10(&text))
        .unwrap_or(0.0);
    let battery = [
        "/sys/class/power_supply/BAT0/capacity",
        "/sys/class/power_supply/BAT1/capacity",
    ]
    .iter()
    .find_map(|path| {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| battery_percent(&text))
    });
    let profile = command("powerprofilesctl", &["get"], COMMAND_TIMEOUT)
        .map(|(success, output)| power_profile(success, &output))
        .unwrap_or_else(|| "unknown".into());

    PerfSample {
        is_gaming: detect_gaming(),
        pressure_some_avg10: pressure,
        power_profile: profile,
        battery_percent: battery,
        has_nvidia,
        has_amd,
        hdr_active: false,
    }
}

fn write_policy(policy: &PerfPolicy) -> std::io::Result<()> {
    let scx = if policy.scx == "none" {
        format!(
            "# kyth-ai-perfd: no scx (ttl {}) reason: {}\n",
            policy.ttl, policy.reason
        )
    } else {
        format!(
            "SCX_SCHEDULER={}\n# reason: {} ttl {}\n",
            policy.scx, policy.reason, policy.ttl
        )
    };
    kyth_shared::atomic_io::atomic_write_text(SCX_LOADER_CONF, &scx, Some(0o644))?;

    let mut sysctl = format!(
        "# kyth-ai-perfd ttl {} reason: {}\n",
        policy.ttl, policy.reason
    );
    for (key, value) in &policy.sysctl {
        sysctl.push_str(&format!("{key} = {value}\n"));
    }
    kyth_shared::atomic_io::atomic_write_text(SYSCTL_CONF, &sysctl, Some(0o644))?;
    Ok(())
}

fn apply_sysctls(policy: &PerfPolicy) {
    for (key, value) in &policy.sysctl {
        let assignment = format!("{key}={value}");
        let _ = command("sysctl", &["-w", &assignment], COMMAND_TIMEOUT);
    }
}

fn apply_gpu_power(policy: &PerfPolicy) {
    if !matches!(policy.gpu_power.as_str(), "high" | "auto" | "low") {
        return;
    }
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("card") {
            continue;
        }
        let target = entry
            .path()
            .join("device/power_dpm_force_performance_level");
        if target.is_file() {
            let _ = fs::write(target, policy.gpu_power.as_bytes());
        }
    }
}

fn apply_policy(policy: &PerfPolicy) -> bool {
    if let Err(error) = write_policy(policy) {
        eprintln!("kyth-ai-perfd: policy files unavailable: {error}");
        return false;
    }
    apply_sysctls(policy);
    apply_gpu_power(policy);

    let expiry = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_add(policy.ttl);
    let _ = kyth_shared::atomic_io::atomic_write_text(TTL_MARKER, &expiry.to_string(), Some(0o644));
    let _ = kyth_shared::atomic_io::atomic_write_text(SCX_MARKER, &policy.scx, Some(0o644));
    true
}

fn cycle(print: bool) -> bool {
    let sample = collect_sample();
    let policy = choose_policy(&sample);
    if print {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({"sample": sample, "policy": policy}))
                .unwrap_or_else(|_| "{}".into())
        );
    }
    apply_policy(&policy)
}

fn usage() {
    eprintln!("usage: kyth-ai-perfd [--once] [--print]");
}

fn main() -> std::process::ExitCode {
    let mut once = false;
    let mut print = false;
    for argument in env::args().skip(1) {
        match argument.as_str() {
            "--once" => once = true,
            "--print" => print = true,
            "-h" | "--help" => {
                usage();
                return std::process::ExitCode::SUCCESS;
            }
            _ => {
                usage();
                return std::process::ExitCode::from(2);
            }
        }
    }

    if once {
        return if cycle(print) {
            std::process::ExitCode::SUCCESS
        } else {
            std::process::ExitCode::from(1)
        };
    }
    loop {
        let _ = cycle(false);
        std::thread::sleep(LOOP_INTERVAL);
    }
}
