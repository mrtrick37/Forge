//! Native read-only suspend/resume readiness check.

use kyth_shared::system::smoke_check::{Level, Report};
use kyth_shared::system::{
    process::run_bounded,
    runtime_diagnostics::{gpu_detected_check, login_session_check, vulkan_check},
};
use std::path::Path;
use std::time::Duration;

fn available(program: &str) -> bool {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .any(|p| p.join(program).is_file())
}
fn probe(program: &str, args: &[&str], timeout: u64) -> Option<std::process::Output> {
    if !available(program) {
        return None;
    }
    let mut argv = vec![program.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    run_bounded(&argv, Duration::from_secs(timeout)).ok()
}
fn add(report: &mut Report, level: &str, name: &str, detail: impl Into<String>) {
    let level = match level {
        "PASS" => Level::Pass,
        "FAIL" => Level::Fail,
        _ => Level::Warn,
    };
    report.record(level, name, detail, "Resume Readiness");
}
fn main() {
    let mut report = Report::default();
    let login = login_session_check(
        available("loginctl"),
        probe("loginctl", &["list-sessions", "--no-legend"], 5).is_some_and(|o| o.status.success()),
    );
    add(
        &mut report,
        if login.passed { "PASS" } else { "WARN" },
        &login.label,
        login.detail,
    );
    match probe("nmcli", &["-t", "-f", "STATE", "general"], 5) {
        Some(output)
            if output.status.success()
                && String::from_utf8_lossy(&output.stdout).starts_with("connected") =>
        {
            add(
                &mut report,
                "PASS",
                "Network",
                String::from_utf8_lossy(&output.stdout).trim(),
            )
        }
        Some(output) => add(
            &mut report,
            "WARN",
            "Network",
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ),
        None => add(&mut report, "WARN", "Network", "nmcli unavailable"),
    }
    for (unit, user) in [
        ("pipewire.service", true),
        ("wireplumber.service", true),
        ("bluetooth.service", false),
    ] {
        let args = if user {
            vec!["--user", "is-active", "--quiet", unit]
        } else {
            vec!["is-active", "--quiet", unit]
        };
        let active = probe("systemctl", &args, 5).is_some_and(|o| o.status.success());
        add(
            &mut report,
            if active { "PASS" } else { "WARN" },
            unit.trim_end_matches(".service"),
            if active { "active" } else { "not active" },
        );
    }
    let bt = Path::new("/sys/class/bluetooth")
        .read_dir()
        .ok()
        .map(|entries| {
            entries
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("hci"))
        })
        .unwrap_or(false);
    add(
        &mut report,
        if bt { "PASS" } else { "WARN" },
        "Bluetooth adapter",
        if bt {
            "controller present"
        } else {
            "no hci controller visible"
        },
    );
    let gpu_lines = kyth_shared::system::gpu::lspci_gpu_lines();
    let gpu = gpu_detected_check(available("lspci"), gpu_lines.first().map(String::as_str));
    add(
        &mut report,
        if gpu.passed { "PASS" } else { "WARN" },
        &gpu.label,
        gpu.detail,
    );
    let vk = probe("vulkaninfo", &["--summary"], 10);
    let vk_check = vulkan_check(
        available("vulkaninfo"),
        vk.is_none() && available("vulkaninfo"),
        vk.as_ref().is_some_and(|o| o.status.success()),
        "failed after resume",
    );
    add(
        &mut report,
        if vk_check.passed { "PASS" } else { "WARN" },
        &vk_check.label,
        vk_check.detail,
    );
    let displays = probe("kscreen-doctor", &["-o"], 5);
    let count = displays
        .as_ref()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|line| line.contains(" connected"))
                .count()
        })
        .unwrap_or(0);
    add(
        &mut report,
        if count > 0 { "PASS" } else { "WARN" },
        "Displays",
        if count > 0 {
            format!("{count} connected output(s)")
        } else {
            "kscreen-doctor unavailable or no connected output reported".into()
        },
    );
    let journal = probe(
        "journalctl",
        &["-b", "--since", "-10 minutes", "-p", "err", "--no-pager"],
        10,
    );
    let errors = journal
        .as_ref()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|line| {
                    let lower = line.to_ascii_lowercase();
                    [
                        "amdgpu",
                        "nvidia",
                        "i915",
                        "xe",
                        "bluetooth",
                        "networkmanager",
                        "pipewire",
                        "wireplumber",
                        "kwin",
                    ]
                    .iter()
                    .any(|key| lower.contains(key))
                })
                .count()
        })
        .unwrap_or(0);
    add(
        &mut report,
        if errors == 0 && journal.is_some() {
            "PASS"
        } else {
            "WARN"
        },
        "Recent critical logs",
        if journal.is_none() {
            "journalctl unavailable or query failed".into()
        } else {
            format!("{errors} matching error(s) in last 10 minutes")
        },
    );
    println!("KythOS Resume Check\n");
    for row in &report.results {
        println!(
            "{:5} {:24} {}",
            format!("{:?}", row.level).to_uppercase(),
            row.name,
            row.detail
        );
    }
    println!(
        "\nResult: {}",
        if report.failures() > 0 {
            "resume readiness has failures"
        } else if report.warnings() > 0 {
            "resume readiness has warnings"
        } else {
            "resume readiness is clean"
        }
    );
    std::process::exit(report.exit_code_with_strict(true));
}
