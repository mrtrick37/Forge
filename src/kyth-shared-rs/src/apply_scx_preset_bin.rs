//! Native replacement for the Python `kyth-apply-scx-preset` launcher.
//!
//! Reads the user-authored `scx.toml` (see `service_preferences::load_scx`)
//! and, if it declares any explicit per-game scheduler, writes
//! `/etc/scx/scx_loader.conf` so the loader picks it up (explicit wins over
//! `kyth-ai-perfd`'s TTL-based policy — see `ai_perf_daemon_bin`'s own writes
//! to the same file). This reproduces the Python launcher's output byte for
//! byte, including behavior that looks incidental: no atomic write, and no
//! awareness of `kyth-ai-perfd`'s TTL/SCX marker files. Fixing those is a
//! separate change, not part of this port.

use std::path::Path;

use kyth_shared::system::service_preferences::{load_scx, scx_config_path};

const SCX_LOADER_CONF: &str = "/etc/scx/scx_loader.conf";

fn main() -> std::process::ExitCode {
    let presets = load_scx(scx_config_path(None::<&Path>));
    if presets.is_empty() {
        println!("kyth-apply-scx-preset: no games");
        return std::process::ExitCode::SUCCESS;
    }
    // Python's launcher picked `list(presets.values())[0]`, i.e. whatever
    // order the source TOML declared its `[games.*]` tables in. `load_scx`
    // returns a `BTreeMap` (key-sorted), which has no equivalent notion of
    // "first declared" — this port deliberately selects the value for the
    // lexicographically-first app name instead. See
    // `multi_game_scx_selects_lexicographically_first_app` in
    // `service_preferences` for the pinned behavior.
    let scx = presets.values().next().cloned().unwrap_or_default();
    if scx != "none" {
        let dest = Path::new(SCX_LOADER_CONF);
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(dest, format!("SCX_SCHEDULER={scx}\n# per-game explicit\n"));
    }
    println!("kyth-apply-scx-preset: {} games \u{2192} {scx}", presets.len());
    std::process::ExitCode::SUCCESS
}
