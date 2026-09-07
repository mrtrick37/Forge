//! Native replacement for the Python `kyth-apply-pipewire-latency` launcher.
//!
//! Writes the session quantum drop-in and per-app env map from
//! `pipewire-latency.toml`, then refreshes the best-effort TTL marker.
//! Always exits `0`; an empty note list prints as `nothing to apply`.
//! `pipewire_latency.py` stays as the Phase 3 fixture.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use kyth_shared::atomic_io::atomic_write_text;
use kyth_shared::system::pipewire_latency::{DEFAULT_RATE, TTL_PATH, TTL_SECS, apply, config_path, load, xdg_config};

fn main() -> std::process::ExitCode {
    let apps = load(config_path(None::<&Path>));
    let mut notes = match apply(&xdg_config(), &apps, DEFAULT_RATE) {
        Ok(notes) => notes,
        Err(error) => {
            eprintln!("kyth-apply-pipewire-latency: failed: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };
    if notes.is_empty() {
        notes.push("nothing to apply".to_string());
    }
    if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
        let _ = atomic_write_text(TTL_PATH, &(now.as_secs() + TTL_SECS).to_string(), None);
    }
    println!("kyth-apply-pipewire-latency: {}", notes.join("; "));
    std::process::ExitCode::SUCCESS
}
