//! Native replacement for the Python `kyth-web-app-categorize` launcher.
//!
//! Scans `~/.local/share/applications` for Chromium-family PWA launchers
//! without a `Categories=` line, inserts `Categories=X-KythWebApp;` after
//! `[Desktop Entry]`, and rebuilds KSysCoca when anything changed.
//! Per-file failures are skipped, exactly as the Python loop's broad
//! except did. Always exits `0`. `desktop/shortcut.py` stays as the
//! Phase 3 fixture.

use std::env;
use std::path::PathBuf;

use kyth_shared::atomic_io::atomic_write_text;
use kyth_shared::system::desktop_shortcuts::{
    categorize_web_app, matches_web_app_name, WEB_APP_GLOBS,
};

fn on_path(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

fn applications_dir() -> PathBuf {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".local/share/applications")
}

fn is_watched(name: &str) -> bool {
    WEB_APP_GLOBS
        .iter()
        .any(|pattern| matches_web_app_name(name, pattern))
}

fn categorize() -> bool {
    let app_dir = applications_dir();
    if !app_dir.is_dir() {
        return false;
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&app_dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .collect()
        })
        .unwrap_or_default();
    entries.sort();
    let mut changed = false;
    for path in &entries {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !is_watched(&name) || !path.is_file() {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let content = String::from_utf8_lossy(&bytes);
        // Mirror the Python loop: the gates passing means write and mark
        // changed, even when the bytes come back identical.
        let Some(updated) = categorize_web_app(&content) else {
            continue;
        };
        if atomic_write_text(path, &updated, None).is_ok() {
            changed = true;
        }
    }
    if changed && on_path("kbuildsycoca6") {
        let _ = std::process::Command::new("kbuildsycoca6")
            .arg("--noincremental")
            .output();
    }
    changed
}

fn main() -> std::process::ExitCode {
    categorize();
    std::process::ExitCode::SUCCESS
}
