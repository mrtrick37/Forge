//! Native replacement for the Python `kyth-ntfs-repair` launcher.
//!
//! Scans NTFS partitions, stabilizes mounts, and redirects Steam Proton
//! compatdata to native storage, print for print like the Python launcher.
//! Always exits `0`. `system/drives.py` stays as the Phase 3 fixture.
//! The `fix-ntfs-drives` ujust recipe keeps working against the stable
//! `/usr/bin` path.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use kyth_shared::system::drives::repair;
use kyth_shared::system::process::run_bounded;

fn main() -> std::process::ExitCode {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let argv = ["lsblk", "-J", "-o", "NAME,FSTYPE,LABEL,UUID,MOUNTPOINT"]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    // Mirror Python: spawn failures yield no partitions; a non-zero lsblk
    // exit aborts (check=True upstream).
    let raw = match run_bounded(&argv, Duration::from_secs(30)) {
        Err(_) => String::new(),
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
        Ok(output) => {
            eprintln!(
                "kyth-ntfs-repair: lsblk failed with status {}",
                output.status
            );
            return std::process::ExitCode::FAILURE;
        }
    };
    repair(&home, &raw);
    std::process::ExitCode::SUCCESS
}
