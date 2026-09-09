//! Gaming section bridge: the tool grid (install/launch/uninstall), the
//! Discord/OBS one-shot capture fixes, the "open a well-known folder"
//! actions from the first-failure playbook / Fix My Game card, and the
//! overlay/sched-ext/per-game profile builder from
//! `page_gaming_tools_perf.py`. Catalog and command builders live in
//! `kyth_shared::system::gaming_tools`, `gaming_perf`, and `gaming_per_game`.

use std::process::Command;
use std::time::Duration;

use serde::Serialize;

use kyth_shared::system::gaming_per_game;
use kyth_shared::system::gaming_perf::{self, ProfileGoal};
use kyth_shared::system::gaming_tools::{self, GAMING_TOOLS};

use super::job::{failure_detail, spawn_argv_job, start_job};

#[derive(Serialize)]
pub(crate) struct GamingToolResponse {
    flatpak: String,
    name: String,
    desc: String,
    installed: bool,
}
#[derive(Serialize)]
pub(crate) struct GamingActionLaunch {
    pub(crate) job: String,
    pub(crate) state: String,
    pub(crate) detail: String,
}

#[tauri::command]
pub(crate) fn gaming_tools() -> Vec<GamingToolResponse> {
    GAMING_TOOLS
        .iter()
        .map(|tool| GamingToolResponse {
            flatpak: tool.flatpak.to_string(),
            name: tool.name.to_string(),
            desc: tool.desc.to_string(),
            installed: kyth_shared::system::software_catalog::is_flatpak_installed(tool.flatpak),
        })
        .collect()
}

fn validated_gaming_tool(flatpak_id: &str) -> Result<&'static gaming_tools::GamingTool, String> {
    gaming_tools::find_gaming_tool(flatpak_id).ok_or_else(|| "unknown gaming tool".to_string())
}

#[tauri::command]
pub(crate) fn gaming_tool_install(flatpak_id: String) -> Result<GamingActionLaunch, String> {
    let tool = validated_gaming_tool(&flatpak_id)?;
    let name = tool.name.to_string();
    let launch_detail = format!("Installing {name}…");
    let argv = vec![
        "bash".to_string(),
        "-c".to_string(),
        format!(
            "flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo && flatpak install -y flathub {flatpak_id}"
        ),
    ];
    let job = start_job("gaming-install", &format!("Installing {name}…"))?;
    spawn_argv_job(
        job.clone(),
        argv,
        Duration::from_secs(600),
        move |result| match result {
            Ok(output) if output.status.success() => {
                ("complete".to_string(), format!("{name} installed."))
            }
            Ok(output) => (
                "failed".to_string(),
                failure_detail("Installation", &output),
            ),
            Err(err) => (
                "failed".to_string(),
                format!("Could not start installation: {err}"),
            ),
        },
    );
    Ok(GamingActionLaunch {
        job,
        state: "running".into(),
        detail: launch_detail,
    })
}

#[tauri::command]
pub(crate) fn gaming_tool_uninstall(flatpak_id: String) -> Result<GamingActionLaunch, String> {
    let tool = validated_gaming_tool(&flatpak_id)?;
    let name = tool.name.to_string();
    let launch_detail = format!("Uninstalling {name}…");
    let argv = vec![
        "flatpak".to_string(),
        "uninstall".to_string(),
        "-y".to_string(),
        flatpak_id,
    ];
    let job = start_job("gaming-uninstall", &format!("Uninstalling {name}…"))?;
    spawn_argv_job(
        job.clone(),
        argv,
        Duration::from_secs(120),
        move |result| match result {
            Ok(output) if output.status.success() => {
                ("complete".to_string(), format!("{name} uninstalled."))
            }
            Ok(output) => ("failed".to_string(), failure_detail("Uninstall", &output)),
            Err(err) => (
                "failed".to_string(),
                format!("Could not start uninstall: {err}"),
            ),
        },
    );
    Ok(GamingActionLaunch {
        job,
        state: "running".into(),
        detail: launch_detail,
    })
}

#[tauri::command]
pub(crate) fn gaming_tool_launch(flatpak_id: String) -> Result<String, String> {
    let tool = validated_gaming_tool(&flatpak_id)?;
    Command::new(tool.launch[0])
        .args(&tool.launch[1..])
        .spawn()
        .map_err(|err| format!("could not launch {}: {err}", tool.name))?;
    Ok(format!("{} launched.", tool.name))
}

#[tauri::command]
pub(crate) fn gaming_job_status(job: String) -> crate::InstallStatus {
    super::job::job_status(job)
}

/// One-shot Flatpak permission repairs — bounded, `--user`-scoped, no sudo.
/// Fast enough to run synchronously rather than as a background job, same
/// as `apply_pipewire_quantum`/`apply_plasma_preset`.
fn run_capture_fix(action: &str, argv: Vec<String>) -> Result<String, String> {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    match kyth_shared::system::process::run_bounded_command(command, Duration::from_secs(10)) {
        Ok(output) if output.status.success() => {
            Ok(format!("{action} applied. Restart the app to take effect."))
        }
        Ok(output) => Err(failure_detail(action, &output)),
        Err(err) => Err(format!("Could not run {action}: {err}")),
    }
}

#[tauri::command]
pub(crate) fn fix_discord_screenshare() -> Result<String, String> {
    run_capture_fix(
        "Discord screen share repair",
        gaming_tools::discord_screenshare_fix_command(),
    )
}

#[tauri::command]
pub(crate) fn fix_obs_pipewire() -> Result<String, String> {
    run_capture_fix(
        "OBS capture repair",
        gaming_tools::obs_pipewire_fix_command(),
    )
}

/// Opens one of the two well-known game-data folders in the desktop file
/// manager. `key` is validated against `game_folder_path`'s fixed set —
/// never an arbitrary caller-supplied path.
#[tauri::command]
pub(crate) fn open_game_folder(key: String) -> Result<String, String> {
    let raw = gaming_tools::game_folder_path(&key).ok_or_else(|| "unknown folder".to_string())?;
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    let expanded = raw.replacen('~', &home, 1);
    if !std::path::Path::new(&expanded).exists() {
        return Err(format!("Folder not found yet: {expanded}"));
    }
    Command::new("xdg-open")
        .arg(&expanded)
        .spawn()
        .map_err(|err| format!("could not open {expanded}: {err}"))?;
    Ok(format!("Opened {expanded}"))
}

// ---------------------------------------------------------------------
// Overlays / sched-ext / per-game profile builder — page_gaming_tools_perf.py.
// ---------------------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct GamingPerfStatusResponse {
    mangohud_installed: bool,
    gamescope_installed: bool,
    vkbasalt_installed: bool,
}

#[tauri::command]
pub(crate) fn gaming_perf_status() -> GamingPerfStatusResponse {
    GamingPerfStatusResponse {
        mangohud_installed: gaming_perf::mangohud_installed(),
        gamescope_installed: gaming_perf::gamescope_installed(),
        vkbasalt_installed: gaming_perf::vkbasalt_installed(),
    }
}

#[derive(Serialize)]
pub(crate) struct ScxStatusResponse {
    active: bool,
    configured: String,
}

#[tauri::command]
pub(crate) fn scx_status() -> Option<ScxStatusResponse> {
    gaming_perf::scx_status().map(|status| ScxStatusResponse {
        active: status.active,
        configured: status.configured,
    })
}

/// Only the two schedulers the Hub's buttons actually offer ("Use
/// scx_rusty", "Stop scx") — a fixed set, not an arbitrary scheduler name
/// from the webview.
#[tauri::command]
pub(crate) fn scx_set_scheduler(scheduler: String) -> Result<String, String> {
    if !matches!(scheduler.as_str(), "rusty" | "stop") {
        return Err("unknown scheduler".to_string());
    }
    let argv = gaming_perf::scx_scheduler_command(&scheduler);
    let job = start_job("scx", &format!("Setting scheduler: {scheduler}…"))?;
    spawn_argv_job(
        job.clone(),
        argv,
        Duration::from_secs(30),
        |result| match result {
            Ok(output) if output.status.success() => {
                ("complete".to_string(), "sched-ext updated.".to_string())
            }
            Ok(output) => (
                "failed".to_string(),
                failure_detail("sched-ext update", &output),
            ),
            Err(err) => (
                "failed".to_string(),
                format!("Could not start sched-ext update: {err}"),
            ),
        },
    );
    Ok(job)
}

fn valid_appid(appid: &str) -> bool {
    !appid.is_empty()
        && appid.len() <= 64
        && !appid.contains('"')
        && !appid.chars().any(char::is_control)
}

#[derive(Serialize)]
pub(crate) struct GameProfileResponse {
    profile: String,
    hdr: bool,
}

#[tauri::command]
pub(crate) fn per_game_profile(appid: String) -> Result<GameProfileResponse, String> {
    if !valid_appid(&appid) {
        return Err("invalid Steam app id".to_string());
    }
    let profile = gaming_per_game::get_profile_for_appid(
        &appid,
        gaming_per_game::per_game_config_path(None::<&str>),
    );
    Ok(GameProfileResponse {
        profile: profile.profile,
        hdr: profile.hdr,
    })
}

#[tauri::command]
pub(crate) fn save_per_game_profile(
    appid: String,
    profile: String,
    hdr: bool,
) -> Result<String, String> {
    if !valid_appid(&appid) {
        return Err("invalid Steam app id".to_string());
    }
    if ProfileGoal::parse(&profile).is_none() {
        return Err("unknown profile".to_string());
    }
    gaming_per_game::set_profile_for_appid(
        &appid,
        &profile,
        hdr,
        gaming_per_game::per_game_config_path(None::<&str>),
    )
    .map_err(|err| format!("Could not save profile: {err}"))?;
    Ok(format!("Saved {profile} (HDR: {hdr}) for {appid}."))
}
