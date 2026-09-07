//! Native replacement for the Python `kyth-batteryd` launcher.
//!
//! Charge-threshold daemon: every 30 seconds it reloads the battery
//! config, applies the charge-stop threshold to every battery, and
//! appends a health snapshot to the ledger when health checks are on.
//! Runs forever until terminated. `battery.py` stays as the Phase 3
//! fixture.

use std::path::PathBuf;
use std::time::Duration;

use kyth_shared::system::battery::{
    LEDGER_PATH, append_ledger, apply_threshold, battery_config_path, load_battery,
    read_battery_health,
};

fn main() -> std::process::ExitCode {
    let config_path = battery_config_path(None::<PathBuf>);
    let ledger = PathBuf::from(LEDGER_PATH);
    loop {
        let config = load_battery(&config_path);
        apply_threshold(config.charge_stop);
        if config.health_check {
            let _ = append_ledger(&ledger, &read_battery_health(), &config);
        }
        std::thread::sleep(Duration::from_secs(30));
    }
}
