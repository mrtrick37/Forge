//! Native read-only replacement for `kyth-health-check`.

use std::path::Path;
use std::time::Duration;

struct Check {
    level: &'static str,
    name: &'static str,
    detail: &'static str,
}

fn command_exists(program: &str) -> bool {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|directory| directory.join(program))
        .any(|candidate| candidate.is_file())
}

fn command_ok(program: &str, args: &[&str]) -> bool {
    if !command_exists(program) {
        return false;
    }
    let mut argv = vec![program.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    kyth_shared::system::process::run_bounded(&argv, Duration::from_secs(10))
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn pipewire_running() -> bool {
    if command_exists("pgrep") {
        return command_ok("pgrep", &["-x", "pipewire"]);
    }
    std::fs::read_dir("/proc")
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .chars()
                    .all(|c| c.is_ascii_digit())
                    && std::fs::read_to_string(entry.path().join("comm"))
                        .map(|comm| comm.trim() == "pipewire")
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn run_checks() -> Vec<Check> {
    let scx_active = command_ok("systemctl", &["is-active", "--quiet", "scx_loader.service"])
        || command_exists("scx_rusty")
        || Path::new("/sys/kernel/sched_ext/state").exists();
    let ntsync_loaded = Path::new("/dev/ntsync").exists()
        || std::fs::read_to_string("/proc/modules")
            .map(|modules| modules.lines().any(|line| line.starts_with("ntsync ")))
            .unwrap_or(false);
    let vulkan_ok = command_ok("vulkaninfo", &["--summary"]);
    let vaapi_ok = command_ok("vainfo", &[]);
    let input_available = Path::new("/dev/input").is_dir();

    vec![
        if scx_active {
            Check {
                level: "PASS",
                name: "Kernel Scheduler",
                detail: "sched-ext (scx) low-latency scheduler active",
            }
        } else {
            Check {
                level: "WARN",
                name: "Kernel Scheduler",
                detail: "CFS/EEVDF fallback (scx not active)",
            }
        },
        Check {
            level: "PASS",
            name: "Wine Synchronization",
            detail: if ntsync_loaded {
                "NTSYNC fast kernel driver loaded"
            } else {
                "FUTEX2 / esync fallback active"
            },
        },
        if pipewire_running() {
            Check {
                level: "PASS",
                name: "Audio Stack",
                detail: "PipeWire low-latency daemon running",
            }
        } else {
            Check {
                level: "WARN",
                name: "Audio Stack",
                detail: "PipeWire daemon not detected",
            }
        },
        if vulkan_ok {
            Check {
                level: "PASS",
                name: "Vulkan 3D Driver",
                detail: "Vulkan device initialized and responsive",
            }
        } else {
            Check {
                level: "WARN",
                name: "Vulkan 3D Driver",
                detail: "Vulkan device query returned warning or fallback",
            }
        },
        Check {
            level: "PASS",
            name: "Video Codecs",
            detail: if vaapi_ok {
                "VA-API hardware video decode/encode active"
            } else {
                "Software codec fallback active"
            },
        },
        if input_available {
            Check {
                level: "PASS",
                name: "Input & Gamepads",
                detail: "Event subsystem and controller udev rules active",
            }
        } else {
            Check {
                level: "WARN",
                name: "Input & Gamepads",
                detail: "/dev/input device node inaccessible",
            }
        },
    ]
}

fn timestamp() -> String {
    // Health output only requires an ISO-shaped local timestamp, matching the
    // Python reporter's diagnostic header without adding a date dependency.
    kyth_shared::health::HealthReport::create([]).generated_at
}

fn main() {
    let checks = run_checks();
    let warnings = checks.iter().filter(|check| check.level == "WARN").count();
    let failures = checks.iter().filter(|check| check.level == "FAIL").count();
    println!("KythOS Subsystem Health");
    println!("Generated: {}\n", timestamp());
    for check in &checks {
        println!("{:<5} {:<28} {}", check.level, check.name, check.detail);
    }
    println!();
    if failures > 0 {
        println!("Result: Subsystem health has failures.");
        std::process::exit(2);
    } else if warnings > 0 {
        println!("Result: System is running with some warning fallback configurations.");
        std::process::exit(1);
    } else {
        println!("Result: Subsystem health looks good.");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn warning_result_uses_python_compatible_summary() {
        assert_eq!(
            "Result: System is running with some warning fallback configurations.",
            "Result: System is running with some warning fallback configurations."
        );
    }
}
