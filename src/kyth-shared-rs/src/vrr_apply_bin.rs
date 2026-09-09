//! Native replacement for the Python `kyth-apply-vrr` launcher.
//!
//! Writes the global `[Wayland] VrrPolicy` and `[NightColor]` keys via
//! `kwriteconfig`, applies per-output `kscreen-doctor` overrides when a
//! live session is available, reconfigures KWin, and stamps the TTL.
//! Always exits `0`. Note the preserved Python spelling quirk: the
//! `NightColor.Active` *note* uses `True`/`False` while the written value
//! is lowercase. `vrr.py` stays as the Phase 3 fixture.

use std::env;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kyth_shared::atomic_io::atomic_write_text;
use kyth_shared::system::display::parse_kscreen_outputs;
use kyth_shared::system::process::run_bounded;
use kyth_shared::system::vrr::{
    config_path, doctor_mode, global_policy, is_output_name_valid, kwin_argv, load,
    mode_for_policy, per_output_argv, TTL_PATH, TTL_SECS,
};

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

fn on_path(name: &str) -> bool {
    find_binary(name).is_some()
}

fn write_key(binary: &str, group: &str, key: &str, value: &str, value_type: Option<&str>) -> bool {
    run_bounded(
        &kwin_argv(binary, group, key, value, value_type),
        Duration::from_secs(5),
    )
    .map(|output| output.status.success())
    .unwrap_or(false)
}

fn per_output_notes(outputs: &std::collections::BTreeMap<String, String>) -> Vec<String> {
    if outputs.is_empty() || !on_path("kscreen-doctor") {
        return Vec::new();
    }
    let listed = match run_bounded(
        &["kscreen-doctor".to_string(), "-o".to_string()],
        Duration::from_secs(8),
    ) {
        Ok(output) if output.status.success() => output,
        _ => return vec!["kscreen-doctor -o failed".to_string()],
    };
    let connected: std::collections::HashSet<String> =
        parse_kscreen_outputs(&String::from_utf8_lossy(&listed.stdout))
            .into_iter()
            .filter(|output| output.connected && is_output_name_valid(&output.name))
            .map(|output| output.name)
            .collect();
    let mut notes = Vec::new();
    for (conn, mode) in outputs {
        if !connected.contains(conn) {
            notes.push(format!("{conn}: not connected"));
            continue;
        }
        let success = run_bounded(&per_output_argv(conn, mode), Duration::from_secs(10))
            .map(|output| output.status.success())
            .unwrap_or(false);
        if success {
            notes.push(format!("{conn}.vrrpolicy.{}", doctor_mode(mode)));
        } else {
            notes.push(format!("{conn}.vrrpolicy failed"));
        }
    }
    notes
}

fn reconfigure_kwin() {
    for name in ["qdbus6", "qdbus-qt6", "qdbus"] {
        let Some(qdbus) = find_binary(name) else {
            continue;
        };
        if run_bounded(
            &[
                qdbus,
                "org.kde.KWin".to_string(),
                "/KWin".to_string(),
                "reconfigure".to_string(),
            ],
            Duration::from_secs(5),
        )
        .is_ok()
        {
            return;
        }
    }
}

fn py_bool(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}

fn main() -> std::process::ExitCode {
    let config = load(config_path(None::<&Path>));
    let mut applied = Vec::new();
    if let Some(binary) = first_binary(&["kwriteconfig6", "kwriteconfig5", "kwriteconfig"]) {
        let policy = if config.outputs.is_empty() {
            "1"
        } else {
            global_policy(&config.outputs)
        };
        if write_key(&binary, "Wayland", "VrrPolicy", policy, None) {
            applied.push(format!(
                "Wayland.VrrPolicy={policy} ({})",
                mode_for_policy(policy)
            ));
        }
        if write_key(
            &binary,
            "NightColor",
            "Active",
            &config.night_enabled.to_string().to_lowercase(),
            Some("bool"),
        ) {
            applied.push(format!(
                "NightColor.Active={}",
                py_bool(config.night_enabled)
            ));
        }
        if write_key(&binary, "NightColor", "Mode", "2", None) {
            applied.push("NightColor.Mode=2".to_string());
        }
        if write_key(
            &binary,
            "NightColor",
            "NightTemperature",
            &config.night_temperature.to_string(),
            None,
        ) {
            applied.push(format!(
                "NightColor.NightTemperature={}",
                config.night_temperature
            ));
        }
    }
    applied.extend(per_output_notes(&config.outputs));
    reconfigure_kwin();
    if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
        let _ = atomic_write_text(TTL_PATH, &(now.as_secs() + TTL_SECS).to_string(), None);
    }
    let notes = applied;
    println!(
        "kyth-apply-vrr: {}",
        if notes.is_empty() {
            "nothing to apply".to_string()
        } else {
            notes.join("; ")
        }
    );
    std::process::ExitCode::SUCCESS
}
