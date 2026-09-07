//! Native replacement for the Python `kyth-duperemove` launcher.
//!
//! Scans `/var/home` for Steam compatdata/shadercache directories and
//! deduplicates each one on a btrfs/xfs filesystem via duperemove with a
//! private hash database. Best-effort throughout. Always exits `0`.
//! `maintenance.py` stays as the Phase 3 fixture.

use std::env;
use std::path::{Path, PathBuf};

use kyth_shared::system::maintenance::{
    DUPE_HASH_DIR, dedupe_command, find_dedupe_targets, secure_dedupe_hash_file,
    supports_dedupe_filesystem,
};

fn log(message: &str) {
    println!("kyth-duperemove: {message}");
}

fn on_path(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

fn fstype_of(target: &Path) -> Option<String> {
    let output = std::process::Command::new("findmnt")
        .args(["-no", "FSTYPE", "-T"])
        .arg(target)
        .output()
        .ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn main() -> std::process::ExitCode {
    if !on_path("duperemove") {
        return std::process::ExitCode::SUCCESS;
    }
    let targets = find_dedupe_targets("/var/home");
    if targets.is_empty() {
        log("no Steam compatdata/shadercache directories found");
        return std::process::ExitCode::SUCCESS;
    }
    let state_dir = PathBuf::from(DUPE_HASH_DIR);
    for target in &targets {
        let supported = on_path("findmnt")
            && fstype_of(target).is_some_and(|fstype| supports_dedupe_filesystem(&fstype));
        if !supported {
            log(&format!("skipping unsupported filesystem for {}", target.display()));
            continue;
        }
        log(&format!("deduping {}", target.display()));
        let Ok(hash_file) = secure_dedupe_hash_file(target, &state_dir) else { continue };
        let command = dedupe_command(target, &hash_file, on_path("ionice"));
        let Some((program, args)) = command.split_first() else { continue };
        let _ = std::process::Command::new(program).args(args).output();
    }
    std::process::ExitCode::SUCCESS
}
