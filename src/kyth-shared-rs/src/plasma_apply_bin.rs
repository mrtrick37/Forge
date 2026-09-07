//! Native replacement for the Python `kyth-apply-plasma` launcher.
//!
//! Applies `plasma.toml` drift via `kwriteconfig` (6 → 5 → plain fallback
//! chain, as the Python launcher ordered it), reconfigures KWin over the
//! first answering `qdbus`, and stamps the TTL marker. Always exits `0`;
//! a missing `kwriteconfig` skips silently with `0 keys`.
//! `plasma_drift.py` stays as the Phase 3 fixture.

use std::env;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use kyth_shared::atomic_io::atomic_write_text;
use kyth_shared::system::plasma_drift::{
    TTL_PATH, TTL_SECS, apply_sections, config_path, kwriteconfig_candidates, load, qdbus_candidates,
    reconfigure_argv, run_timeout,
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

fn first_binary(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| find_binary(name))
}

fn reconfigure_kwin() {
    for name in qdbus_candidates() {
        let Some(qdbus) = find_binary(name) else { continue };
        // Mirror Python: return after the first spawn attempt regardless of
        // its exit status; only a spawn failure tries the next binary.
        if run_bounded(&reconfigure_argv(&qdbus), run_timeout()).is_ok() {
            return;
        }
    }
}

fn main() -> std::process::ExitCode {
    let sections = load(config_path(None::<&Path>));
    let mut applied = Vec::new();
    if let Some(binary) = first_binary(&kwriteconfig_candidates()) {
        applied = apply_sections(&sections, &binary, &|argv| {
            run_bounded(argv, run_timeout()).map(|output| output.status.success()).unwrap_or(false)
        });
        reconfigure_kwin();
        if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
            let _ = atomic_write_text(TTL_PATH, &(now.as_secs() + TTL_SECS).to_string(), None);
        }
    }
    println!("kyth-apply-plasma: {} keys", applied.len());
    std::process::ExitCode::SUCCESS
}
