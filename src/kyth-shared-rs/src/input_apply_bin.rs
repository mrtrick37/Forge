//! Native replacement for the Python `kyth-apply-input` launcher.
//!
//! Renders `input.toml` per-device libinput presets to
//! `/etc/X11/xorg.conf.d/50-kyth-input.conf` (atomic replace, as the Python
//! launcher's tmp-write + rename did) and refreshes the best-effort
//! `/run/kyth-input-ttl` marker. `input_preset.py` stays as the Phase 3
//! fixture. The one deliberate behavior note: comment speed values use
//! Python `str(float)` formatting (`0.0`, never Rust's `0`).

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use kyth_shared::atomic_io::atomic_write_text;
use kyth_shared::system::input_preset::{TTL_PATH, TTL_SECS, XORG_CONF_DEST, config_path, load, render_xorg_conf};

fn main() -> std::process::ExitCode {
    let devices = load(config_path(None::<&Path>));
    let dest = Path::new(XORG_CONF_DEST);
    if let Some(parent) = dest.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            eprintln!("kyth-apply-input: failed: {error}");
            return std::process::ExitCode::FAILURE;
        }
    }
    if let Err(error) = atomic_write_text(dest, &render_xorg_conf(&devices), None) {
        eprintln!("kyth-apply-input: failed: {error}");
        return std::process::ExitCode::FAILURE;
    }
    if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
        let _ = std::fs::write(TTL_PATH, (now.as_secs() + TTL_SECS).to_string());
    }
    println!("kyth-apply-input: {} devices → {XORG_CONF_DEST}", devices.len());
    std::process::ExitCode::SUCCESS
}
