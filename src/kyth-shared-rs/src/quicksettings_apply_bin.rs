//! Native replacement for the Python `kyth-apply-quicksettings` launcher.
//!
//! Applies the QuickSettings brightness via PowerDevil over `qdbus` and
//! stamps the best-effort TTL marker. Always exits `0`; a missing `qdbus`
//! simply records no note. One deliberate deviation: an unparseable
//! `brightness` value falls back to `80` instead of aborting with a
//! traceback — crashing on a user config typo is a bug, not a contract.
//! `quicksettings.py` stays as the Phase 3 fixture.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kyth_shared::atomic_io::atomic_write_text;
use kyth_shared::system::process::run_bounded;
use kyth_shared::system::quicksettings::{brightness_argv, config_path, load, TTL_PATH, TTL_SECS};

fn main() -> std::process::ExitCode {
    let config = load(config_path(None::<&Path>));
    let mut applied = Vec::new();
    if run_bounded(&brightness_argv(config.brightness), Duration::from_secs(5)).is_ok() {
        applied.push("brightness".to_string());
    }
    if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
        let _ = atomic_write_text(TTL_PATH, &(now.as_secs() + TTL_SECS).to_string(), None);
    }
    println!("kyth-apply-quicksettings: {}", applied.len());
    std::process::ExitCode::SUCCESS
}
