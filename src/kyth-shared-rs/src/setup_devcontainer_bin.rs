//! Native replacement for the Python `kyth-setup-devcontainer` launcher.
//!
//! Creates one distrobox per `[containers."name"]` entry in
//! `devcontainers.toml` (offline declarative preset). Prints the
//! no-containers line when the config is missing or empty, prints one
//! status line per box, and runs each creation best-effort with a 120s
//! bound. Exit is always `0`. `devcontainers.py` stays as the Phase 3
//! fixture.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use kyth_shared::system::devcontainers::{
    create_argv, describe, devcontainers_path, load_devcontainers, CREATE_TIMEOUT_SECS,
    NO_CONTAINERS_MESSAGE,
};
use kyth_shared::system::process::run_bounded;

fn main() -> std::process::ExitCode {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let xdg = env::var("XDG_CONFIG_HOME").ok();
    let boxes = load_devcontainers(&devcontainers_path(&home, xdg.as_deref()));
    if boxes.is_empty() {
        println!("{NO_CONTAINERS_MESSAGE}");
        return std::process::ExitCode::SUCCESS;
    }
    for (name, entry) in &boxes {
        println!("{}", describe(name, &entry.image));
        let _ = run_bounded(
            &create_argv(name, &entry.image),
            Duration::from_secs(CREATE_TIMEOUT_SECS),
        );
    }
    std::process::ExitCode::SUCCESS
}
