//! Native replacement for the Python `kyth-apply-tailscale` launcher.
//!
//! Brings `tailscale up` per `tailscale.toml` (exit node and route
//! acceptance only when configured). Best-effort with the launcher's
//! output lines; always exits `0`. `tailscale_preset.py` stays as the
//! Phase 3 fixture.

use std::path::Path;
use std::time::Duration;

use kyth_shared::system::process::run_bounded;
use kyth_shared::system::tailscale_preset::{config_path, load, up_argv};

fn main() -> std::process::ExitCode {
    let preset = load(config_path(None::<&Path>));
    if preset.tailnet.is_empty() {
        println!("kyth-apply-tailscale: no tailnet, skipping");
        return std::process::ExitCode::SUCCESS;
    }
    let _ = run_bounded(&up_argv(&preset), Duration::from_secs(10));
    println!("kyth-apply-tailscale: tailnet {}", preset.tailnet);
    std::process::ExitCode::SUCCESS
}
