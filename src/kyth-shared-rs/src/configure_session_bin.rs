//! Fail-open Plasma Login Manager session migration.
//!
//! This is the native owner of the small, bounded session-preparation step.
//! It only writes fixed configuration files and rewrites stale Plasma X11
//! session selections; it never invokes a shell or an external command.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

const SESSION_CONF: &str =
    "[General]\nDefaultSession=plasma.desktop\n\n[Autologin]\nSession=plasma.desktop\n";

fn arg_path(args: &[String], name: &str, default: &str) -> PathBuf {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| PathBuf::from(&pair[1]))
        .unwrap_or_else(|| PathBuf::from(default))
}
fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn warn(error: impl std::fmt::Display) {
    eprintln!("Warning: {error}");
}

fn rewrite_file(path: &Path) {
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    let (rewritten, changed) = kyth_shared::system::wayland::rewrite_session_key(&text, "Session");
    if changed {
        if let Err(error) = fs::write(path, rewritten) {
            warn(format!("could not migrate {path:?}: {error}"));
        }
    }
}

fn migrate_homes(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let home = entry.path();
        if home.is_dir() {
            rewrite_file(&home.join(".dmrc"));
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let conf_dir = arg_path(&args, "--conf-dir", "/etc/plasmalogin.conf.d");
    let state = arg_path(&args, "--state-file", "/var/lib/plasmalogin/state.conf");
    let homes = arg_path(&args, "--homes", "/home");
    let env_file = arg_path(&args, "--env-file", "/run/kyth-greeter.env");
    let dri = arg_path(&args, "--dri", "/dev/dri");
    let cmdline = arg_value(&args, "--cmdline")
        .or_else(|| fs::read_to_string("/proc/cmdline").ok())
        .unwrap_or_default();

    if let Err(error) = fs::create_dir_all(&conf_dir) {
        warn(format!("could not create {conf_dir:?}: {error}"));
        return;
    }
    if let Err(error) = fs::write(conf_dir.join("11-kyth-session.conf"), SESSION_CONF) {
        warn(format!("could not write session configuration: {error}"));
    }
    rewrite_file(&state);
    rewrite_file(Path::new("/var/lib/sddm/state.conf"));
    migrate_homes(&homes);

    let body = if kyth_shared::system::wayland::needs_software_compose(&dri, Some(&cmdline)) {
        kyth_shared::system::wayland::software_compose_env()
            .into_iter()
            .map(|(key, value)| format!("{key}={value}\n"))
            .collect()
    } else {
        String::new()
    };
    if let Err(error) = fs::write(&env_file, body) {
        warn(format!("could not write greeter environment: {error}"));
    }
}
