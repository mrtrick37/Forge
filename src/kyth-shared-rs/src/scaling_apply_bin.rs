//! Native replacement for the Python `kyth-apply-scaling` launcher.
//!
//! Applies `scaling.toml` per-output fractional scales via
//! `kscreen-doctor` and deploys ICC profiles best-effort. Always exits
//! `0`; an empty config prints `nothing to apply`. `scaling.py` stays as
//! the Phase 3 fixture.

use std::collections::HashSet;
use std::env;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kyth_shared::atomic_io::atomic_write_text;
use kyth_shared::system::display::parse_kscreen_outputs;
use kyth_shared::system::process::run_bounded;
use kyth_shared::system::scaling::{
    ICC_DEST_DIR, TTL_PATH, TTL_SECS, IccOutcome, config_path, icc_outcome, is_output_name_valid, load,
    scale_arg, scale_argv,
};

fn on_path(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

fn main() -> std::process::ExitCode {
    let (mut notes, stamp_ttl) = apply();
    if notes.is_empty() {
        notes.push("nothing to apply".to_string());
    }
    if stamp_ttl {
        if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
            let _ = atomic_write_text(TTL_PATH, &(now.as_secs() + TTL_SECS).to_string(), None);
        }
    }
    println!("kyth-apply-scaling: {}", notes.join("; "));
    std::process::ExitCode::SUCCESS
}

/// Notes plus whether the TTL marker applies (the Python early returns for
/// empty configs, missing kscreen-doctor, and failed probes skip it).
fn apply() -> (Vec<String>, bool) {
    let outputs = load(config_path(None::<&Path>));
    if outputs.is_empty() {
        return (Vec::new(), false);
    }
    if !on_path("kscreen-doctor") {
        return (vec!["kscreen-doctor unavailable".to_string()], false);
    }
    let listed = match run_bounded(&["kscreen-doctor".to_string(), "-o".to_string()], Duration::from_secs(8)) {
        Ok(output) if output.status.success() => output,
        _ => return (vec!["kscreen-doctor -o failed".to_string()], false),
    };
    let connected: HashSet<String> = parse_kscreen_outputs(&String::from_utf8_lossy(&listed.stdout))
        .into_iter()
        .filter(|output| output.connected && is_output_name_valid(&output.name))
        .map(|output| output.name)
        .collect();
    let mut applied = Vec::new();
    for (conn, entry) in &outputs {
        if !connected.contains(conn) {
            applied.push(format!("{conn}: not connected"));
            continue;
        }
        let scale = scale_arg(entry.scale);
        let success = run_bounded(&scale_argv(conn, &scale), Duration::from_secs(10))
            .map(|output| output.status.success())
            .unwrap_or(false);
        if success {
            applied.push(format!("{conn}.scale={scale}"));
        } else {
            applied.push(format!("{conn}.scale failed"));
        }
        match icc_outcome(conn, &entry.icc, Path::new(ICC_DEST_DIR)) {
            IccOutcome::Skipped => {}
            IccOutcome::Deployed(note) | IccOutcome::NotDeployed(note) | IccOutcome::Failed(note) => {
                applied.push(note);
            }
        }
    }
    (applied, true)
}
