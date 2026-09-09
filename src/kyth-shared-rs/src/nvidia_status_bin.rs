//! Native read-only NVIDIA readiness report.

use kyth_shared::system::smoke_check::{Level, Report};
use kyth_shared::system::{gpu::lspci_gpu_lines, process::run_bounded};
use std::time::Duration;

fn run(program: &str, args: &[&str]) -> Option<std::process::Output> {
    let mut argv = vec![program.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    run_bounded(&argv, Duration::from_secs(10)).ok()
}
fn add(report: &mut Report, level: Level, name: &str, detail: impl Into<String>) {
    report.record(level, name, detail, "NVIDIA Status");
}
fn main() {
    let mut report = Report::default();
    let gpu = lspci_gpu_lines()
        .into_iter()
        .find(|line| line.to_ascii_lowercase().contains("nvidia"));
    let Some(gpu) = gpu else {
        add(
            &mut report,
            Level::Pass,
            "NVIDIA hardware",
            "no NVIDIA GPU detected",
        );
        println!("NVIDIA Status\n\nPASS  NVIDIA hardware  no NVIDIA GPU detected\n\nResult: no NVIDIA-specific work needed.");
        return;
    };
    add(&mut report, Level::Pass, "NVIDIA hardware", gpu.trim());
    match run("rpm", &["-q", "akmod-nvidia"]) {
        Some(output) if output.status.success() => {
            add(&mut report, Level::Pass, "akmod-nvidia", "installed")
        }
        _ => add(
            &mut report,
            Level::Fail,
            "akmod-nvidia",
            "missing from image",
        ),
    }
    if run("modinfo", &["nvidia"]).is_some_and(|output| output.status.success()) {
        add(
            &mut report,
            Level::Pass,
            "Kernel module built",
            "modinfo nvidia works",
        );
    } else {
        add(
            &mut report,
            Level::Warn,
            "Kernel module built",
            "not built for current kernel yet",
        );
    }
    let loaded = std::fs::read_to_string("/proc/modules")
        .map(|text| text.lines().any(|line| line.starts_with("nvidia ")))
        .unwrap_or(false);
    add(
        &mut report,
        if loaded { Level::Pass } else { Level::Warn },
        "Kernel module loaded",
        if loaded {
            "nvidia loaded"
        } else {
            "not loaded; reboot may be required after build"
        },
    );
    if let Some(output) = run("nvidia-smi", &[]) {
        if output.status.success() {
            add(
                &mut report,
                Level::Pass,
                "nvidia-smi",
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .next()
                    .unwrap_or("nvidia-smi works"),
            );
        } else {
            add(
                &mut report,
                Level::Warn,
                "nvidia-smi",
                "command failed or no GPU output",
            );
        }
    } else {
        add(
            &mut report,
            Level::Warn,
            "nvidia-smi",
            "not available until proprietary driver is active",
        );
    }
    println!("NVIDIA Status\n");
    for row in &report.results {
        println!(
            "{:5} {:24} {}",
            format!("{:?}", row.level).to_uppercase(),
            row.name,
            row.detail
        );
    }
    println!(
        "\nResult: NVIDIA setup {}.",
        if report.failures() > 0 {
            "needs attention"
        } else if report.warnings() > 0 {
            "needs attention or a reboot"
        } else {
            "is ready"
        }
    );
    std::process::exit(report.exit_code_with_strict(true));
}
