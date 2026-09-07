//! Native replacement for the Python `kyth-apply-role-preset` launcher.
//!
//! Applies a role profile in two halves: the Plasma layout half (kickoff
//! favorites, discover notifier, widget-update script, `kbuildsycoca`) and
//! the declarative-home half (missing Flatpaks, Distroboxes, and editor
//! extensions only — a second run is a no-op). Exit status is the layout
//! result (`0`, or `64` for an unknown profile); preset warnings go to
//! stderr without affecting it. Both Python sources stay as Phase 3
//! fixtures.

use std::collections::HashSet;
use std::env;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use kyth_shared::system::desktop_plasma::{
    HIDDEN_TRAY_ITEMS, TRAY_ITEMS, LauncherChoice, default_application_roots, evaluate_plasma_argv,
    filter_available_launchers, kwriteconfig_argv, normalize_role_arg, profile_stamp_path,
    qdbus_candidates, render_role_script, role_launchers, role_layout_target,
};
use kyth_shared::system::process::run_bounded;
use kyth_shared::system::role_preset::{
    Role, config_path as preset_config_path, defaults_for, distrobox_create_argv,
    extension_install_argv, flatpak_install_argv, parse_distrobox_list, parse_extension_list,
    parse_flatpak_list, plan_installs, save, VSCODE_BINARIES, VSCODE_INSTALL_BINARIES,
};

const USAGE: &str = "Usage: kyth-apply-role-preset [everyday|gaming|dev|creator]";

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

fn probe(argv: &[String], timeout_secs: u64) -> Option<String> {
    run_bounded(argv, Duration::from_secs(timeout_secs)).ok().and_then(|output| {
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    })
}

/// Layout half, mirroring `apply_role_preset` (idempotent, always safe).
fn apply_layout(profile: &str) -> i32 {
    let Some(launchers) = role_launchers(role_layout_target(profile)) else { return 64 };
    let home = env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    let stamp = profile_stamp_path(&home);
    if let Some(parent) = stamp.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&stamp, format!("{profile}\n"));
    let choices: Vec<LauncherChoice> =
        launchers.iter().map(|name| LauncherChoice::Single((*name).to_string())).collect();
    let available: Vec<String> = filter_available_launchers(&choices, &default_application_roots(&home));
    let csv = available.join(",");
    if let Some(kwrite) = first_binary(&["kwriteconfig6", "kwriteconfig5", "kwriteconfig"]) {
        let _ = run_bounded(
            &kwriteconfig_argv(&kwrite, "kickoffrc", &["Favorites"], "FavoriteURLs", &csv, None),
            Duration::from_secs(5),
        );
        let _ = run_bounded(
            &kwriteconfig_argv(&kwrite, "plasma-discoverrc", &["UpdatesNotifier"], "UseNotifications", "false", Some("bool")),
            Duration::from_secs(5),
        );
    }
    if let Some(qdbus) = first_binary(&qdbus_candidates()) {
        let script = render_role_script(&csv, &TRAY_ITEMS.join(","), &HIDDEN_TRAY_ITEMS.join(","));
        let _ = run_bounded(&evaluate_plasma_argv(&qdbus, &script), Duration::from_secs(15));
    }
    if let Some(sycoca) = first_binary(&["kbuildsycoca6", "kbuildsycoca"]) {
        let _ = run_bounded(&[sycoca, "--noincremental".to_string()], Duration::from_secs(10));
    }
    0
}

/// Preset half, mirroring `apply_preset` (install-only-missing).
fn apply_preset(profile: Role) {
    let preset = defaults_for(profile);
    if let Err(error) = save(preset_config_path(None::<&Path>), &preset) {
        eprintln!("preset apply warning: {error}");
    }
    let have_flatpaks = probe(&["flatpak".to_string(), "list".to_string(), "--app".to_string(), "--columns=application".to_string()], 10)
        .map(|text| parse_flatpak_list(&text))
        .unwrap_or_default();
    let have_boxes = probe(&["distrobox".to_string(), "list".to_string(), "--no-color".to_string()], 10)
        .map(|text| parse_distrobox_list(&text))
        .unwrap_or_default();
    let mut have_extensions: HashSet<String> = HashSet::new();
    for binary in VSCODE_BINARIES {
        if let Some(text) = probe(&[binary.to_string(), "--list-extensions".to_string()], 10) {
            have_extensions = parse_extension_list(&text);
            break;
        }
    }
    let (installed, _skipped) = plan_installs(&preset, &have_flatpaks, &have_boxes, &have_extensions);
    for app in &preset.flatpaks {
        if installed.contains(app) {
            let _ = run_bounded(&flatpak_install_argv(app), Duration::from_secs(300));
        }
    }
    for name in &preset.distroboxes {
        if installed.contains(name) {
            let _ = run_bounded(&distrobox_create_argv(name), Duration::from_secs(300));
        }
    }
    for extension in &preset.vscode_extensions {
        if installed.contains(extension) {
            for binary in VSCODE_INSTALL_BINARIES {
                // First binary that spawns wins, regardless of exit status.
                if run_bounded(&extension_install_argv(binary, extension), Duration::from_secs(60)).is_ok() {
                    break;
                }
            }
        }
    }
}

fn main() -> ExitCode {
    let arg = env::args().nth(1).unwrap_or_else(|| "everyday".to_string());
    let Some(profile) = normalize_role_arg(&arg) else {
        eprintln!("{USAGE}");
        return ExitCode::from(64);
    };
    let layout_rc = apply_layout(profile);
    apply_preset(Role::parse(Some(profile)));
    ExitCode::from(layout_rc as u8)
}
