//! Native replacement for the Python `kyth-apply-driver-switch` launcher.
//!
//! Reports the `driver.toml` GPU selection (`gpu` + `mesa_git`); the real
//! switch happens via `thirdparty.sh --driver`, exactly as the Python
//! launcher only logged. Always exits `0`. `driver_preset.py` stays as
//! the Phase 3 fixture.

use std::path::Path;

use kyth_shared::system::driver_config::{config_path, load};

fn main() -> std::process::ExitCode {
    let config = load(config_path(None::<&Path>));
    println!(
        "kyth-driver-switch: gpu {} mesa_git {}",
        config.gpu, config.mesa_git
    );
    std::process::ExitCode::SUCCESS
}
