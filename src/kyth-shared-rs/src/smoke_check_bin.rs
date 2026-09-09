//! Native read-only daily-driver smoke check.

use std::path::Path;
use std::time::Duration;

use kyth_shared::system::process::run_bounded;
use kyth_shared::system::smoke_check::{command_available, path_check, Report};

fn command_ok(program: &str, args: &[&str]) -> bool {
    if !std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .any(|p| p.join(program).is_file())
    {
        return false;
    }
    let mut argv = vec![program.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    run_bounded(&argv, Duration::from_secs(10))
        .map(|out| out.status.success())
        .unwrap_or(false)
}
fn collect() -> Report {
    let mut report = Report::default();
    let section = "Identity And Update Safety";
    let os_release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    if os_release.lines().any(|line| line == "ID=kythos") {
        report.passed(
            "OS identity ID",
            "boot metadata uses KythOS identity",
            section,
        );
    } else {
        report.failed(
            "OS identity ID",
            "/etc/os-release does not advertise KythOS",
            section,
        );
    }
    let bootc = command_available("bootc", "/usr/bin", "bootc", false, section);
    report.record(bootc.level, bootc.name, bootc.detail, bootc.section);
    for (unit, label) in [
        ("display-manager.service", "Login manager"),
        ("NetworkManager.service", "Network manager"),
        ("bluetooth.service", "Bluetooth"),
    ] {
        let row = if command_ok("systemctl", &["is-active", "--quiet", unit]) {
            ("PASS", "active")
        } else if Path::new("/run/systemd/system").exists() {
            ("WARN", "not active or unavailable")
        } else {
            ("WARN", "systemd unavailable")
        };
        report.record(
            match row.0 {
                "PASS" => kyth_shared::system::smoke_check::Level::Pass,
                _ => kyth_shared::system::smoke_check::Level::Warn,
            },
            label,
            row.1,
            section,
        );
    }

    let section = "Desktop Familiarity";
    let session =
        std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "no graphical session".into());
    if session.eq_ignore_ascii_case("wayland") {
        report.passed("Session type", session, section);
    } else if session.eq_ignore_ascii_case("x11") {
        report.failed(
            "Session type",
            "X11 — KythOS ships Plasma Wayland only",
            section,
        );
    } else {
        report.warned("Session type", session, section);
    }
    for (path, label) in [
        ("/etc/plymouth/plymouthd.conf", "Plymouth configuration"),
        ("/etc/xdg/mimeapps.list", "Default app associations"),
    ] {
        let row = path_check(path, label, false, false, section);
        report.record(row.level, row.name, row.detail, row.section);
    }

    let section = "Hardware And Drivers";
    for (program, label, optional) in [
        ("lspci", "PCI hardware probe", false),
        ("vulkaninfo", "Vulkan", true),
        ("vainfo", "VA-API", true),
    ] {
        let row = command_available(program, "/usr/bin", label, optional, section);
        report.record(row.level, row.name, row.detail, row.section);
    }
    let input = path_check("/dev/input", "Input & Gamepads", false, false, section);
    report.record(input.level, input.name, input.detail, input.section);

    let section = "Gaming Stack";
    for (program, label) in [
        ("gamemoderun", "GameMode wrapper"),
        ("gamescope", "Gamescope"),
        ("umu-run", "umu launcher"),
    ] {
        let row = command_available(program, "/usr/bin", label, true, section);
        report.record(row.level, row.name, row.detail, row.section);
    }
    report
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|arg| arg == "--json");
    let strict = args.iter().any(|arg| arg == "--strict");
    let report = collect();
    if json {
        let health = kyth_shared::health::from_smoke_report(
            kyth_shared::health::HealthReport::create([]).generated_at,
            &report,
        );
        println!("{}", health.to_json().expect("health report serializes"));
    } else {
        println!("KythOS Smoke Check");
        for row in &report.results {
            println!(
                "{:5} {:34} {}",
                format!("{:?}", row.level).to_uppercase(),
                row.name,
                row.detail
            );
        }
        println!(
            "\n== Summary ==\nPASS: {}\nWARN: {}\nFAIL: {}",
            report
                .results
                .iter()
                .filter(|r| matches!(r.level, kyth_shared::system::smoke_check::Level::Pass))
                .count(),
            report.warnings(),
            report.failures()
        );
    }
    std::process::exit(report.exit_code_with_strict(strict));
}
