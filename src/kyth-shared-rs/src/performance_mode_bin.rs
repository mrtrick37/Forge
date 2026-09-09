//! Native replacement for the Python `kyth-performance-mode` launcher.
//!
//! Switches performance presets (`max|gaming|performance|balanced|
//! powersave`), saves/restores the pre-switch state, and reports status.
//! CLI, messages, and exit codes mirror the Python launcher exactly
//! (`Unknown option/mode` + usage on stderr with exit 1). The
//! `performance.py` helpers it used stay as the Phase 3 fixture.

use std::env;
use std::process::ExitCode;
use std::time::Duration;

use kyth_shared::system::desktop_plasma::{kreadconfig_argv, kwriteconfig_argv, qdbus_candidates};
use kyth_shared::system::performance_mode::{
    get_current_epp, get_power_profile, mode_settings, read_state_key, render_state, set_epp,
    set_power_profile, state_path,
};
use kyth_shared::system::process::run_bounded;

const USAGE: &str =
    "Usage: kyth-performance-mode [save|restore|status|max|gaming|performance|balanced|powersave]";

fn find_binary(name: &str) -> Option<String> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
            .map(|path| path.to_string_lossy().into_owned())
    })
}

fn kread(file: &str, group: &str, key: &str) -> Option<String> {
    let binary = find_binary("kreadconfig6").or_else(|| find_binary("kreadconfig"))?;
    run_bounded(
        &kreadconfig_argv(&binary, file, group, key),
        Duration::from_secs(5),
    )
    .ok()
    .and_then(|output| {
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
    })
}

fn kwrite(file: &str, group: &str, key: &str, value: &str) {
    if let Some(binary) = find_binary("kwriteconfig6").or_else(|| find_binary("kwriteconfig")) {
        let _ = run_bounded(
            &kwriteconfig_argv(&binary, file, &[group], key, value, None),
            Duration::from_secs(5),
        );
    }
}

fn reconfigure_kwin() {
    if let Some(qdbus) = qdbus_candidates().iter().find_map(|name| find_binary(name)) {
        let _ = run_bounded(
            &[
                qdbus,
                "org.kde.KWin".to_string(),
                "/KWin".to_string(),
                "reconfigure".to_string(),
            ],
            Duration::from_secs(5),
        );
    }
}

fn save_state() {
    if let Err(error) = save_state_inner() {
        eprintln!("kyth-performance-mode: failed to save state: {error}");
        std::process::exit(1);
    }
}

fn save_state_inner() -> std::io::Result<()> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let power_profile = get_power_profile();
    let anim_factor = kread("kdeglobals", "KDE", "AnimationDurationFactor").unwrap_or_default();
    let blur_enabled = kread("kwinrc", "Plugins", "blurEnabled").unwrap_or_default();
    let epp = get_current_epp();
    std::fs::write(
        &path,
        render_state(&power_profile, &anim_factor, &blur_enabled, &epp),
    )
}

fn restore_state() {
    let path = state_path();
    if !path.is_file() {
        return;
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("kyth-performance-mode: failed to restore state: {error}");
            std::process::exit(1);
        }
    };
    let power_profile = read_state_key(&text, "POWER_PROFILE");
    let anim_factor = read_state_key(&text, "ANIMATION_DURATION_FACTOR");
    let blur_enabled = read_state_key(&text, "BLUR_ENABLED");
    let epp = read_state_key(&text, "EPP");
    if !power_profile.is_empty() {
        set_power_profile(&power_profile);
    }
    if !anim_factor.is_empty() {
        kwrite("kdeglobals", "KDE", "AnimationDurationFactor", &anim_factor);
    }
    if !blur_enabled.is_empty() {
        kwrite("kwinrc", "Plugins", "blurEnabled", &blur_enabled);
    }
    if !epp.is_empty() {
        set_epp(&epp);
    }
    reconfigure_kwin();
    let _ = std::fs::remove_file(&path);
}

fn apply_mode(mode: &str) {
    let Some((profile, epp, anim, blur)) = mode_settings(mode) else {
        eprintln!("Unknown mode: {mode}");
        eprintln!("{USAGE}");
        std::process::exit(1);
    };
    set_power_profile(profile);
    set_epp(epp);
    kwrite("kdeglobals", "KDE", "AnimationDurationFactor", anim);
    kwrite("kwinrc", "Plugins", "blurEnabled", blur);
    reconfigure_kwin();
}

fn status_mode() {
    println!("Power profile: {}", get_power_profile());
    println!("CPU EPP: {}", get_current_epp());
    println!(
        "KDE animation factor: {}",
        kread("kdeglobals", "KDE", "AnimationDurationFactor")
            .unwrap_or_else(|| "default".to_string())
    );
    println!(
        "KWin blur enabled: {}",
        kread("kwinrc", "Plugins", "blurEnabled").unwrap_or_else(|| "default".to_string())
    );
}

fn main() -> ExitCode {
    let cmd = env::args().nth(1).unwrap_or_else(|| "status".to_string());
    match cmd.as_str() {
        "save" => save_state(),
        "restore" => restore_state(),
        "status" => status_mode(),
        "max" | "gaming" | "performance" | "balanced" | "powersave" => apply_mode(&cmd),
        _ => {
            eprintln!("Unknown option: {cmd}");
            eprintln!("{USAGE}");
            std::process::exit(1);
        }
    }
    ExitCode::SUCCESS
}
