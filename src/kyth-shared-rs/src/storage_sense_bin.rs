//! Native replacement for the Python `kyth-storage-sense` launcher.
//!
//! Prunes trash older than 30 days, removes unused Flatpaks, and vacuums
//! user journals. Every step is best-effort. Always exits `0`.
//! `maintenance.py` stays as the Phase 3 fixture.

use std::env;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use kyth_shared::system::maintenance::{
    cleanup_flatpaks_command, prune_trash, vacuum_user_journal_command,
};

fn on_path(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

fn run_quiet(argv: &[String]) {
    let Some((program, args)) = argv.split_first() else {
        return;
    };
    let _ = std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
}

fn main() -> std::process::ExitCode {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|span| span.as_secs() as i64)
        .unwrap_or(0);
    prune_trash(&home, 30, now);
    if on_path("flatpak") {
        run_quiet(&cleanup_flatpaks_command());
    }
    if on_path("journalctl") {
        run_quiet(&vacuum_user_journal_command(30));
    }
    std::process::ExitCode::SUCCESS
}
