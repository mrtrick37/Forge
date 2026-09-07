//! Native replacement for the Python `kyth-refresh-taskbar-pins` launcher.
//!
//! Re-applies the taskbar pins when the layout stamp exists, the launcher
//! set is non-empty, and the state file disagrees. Exit `0` on every early
//! path; `1` only when the Plasma script fails. `desktop/plasma.py` stays
//! as the Phase 3 fixture.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use kyth_shared::system::desktop_plasma::{
    CONFIG_FILE, default_application_roots, default_launchers, evaluate_plasma_argv,
    filter_available_launchers, kreadconfig_argv, qdbus_candidates, render_pins_script,
    taskbar_pins_state_path,
};
use kyth_shared::system::process::run_bounded;

fn find_binary(name: &str) -> Option<String> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
            .map(|path| path.to_string_lossy().into_owned())
    })
}

fn kread(file: &str, group: &str, key: &str) -> Option<String> {
    let binary = find_binary("kreadconfig6").or_else(|| find_binary("kreadconfig"))?;
    run_bounded(&kreadconfig_argv(&binary, file, group, key), Duration::from_secs(5))
        .ok()
        .and_then(|output| {
            output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        })
}

fn refresh() -> i32 {
    if kread(CONFIG_FILE, "KythOS", "KythComfortLayout").filter(|stamp| !stamp.is_empty()).is_none() {
        return 0;
    }
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let available = filter_available_launchers(&default_launchers(), &default_application_roots(&home));
    let csv = available.join(",");
    if csv.is_empty() {
        return 0;
    }
    let state_file = taskbar_pins_state_path(&home);
    if state_file.is_file() {
        if let Ok(text) = std::fs::read_to_string(&state_file) {
            if text.trim() == csv {
                return 0;
            }
        }
    }
    let Some(qdbus) = qdbus_candidates().iter().find_map(|name| find_binary(name)) else { return 0 };
    let applied = run_bounded(&evaluate_plasma_argv(&qdbus, &render_pins_script(&csv)), Duration::from_secs(15))
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !applied {
        return 1;
    }
    if let Some(parent) = state_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&state_file, format!("{csv}\n"));
    0
}

fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(refresh() as u8)
}
