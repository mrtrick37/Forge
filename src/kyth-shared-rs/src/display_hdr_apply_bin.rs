//! Native replacement for the Python `kyth-apply-display-hdr` launcher.
//!
//! Applies `display-hdr.toml` via `kscreen-doctor` in a Wayland session,
//! printing the per-output notes exactly as the Python launcher did. The
//! launcher always exits `0`; failures are reported as note strings
//! (`kscreen-doctor unavailable`, `no connected outputs`, `*.hdr.* failed`,
//! `nothing to apply`). `display_hdr.py` itself stays as the Phase 3
//! fixture (its `plasma_hdr` import path is test-only).

use std::env;
use std::time::Duration;

use kyth_shared::system::display::parse_kscreen_outputs;
use kyth_shared::system::hdr::{
    apply_note, config_path, is_output_name_valid, kscreen_apply_argv, load, select_targets,
};
use kyth_shared::system::process::run_bounded;

fn on_path(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

fn is_wayland() -> bool {
    env::var("XDG_SESSION_TYPE").map(|value| value.to_ascii_lowercase() == "wayland").unwrap_or(false)
}

fn main() {
    let notes = apply();
    println!("kyth-apply-display-hdr: {}", notes.join("; "));
}

fn apply() -> Vec<String> {
    let displays = load(config_path(None::<&std::path::Path>));
    if !is_wayland() {
        return vec!["hdr skipped: not a Wayland session".to_string()];
    }
    if !on_path("kscreen-doctor") {
        return vec!["kscreen-doctor unavailable".to_string()];
    }
    let listed = match run_bounded(&["kscreen-doctor".to_string(), "-o".to_string()], Duration::from_secs(8)) {
        Ok(output) if output.status.success() => output,
        _ => return vec!["kscreen-doctor -o failed".to_string()],
    };
    let connected: Vec<String> = parse_kscreen_outputs(&String::from_utf8_lossy(&listed.stdout))
        .into_iter()
        .filter(|output| output.connected && is_output_name_valid(&output.name))
        .map(|output| output.name)
        .collect();
    if connected.is_empty() {
        return vec!["no connected outputs".to_string()];
    }
    let targets = select_targets(&connected, &displays, None);
    let mut applied = Vec::new();
    for (name, display) in &targets {
        let argv = kscreen_apply_argv(name, display);
        let success = run_bounded(&argv, Duration::from_secs(12))
            .map(|output| output.status.success())
            .unwrap_or(false);
        applied.push(apply_note(name, display, success));
    }
    if applied.is_empty() {
        applied.push("nothing to apply".to_string());
    }
    applied
}
