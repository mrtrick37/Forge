//! Native replacement for the Python `kyth-kali-desktop-fixup` launcher.
//!
//! Rewrites `.desktop` launchers exported from the Kali container
//! (security category, privilege escalation, hidden flags) plus the
//! Zenmap-specific Exec routing, then refreshes the desktop database and
//! KSysCoca when anything changed. Per-file failures are skipped, exactly
//! as the Python loop's broad except did. Always exits `0`.
//! `desktop/shortcut.py` stays as the Phase 3 fixture.

use std::env;
use std::path::PathBuf;

use kyth_shared::atomic_io::atomic_write_text;
use kyth_shared::system::desktop_shortcuts::{rewrite_kali_desktop, rewrite_zenmap_desktop};

fn on_path(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

fn applications_dir() -> PathBuf {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".local/share/applications")
}

fn is_desktop_file(name: &str) -> bool {
    name.ends_with(".desktop")
}

fn fixup() -> bool {
    let app_dir = applications_dir();
    if !app_dir.is_dir() {
        return false;
    }
    let mut changed = false;
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&app_dir)
        .map(|entries| entries.filter_map(|entry| entry.ok().map(|entry| entry.path())).collect())
        .unwrap_or_default();
    entries.sort();
    for path in &entries {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        if !is_desktop_file(&name) || !path.is_file() {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else { continue };
        let content = String::from_utf8_lossy(&bytes);
        let Some(fixed) = rewrite_kali_desktop(&content) else { continue };
        // Mirror the Python per-file flag: gate pass alone writes nothing —
        // only an actual change (beyond a missing trailing newline) does.
        if fixed == content || format!("{content}\n") == fixed {
            continue;
        }
        if atomic_write_text(path, &fixed, None).is_ok() {
            changed = true;
        }
    }
    for path in &entries {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        if !name.contains("zenmap") || !is_desktop_file(&name) || !path.is_file() {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else { continue };
        let content = String::from_utf8_lossy(&bytes);
        let Some(fixed) = rewrite_zenmap_desktop(&content) else { continue };
        // The Python Zenmap loop writes and marks changed whenever the gate
        // passes, even when neither Exec line matched.
        if atomic_write_text(path, &fixed, None).is_ok() {
            changed = true;
        }
    }
    if changed {
        if on_path("update-desktop-database") {
            let _ = std::process::Command::new("update-desktop-database").arg(&app_dir).output();
        }
        if on_path("kbuildsycoca6") {
            let _ = std::process::Command::new("kbuildsycoca6").arg("--noincremental").output();
        }
    }
    changed
}

fn main() -> std::process::ExitCode {
    fixup();
    std::process::ExitCode::SUCCESS
}
