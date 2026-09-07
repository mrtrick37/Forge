//! Native replacement for the Python `kyth-apply-rgb` launcher.
//!
//! Applies `rgb.toml` per-device presets via `openrgb` and `liquidctl`.
//! Every device invocation is best-effort (fire-and-forget, as upstream);
//! the launcher always prints the device count and exits `0`.
//! `rgb_preset.py` stays as the Phase 3 fixture. The `kyth-rgb.service`
//! unit needs no change: it already execs the stable `/usr/bin` path.

use std::path::Path;
use std::time::Duration;

use kyth_shared::system::process::run_bounded;
use kyth_shared::system::rgb_preset::{config_path, liquidctl_argv, load, openrgb_argv};

fn main() -> std::process::ExitCode {
    let devices = load(config_path(None::<&Path>));
    for (device, preset) in &devices {
        let _ = run_bounded(&openrgb_argv(device, preset), Duration::from_secs(5));
        let _ = run_bounded(&liquidctl_argv(device, preset), Duration::from_secs(5));
    }
    println!("kyth-apply-rgb: {} devices", devices.len());
    std::process::ExitCode::SUCCESS
}
