//! Native post-update confidence check used by the diagnostic recipe.
//!
//! This is intentionally a small, read-only CLI. Collection is bounded and
//! interpretation is delegated to `system::runtime_diagnostics`, so the Hub
//! and this compatibility-named utility share the same Rust evidence rules.

use std::time::Duration;

fn run(program: &str, args: &[&str], timeout: u64) -> Option<(bool, String)> {
    let argv = std::iter::once(program.to_string())
        .chain(args.iter().map(|arg| (*arg).to_string()))
        .collect::<Vec<_>>();
    let output =
        kyth_shared::system::process::run_bounded(&argv, Duration::from_secs(timeout)).ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Some((
        output.status.success(),
        kyth_shared::system::process::redact_sensitive_text(text.trim()),
    ))
}

fn command_exists(program: &str) -> bool {
    let directories = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    directories
        .into_iter()
        .any(|directory| directory.join(program).is_file())
}

fn deployment_id() -> String {
    let ostree = run("ostree", &["admin", "status"], 8).map(|(_, text)| text);
    let bootc = run("bootc", &["status", "--json"], 8).map(|(_, text)| text);
    kyth_shared::system::runtime_diagnostics::deployment_id(
        ostree.as_deref(),
        bootc.as_deref(),
        &run("uname", &["-r"], 3)
            .map(|(_, text)| text)
            .unwrap_or_else(|| "unknown".into()),
    )
}

fn notify(title: &str, body: &str) {
    let _ = run("notify-send", &["--app-name=KythOS", title, body], 5);
}

fn main() -> std::process::ExitCode {
    if kyth_shared::system::process::is_live_session() {
        return std::process::ExitCode::SUCCESS;
    }
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let force = args.iter().any(|arg| arg == "--force");
    let no_notify = args.iter().any(|arg| arg == "--no-notify");
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/root".into());
    if !force && !home.join(".config/kyth-welcome-done").is_file() {
        return std::process::ExitCode::SUCCESS;
    }

    let deployment = deployment_id();
    let safe_deployment = deployment
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let marker = home
        .join(".local/share/kyth")
        .join(format!("post-update-check-{safe_deployment}"));
    if !force && marker.is_file() {
        return std::process::ExitCode::SUCCESS;
    }

    let mut failures = Vec::new();
    let mut warnings = Vec::new();

    let ostree_available = command_exists("ostree");
    let ostree_output =
        run("ostree", &["admin", "status"], 8).and_then(|(ok, text)| ok.then_some(text));
    let bootc_available = command_exists("bootc");
    let bootc_result = run("bootc", &["status"], 8).map(|(ok, _)| ok);
    let (deployment_failures, deployment_warnings) =
        kyth_shared::system::runtime_diagnostics::deployment_rollback(
            ostree_available,
            ostree_output.as_deref(),
            bootc_available,
            bootc_result,
        );
    failures.extend(deployment_failures);
    warnings.extend(deployment_warnings);

    let gpu_line = run("lspci", &[], 8).and_then(|(ok, text)| {
        ok.then(|| {
            text.lines()
                .find(|line| {
                    let lower = line.to_ascii_lowercase();
                    lower.contains("vga")
                        || lower.contains("3d controller")
                        || lower.contains("display controller")
                })
                .unwrap_or_default()
                .to_string()
        })
    });
    let modules_text = std::fs::read_to_string("/proc/modules").unwrap_or_default();
    let modules = modules_text
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect::<Vec<_>>();
    let gpu = kyth_shared::system::runtime_diagnostics::gpu_detected_check(
        command_exists("lspci"),
        gpu_line.as_deref(),
    );
    if !gpu.passed {
        failures.push(gpu.detail);
    }
    if let Some(line) = gpu_line {
        let drivers = kyth_shared::system::runtime_diagnostics::driver_check(&line, modules);
        if !drivers.passed {
            failures.push(drivers.detail);
        }
    }

    let vulkan = if !command_exists("vulkaninfo") {
        kyth_shared::system::runtime_diagnostics::vulkan_check(
            false,
            false,
            false,
            "Vulkan probe failed",
        )
    } else {
        match run("vulkaninfo", &["--summary"], 20) {
            Some((ok, output)) => kyth_shared::system::runtime_diagnostics::vulkan_check(
                true,
                false,
                ok,
                if output.is_empty() {
                    "Vulkan probe failed"
                } else {
                    &output
                },
            ),
            None => kyth_shared::system::runtime_diagnostics::vulkan_check(
                true,
                true,
                false,
                "Vulkan probe failed",
            ),
        }
    };
    if !vulkan.passed {
        warnings.push(vulkan.detail);
    }

    if !command_exists("flatpak") {
        failures.push("Flatpak is missing.".into());
    }
    for unit in ["pipewire.service", "wireplumber.service"] {
        let active = run("systemctl", &["--user", "is-active", unit], 5)
            .is_some_and(|(ok, output)| ok && output.trim() == "active");
        if !active {
            warnings.push(format!("{unit} is not active in this login session."));
        }
    }

    let status = if failures.is_empty() && warnings.is_empty() {
        "ready"
    } else if failures.is_empty() {
        "review recommended"
    } else {
        "needs attention"
    };
    let detail = failures
        .iter()
        .chain(warnings.iter())
        .take(5)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    println!("KythOS Post-Update Check: {status}");
    for failure in &failures {
        println!("FAIL  {failure}");
    }
    for warning in &warnings {
        println!("WARN  {warning}");
    }
    if !no_notify && (force || !failures.is_empty() || !warnings.is_empty()) {
        notify(
            &format!("KythOS Post-Update Check: {status}"),
            if detail.is_empty() {
                "Update looks healthy."
            } else {
                &detail
            },
        );
    }
    if failures.is_empty() {
        let _ = kyth_shared::atomic_io::atomic_write_text(&marker, "", Some(0o644));
    }
    if failures.is_empty() && warnings.is_empty() {
        std::process::ExitCode::SUCCESS
    } else if failures.is_empty() {
        std::process::ExitCode::from(1)
    } else {
        std::process::ExitCode::from(2)
    }
}
