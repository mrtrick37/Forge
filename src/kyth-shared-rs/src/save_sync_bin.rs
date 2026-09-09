//! Native replacement for the Python `kyth-save-sync` launcher.
//!
//! Keeps a local restic repo of Steam compat saves and syncs it to the
//! configured rclone remote when one is set and authenticated. Every step
//! is best-effort with the Python timeouts. Always exits `0`.
//! `save_cloud.py` stays as the Phase 3 fixture.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use kyth_shared::system::process::run_bounded;
use kyth_shared::system::save_cloud::{compat_drive_cs, config_path, load};

fn run(argv: &[&str], timeout_secs: u64) {
    let argv: Vec<String> = argv.iter().map(|part| (*part).to_string()).collect();
    let _ = run_bounded(&argv, Duration::from_secs(timeout_secs));
}

fn main() -> std::process::ExitCode {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let config = load(config_path(None::<PathBuf>));
    let repo = PathBuf::from(&config.repo);
    let _ = std::fs::create_dir_all(&repo);
    if !repo.join("config").exists() {
        run(&["restic", "init", "--repo", &repo.to_string_lossy()], 10);
    }
    for compat in compat_drive_cs(&home) {
        run(
            &[
                "restic",
                "--repo",
                &repo.to_string_lossy(),
                "backup",
                &compat.to_string_lossy(),
            ],
            60,
        );
    }
    let remote = config.remote.clone();
    if !remote.is_empty() && home.join(".config/rclone/rclone.conf").exists() {
        run(&["rclone", "sync", &repo.to_string_lossy(), &remote], 120);
    }
    let remote = if remote.is_empty() {
        "none".to_string()
    } else {
        remote
    };
    println!("kyth-save-sync: repo {} remote {remote}", repo.display());
    std::process::ExitCode::SUCCESS
}
