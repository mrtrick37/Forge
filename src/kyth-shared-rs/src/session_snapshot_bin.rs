//! Native replacement for the Python `kyth-session-snapshot` launcher.
//!
//! Collects environment diagnostics into a timestamped snapshot file and
//! prints its path. An optional positional argument overrides the output
//! directory. `session.py` stays as the Phase 3 fixture.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use kyth_shared::system::issue_draft::local_timestamp;
use kyth_shared::system::process::run_bounded;
use kyth_shared::system::session_snapshot::{current_host, current_user, now_iso, snapshot};

fn main() -> std::process::ExitCode {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let out_dir = env::args().nth(1).map(PathBuf::from);
    let path = snapshot(
        &home,
        out_dir.as_deref(),
        &local_timestamp(),
        &now_iso(),
        &current_user(),
        &current_host(),
        &|argv| {
            run_bounded(argv, Duration::from_secs(60))
                .map_err(|error| error.to_string())
                .map(|output| {
                    (
                        String::from_utf8_lossy(&output.stdout).into_owned(),
                        String::from_utf8_lossy(&output.stderr).into_owned(),
                    )
                })
        },
        &|name| {
            env::var_os("PATH")
                .map(|paths| env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
                .unwrap_or(false)
        },
    );
    println!("{}", path.display());
    std::process::ExitCode::SUCCESS
}
