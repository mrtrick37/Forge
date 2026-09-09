//! Security tab bridge: the Kali distrobox lifecycle (create/enter/export/
//! remove) and the host-side (Flatpak) security-tools grid. Command argv
//! comes entirely from `kyth_shared::system::security_container`'s fixed
//! templates/catalog — nothing here accepts a free-form command or Flatpak
//! id from the webview.
//!
//! Jobs report running/complete/failed like `run_hub_action` and the App Store's
//! Flatpak install job, not a live percentage — see
//! `security_container`'s module doc for why the Python progress-bar
//! parser wasn't ported.

use std::process::Command;
use std::time::Duration;

use serde::Serialize;

use kyth_shared::system::security_container::{
    self, KaliTier, DEFAULT_KALI_BOX, DEFAULT_KALI_IMAGE,
};

use super::job::{failure_detail, spawn_argv_job, start_job};

#[derive(Serialize)]
pub(crate) struct SecurityActionLaunch {
    pub(crate) job: String,
    pub(crate) state: String,
    pub(crate) detail: String,
}

#[tauri::command]
pub(crate) fn kali_status() -> bool {
    security_container::is_socket_capable_kali_box(DEFAULT_KALI_BOX)
}

#[tauri::command]
pub(crate) fn kali_create(tier: String) -> Result<SecurityActionLaunch, String> {
    let parsed = KaliTier::parse(&tier).ok_or_else(|| "unknown Kali tier".to_string())?;
    let argv =
        security_container::build_kali_create_command(DEFAULT_KALI_BOX, DEFAULT_KALI_IMAGE, parsed);
    let job = start_job("kali-create", "Pulling Kali container image…")?;
    // kali-linux-everything can pull 15-20GB; give it real headroom.
    spawn_argv_job(
        job.clone(),
        argv,
        Duration::from_secs(1800),
        move |result| match result {
            Ok(output) if output.status.success() => (
                "complete".to_string(),
                if parsed.has_gui() {
                    "Kali box created. GUI apps exported — check your application menu.".to_string()
                } else {
                    "Kali box created. Launch a terminal to start hacking.".to_string()
                },
            ),
            Ok(output) => ("failed".to_string(), failure_detail("Kali setup", &output)),
            Err(err) => (
                "failed".to_string(),
                format!("Could not start Kali setup: {err}"),
            ),
        },
    );
    Ok(SecurityActionLaunch {
        job,
        state: "running".into(),
        detail: "Pulling Kali container image…".into(),
    })
}

#[tauri::command]
pub(crate) fn kali_export() -> Result<SecurityActionLaunch, String> {
    let argv = security_container::build_kali_export_command(DEFAULT_KALI_BOX);
    let job = start_job("kali-export", "Scanning Kali container for GUI apps…")?;
    spawn_argv_job(
        job.clone(),
        argv,
        Duration::from_secs(300),
        |result| {
            match result {
        Ok(output) if output.status.code() == Some(2) => (
            "complete".to_string(),
            "No GUI apps found. kali-linux-headless only includes CLI tools. Re-create the box \
             with 'Default' or 'Everything' to get exportable GUI apps."
                .to_string(),
        ),
        Ok(output) if output.status.success() => {
            let count = security_container::parse_kali_export_count(&String::from_utf8_lossy(&output.stdout)).unwrap_or(0);
            let detail = if count == 0 {
                "No GUI apps exported. kali-linux-headless contains CLI tools only — remove this \
                 box and re-create it with 'Default' or 'Everything' to get exportable GUI apps."
                    .to_string()
            } else {
                format!("Exported {count} app(s) — they should appear in your application menu shortly.")
            };
            ("complete".to_string(), detail)
        }
        Ok(output) => ("failed".to_string(), failure_detail("Export", &output)),
        Err(err) => ("failed".to_string(), format!("Could not start export: {err}")),
    }
        },
    );
    Ok(SecurityActionLaunch {
        job,
        state: "running".into(),
        detail: "Scanning Kali container for GUI apps…".into(),
    })
}

#[tauri::command]
pub(crate) fn kali_remove() -> Result<SecurityActionLaunch, String> {
    let argv = security_container::build_kali_remove_command(DEFAULT_KALI_BOX);
    let job = start_job("kali-remove", "Stopping and removing Kali box…")?;
    spawn_argv_job(
        job.clone(),
        argv,
        Duration::from_secs(120),
        |result| match result {
            Ok(output) if output.status.success() => {
                ("complete".to_string(), "Kali box removed.".to_string())
            }
            Ok(output) => ("failed".to_string(), failure_detail("Removal", &output)),
            Err(err) => (
                "failed".to_string(),
                format!("Could not start removal: {err}"),
            ),
        },
    );
    Ok(SecurityActionLaunch {
        job,
        state: "running".into(),
        detail: "Stopping and removing Kali box…".into(),
    })
}

#[tauri::command]
pub(crate) fn kali_enter_terminal() -> Result<String, String> {
    let terminal = security_container::detect_terminal()
        .ok_or_else(|| "Could not find a terminal emulator to open.".to_string())?;
    let argv = security_container::kali_enter_argv(terminal, DEFAULT_KALI_BOX);
    Command::new(&argv[0])
        .args(&argv[1..])
        .spawn()
        .map_err(|err| format!("could not open a terminal: {err}"))?;
    Ok("Opened a Kali terminal.".to_string())
}

#[tauri::command]
pub(crate) fn security_job_status(job: String) -> crate::InstallStatus {
    super::job::job_status(job)
}

#[derive(Serialize)]
pub(crate) struct SecHostToolResponse {
    flatpak: String,
    name: String,
    desc: String,
    installed: bool,
}

#[tauri::command]
pub(crate) fn sec_host_tools() -> Vec<SecHostToolResponse> {
    security_container::SEC_HOST_TOOLS
        .iter()
        .map(|tool| SecHostToolResponse {
            flatpak: tool.flatpak.to_string(),
            name: tool.name.to_string(),
            desc: tool.desc.to_string(),
            installed: kyth_shared::system::software_catalog::is_flatpak_installed(tool.flatpak),
        })
        .collect()
}

fn validated_sec_tool(
    flatpak_id: &str,
) -> Result<&'static security_container::SecHostTool, String> {
    security_container::find_sec_host_tool(flatpak_id)
        .ok_or_else(|| "unknown security tool".to_string())
}

#[tauri::command]
pub(crate) fn sec_host_tool_install(flatpak_id: String) -> Result<SecurityActionLaunch, String> {
    let tool = validated_sec_tool(&flatpak_id)?;
    let name = tool.name.to_string();
    let launch_detail = format!("Installing {name}…");
    let argv = vec![
        "bash".to_string(),
        "-c".to_string(),
        format!(
            "flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo && flatpak install -y flathub {flatpak_id}"
        ),
    ];
    let job = start_job("sec-install", &format!("Installing {name}…"))?;
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
    Ok(SecurityActionLaunch {
        job,
        state: "running".into(),
        detail: launch_detail,
    })
}

#[tauri::command]
pub(crate) fn sec_host_tool_uninstall(flatpak_id: String) -> Result<SecurityActionLaunch, String> {
    let tool = validated_sec_tool(&flatpak_id)?;
    let name = tool.name.to_string();
    let launch_detail = format!("Uninstalling {name}…");
    let argv = vec![
        "flatpak".to_string(),
        "uninstall".to_string(),
        "-y".to_string(),
        flatpak_id,
    ];
    let job = start_job("sec-uninstall", &format!("Uninstalling {name}…"))?;
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
    Ok(SecurityActionLaunch {
        job,
        state: "running".into(),
        detail: launch_detail,
    })
}

#[tauri::command]
pub(crate) fn sec_host_tool_launch(flatpak_id: String) -> Result<String, String> {
    let tool = validated_sec_tool(&flatpak_id)?;
    Command::new("flatpak")
        .args(["run", tool.flatpak])
        .spawn()
        .map_err(|err| format!("could not launch {}: {err}", tool.name))?;
    Ok(format!("{} launched.", tool.name))
}
