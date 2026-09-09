//! Native replacement for the Python `kyth-apply-window-snap` launcher.
//!
//! Writes the `ElectricBorder` key (the only noted write), refreshes the
//! three Win+Arrow shortcuts best-effort, and stamps the TTL. Always exits
//! `0`; a missing `kwriteconfig` skips silently. `window_snap.py` stays as
//! the Phase 3 fixture.

use std::env;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kyth_shared::atomic_io::atomic_write_text;
use kyth_shared::system::process::run_bounded;
use kyth_shared::system::window_snap::{
    config_path, electric_border_argv, kwriteconfig_candidates, load, shortcut_argv, SHORTCUTS,
    TTL_PATH, TTL_SECS,
};

fn main() -> std::process::ExitCode {
    let config = load(config_path(None::<&Path>));
    let mut applied = Vec::new();
    if let Some(binary) = kwriteconfig_candidates().iter().find_map(|name| {
        env::var_os("PATH").and_then(|paths| {
            env::split_paths(&paths)
                .map(|dir| dir.join(name))
                .find(|path| path.is_file())
                .map(|path| path.to_string_lossy().into_owned())
        })
    }) {
        if run_bounded(
            &electric_border_argv(&binary, config.electric),
            Duration::from_secs(5),
        )
        .map(|output| output.status.success())
        .unwrap_or(false)
        {
            applied.push("kwinrc ElectricBorder".to_string());
        }
        for (action, key) in SHORTCUTS {
            let _ = run_bounded(&shortcut_argv(&binary, action, key), Duration::from_secs(5));
        }
        if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
            let _ = atomic_write_text(TTL_PATH, &(now.as_secs() + TTL_SECS).to_string(), None);
        }
    }
    println!("kyth-apply-window-snap: {}", applied.len());
    std::process::ExitCode::SUCCESS
}
