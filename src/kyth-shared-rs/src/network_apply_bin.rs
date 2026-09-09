//! Native replacement for the Python `kyth-apply-network` launcher.
//!
//! Loads `network.toml` and writes the `systemd-resolved` drop-in with
//! backup/rollback, then refreshes the best-effort TTL marker. The output
//! line keeps Python's `True`/`False` boolean spelling.
//! `network_preset.py` stays as the Phase 3 fixture.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use kyth_shared::system::network_preset::{apply_preset, config_path, load, TTL_PATH, TTL_SECS};

fn py_bool(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}

fn main() -> std::process::ExitCode {
    let preset = load(config_path(None::<&Path>));
    match apply_preset(&preset, Path::new("/")) {
        Ok(written) => {
            let dest = written
                .first()
                .map(PathBuf::as_path)
                .unwrap_or_else(|| Path::new(""));
            println!(
                "kyth-apply-network: wrote {} doh={} dns={}",
                dest.display(),
                py_bool(preset.doh),
                preset.dns,
            );
            if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
                let _ = std::fs::write(TTL_PATH, (now.as_secs() + TTL_SECS).to_string());
            }
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("kyth-apply-network: failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
