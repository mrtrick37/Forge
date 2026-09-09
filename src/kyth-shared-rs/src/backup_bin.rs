//! Native replacement for the Python `kyth-backup` launcher.
//!
//! Restic backup of `/home` plus best-effort btrfs send to the first USB
//! target and rclone sync, with the same battery gates. Timeouts match the
//! launcher (10/120/300/120s). Always exits `0`. `backup_preset.py`
//! stays as the Phase 3 fixture.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use kyth_shared::system::backup_config::{config_path, load, on_battery};
use kyth_shared::system::process::run_bounded;

fn run(argv: &[&str], timeout_secs: u64) {
    let argv: Vec<String> = argv.iter().map(|part| (*part).to_string()).collect();
    let _ = run_bounded(&argv, Duration::from_secs(timeout_secs));
}

fn first_usb_dir() -> Option<PathBuf> {
    let mut groups: Vec<PathBuf> = std::fs::read_dir("/run/media")
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .collect()
        })
        .unwrap_or_default();
    groups.sort();
    for group in &groups {
        let mut children: Vec<PathBuf> = std::fs::read_dir(group)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                    .collect()
            })
            .unwrap_or_default();
        children.sort();
        if let Some(first) = children.into_iter().find(|child| child.is_dir()) {
            return Some(first);
        }
    }
    None
}

fn main() -> std::process::ExitCode {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let config = load(config_path(None::<PathBuf>));
    if !config.on_battery && on_battery() {
        println!("kyth-backup: on battery, skipping btrfs send");
    }
    let repo = PathBuf::from(&config.repo);
    let _ = std::fs::create_dir_all(&repo);
    if !repo.join("config").exists() {
        run(&["restic", "init", "--repo", &repo.to_string_lossy()], 10);
    }
    run(
        &[
            "restic",
            "--repo",
            &repo.to_string_lossy(),
            "backup",
            &home.to_string_lossy(),
        ],
        120,
    );
    if config.btrfs_send && !on_battery() {
        if let Some(usb) = first_usb_dir() {
            run(
                &["btrfs", "send", "-p", "/home", &usb.to_string_lossy()],
                300,
            );
        }
    }
    if !config.remote.is_empty() && home.join(".config/rclone/rclone.conf").exists() {
        run(
            &["rclone", "sync", &repo.to_string_lossy(), &config.remote],
            120,
        );
    }
    println!("kyth-backup: repo {}", repo.display());
    std::process::ExitCode::SUCCESS
}
