//! Native replacement for the Python `kyth-dynamic-lock` launcher.
//!
//! Polls KDE Connect every 5 seconds and locks the Plasma session once a
//! trusted device has been missing for the grace period. A broken
//! availability query never counts as the device leaving. Exits `0` on
//! SIGINT/SIGTERM. The launcher was self-contained; there is no shared
//! Python fixture.

use std::env;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use kyth_shared::system::dynamic_lock::{
    config_path, load_config, Availability, Monitor, POLL_SECONDS, RUNNING,
};
use kyth_shared::system::process::run_bounded;

extern "C" fn stop(_signum: libc::c_int) {
    RUNNING.store(false, Ordering::SeqCst);
}

fn on_path(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

fn run_quiet(argv: &[String], timeout_secs: u64) -> Option<(i32, String)> {
    run_bounded(argv, Duration::from_secs(timeout_secs))
        .ok()
        .map(|output| {
            (
                output.status.code().unwrap_or(1),
                String::from_utf8_lossy(&output.stdout).into_owned(),
            )
        })
}

fn available_ids() -> Option<std::collections::HashSet<String>> {
    if !on_path("kdeconnect-cli") {
        return None;
    }
    let argv = ["kdeconnect-cli", "--list-available", "--id-only"]
        .iter()
        .map(|part| (*part).to_string())
        .collect::<Vec<_>>();
    match run_quiet(&argv, 12) {
        Some((0, stdout)) => Some(
            stdout
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect(),
        ),
        _ => None,
    }
}

fn lock_session() -> bool {
    const COMMANDS: &[&[&str]] = &[
        &[
            "qdbus6",
            "org.freedesktop.ScreenSaver",
            "/ScreenSaver",
            "Lock",
        ],
        &[
            "qdbus-qt6",
            "org.freedesktop.ScreenSaver",
            "/ScreenSaver",
            "Lock",
        ],
        &[
            "qdbus",
            "org.freedesktop.ScreenSaver",
            "/ScreenSaver",
            "Lock",
        ],
        &["loginctl", "lock-session"],
    ];
    for command in COMMANDS {
        if !on_path(command[0]) {
            continue;
        }
        let argv: Vec<String> = command.iter().map(|part| (*part).to_string()).collect();
        if run_quiet(&argv, 10).is_some_and(|(code, _)| code == 0) {
            return true;
        }
    }
    false
}

fn main() -> std::process::ExitCode {
    unsafe {
        libc::signal(libc::SIGINT, stop as libc::sighandler_t);
        libc::signal(libc::SIGTERM, stop as libc::sighandler_t);
    }
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let override_path = env::var("KYTH_DYNAMIC_LOCK_CONFIG").ok();
    let path = config_path(&home, override_path.as_deref());
    let started = Instant::now();
    let mut monitor = Monitor::default();
    while RUNNING.load(Ordering::SeqCst) {
        let config = load_config(&path);
        let availability = if config.enabled {
            match available_ids() {
                Some(ids) => Availability::Present(ids.contains(&config.device_id)),
                None => Availability::Unavailable,
            }
        } else {
            Availability::Unavailable
        };
        if monitor.step(&config, availability, started.elapsed().as_secs_f64()) {
            lock_session();
        }
        std::thread::sleep(Duration::from_secs(POLL_SECONDS));
    }
    std::process::ExitCode::SUCCESS
}
