//! Native replacement for the Python `kyth-apply-desktop-layout` launcher.
//!
//! Applies the Kyth comfort Plasma layout (bottom panel, kickoff, icon
//! tasks, tray, clock) via a `qdbus` Plasma evaluate-script, stamping
//! `KythComfortLayout` afterwards. Exit codes mirror the Python launcher:
//! `0` already current or applied, `64` when neither `--force` nor
//! `--initial` was passed, `75` when no `qdbus` is installed, `1` when the
//! Plasma script fails. Unknown arguments are ignored, as before.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use kyth_shared::system::desktop_plasma::{
    CONFIG_FILE, HIDDEN_TRAY_ITEMS, LAYOUT_VERSION, LayoutDecision, TRAY_ITEMS, decide_layout,
    default_application_roots, default_launchers, evaluate_plasma_argv, filter_available_launchers,
    kreadconfig_argv, kwriteconfig_argv, qdbus_candidates, render_layout_script,
};
use kyth_shared::system::process::run_bounded;

fn find_binary(name: &str) -> Option<PathBuf> {
    env::var_os("PATH").map(|paths| {
        env::split_paths(&paths).map(|dir| dir.join(name)).find(|path| path.is_file())
    }).flatten()
}

fn first_binary(names: &[&str]) -> Option<PathBuf> {
    names.iter().find_map(|name| find_binary(name))
}

fn read_config_key(binary: &PathBuf, key: &str) -> Option<String> {
    let binary = binary.to_string_lossy().into_owned();
    let argv = kreadconfig_argv(&binary, CONFIG_FILE, "KythOS", key);
    let output = run_bounded(&argv, Duration::from_secs(5)).ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        None
    }
}

fn main() -> ExitCode {
    let mut force = false;
    let mut initial = false;
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--force" => force = true,
            "--initial" => initial = true,
            "-h" | "--help" => {
                println!("Usage: kyth-apply-desktop-layout [--initial|--force]");
                return ExitCode::SUCCESS;
            }
            _ => {}
        }
    }

    let kread = first_binary(&["kreadconfig6", "kreadconfig"]);
    let current = kread.as_ref().and_then(|binary| read_config_key(binary, "KythComfortLayout"));
    let legacy = kread.as_ref().and_then(|binary| read_config_key(binary, "WindowsFamiliarLayout"));
    match decide_layout(force, initial, current.as_deref(), legacy.as_deref()) {
        LayoutDecision::AlreadyCurrent => return ExitCode::SUCCESS,
        LayoutDecision::Refused => return ExitCode::from(64),
        LayoutDecision::Apply => {}
    }

    let Some(qdbus) = first_binary(&qdbus_candidates()) else {
        return ExitCode::from(75);
    };
    let home = env::var("HOME").unwrap_or_default();
    let available = filter_available_launchers(&default_launchers(), &default_application_roots(home));
    let script = render_layout_script(
        &available.join(","),
        &TRAY_ITEMS.join(","),
        &HIDDEN_TRAY_ITEMS.join(","),
    );
    let argv = evaluate_plasma_argv(&qdbus.to_string_lossy(), &script);
    let applied = run_bounded(&argv, Duration::from_secs(15))
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !applied {
        return ExitCode::from(1);
    }
    if let Some(kwrite) = first_binary(&["kwriteconfig6", "kwriteconfig"]) {
        let binary = kwrite.to_string_lossy().into_owned();
        let argv = kwriteconfig_argv(&binary, CONFIG_FILE, &["KythOS"], "KythComfortLayout", LAYOUT_VERSION, None);
        let _ = run_bounded(&argv, Duration::from_secs(5));
    }
    ExitCode::SUCCESS
}
