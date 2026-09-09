use std::collections::HashMap;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};

static HUB_ACTION_JOBS: OnceLock<Mutex<HashMap<String, (String, String)>>> = OnceLock::new();
static UPDATE_JOBS: OnceLock<Mutex<HashMap<String, (String, String)>>> = OnceLock::new();

fn hub_action_jobs() -> &'static Mutex<HashMap<String, (String, String)>> {
    HUB_ACTION_JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn update_jobs() -> &'static Mutex<HashMap<String, (String, String)>> {
    UPDATE_JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Serialize)]
pub(crate) struct JustRecipeResponse {
    pub(crate) name: String,
    pub(crate) params: String,
    pub(crate) comment: String,
}

#[tauri::command]
pub(crate) fn just_list() -> Vec<JustRecipeResponse> {
    kyth_shared::system::just::just_list()
        .into_iter()
        .map(|recipe| JustRecipeResponse {
            name: recipe.name,
            params: recipe.params,
            comment: recipe.comment,
        })
        .collect()
}

fn just_output_detail(recipe: &str, output: &std::process::Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    let text = kyth_shared::system::process::redact_sensitive_text(
        kyth_shared::system::process::strip_ansi(text.trim()).as_str(),
    );
    let detail: String = text
        .chars()
        .rev()
        .take(800)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if !detail.trim().is_empty() {
        return if output.status.success() {
            format!("{recipe} complete — {}", detail.trim())
        } else {
            format!("{recipe} could not be completed — {}", detail.trim())
        };
    }
    if output.status.success() {
        format!("{recipe} complete.")
    } else {
        match output.status.code() {
            Some(code) => format!("{recipe} could not be completed (exit code {code})."),
            None => format!("{recipe} stopped before it could complete."),
        }
    }
}

#[derive(Serialize)]
pub(crate) struct HubActionLaunch {
    pub(crate) job: String,
    pub(crate) state: String,
    pub(crate) detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum HubAction {
    SetupTailscale,
    UpdateHealth,
    ResumeCheck,
    DeviceInfo,
    StartupApps,
    FirmwareUpdate,
    HealthCheck,
    GamingStackStatus,
    FixDualbootClock,
    SetupBootWindowsSteam,
    ReclaimWindows,
    InstallLudusavi,
    InstallMsFonts,
    SetupKythDevBox,
    AiDevStatus,
    AiDevSetup,
    SetupWaydroid,
    RemoveWaydroid,
    InstallVscode,
    InstallBoxbuddy,
    InstallJetbrainsToolbox,
    GamingMode,
    BalancedMode,
    HdrPerGame,
    EnableBpftune,
    DisableBpftune,
    InstallSteam,
    InstallHeroic,
    InstallLutris,
    InstallBottles,
    InstallPrismlauncher,
    InstallItch,
    InstallEpicLauncher,
    InstallBattlenet,
    InstallEaApp,
    InstallUbisoftConnect,
    PreheatShaders,
    EnableObsCapture,
    GameBoost,
    ControllerCheck,
    ExportSteamGames,
    InstallObs,
    InstallGpuScreenRecorder,
    InstallGoverlay,
    InstallMangojuice,
    InstallUmu,
    InstallLact,
    InstallPiper,
    InstallSolaar,
    NvidiaStatus,
    ListPresets,
    SetupPrinter,
    EnrollSecureboot,
    SystemAudit,
    GamingAudit,
}

impl HubAction {
    fn recipe(&self) -> &'static str {
        match self {
            Self::SetupTailscale => "setup-tailscale",
            Self::UpdateHealth => "update-health",
            Self::ResumeCheck => "resume-check",
            Self::DeviceInfo => "device-info",
            Self::StartupApps => "startup-apps",
            Self::FirmwareUpdate => "firmware-update",
            Self::HealthCheck => "health-check",
            Self::GamingStackStatus => "gaming-stack-status",
            Self::FixDualbootClock => "fix-dualboot-clock",
            Self::SetupBootWindowsSteam => "setup-boot-windows-steam",
            Self::ReclaimWindows => "reclaim-windows",
            Self::InstallLudusavi => "install-ludusavi",
            Self::InstallMsFonts => "install-ms-fonts",
            Self::SetupKythDevBox => "setup-kyth-dev-box",
            Self::AiDevStatus => "ai-dev-status",
            Self::AiDevSetup => "ai-dev-setup",
            Self::SetupWaydroid => "setup-waydroid",
            Self::RemoveWaydroid => "remove-waydroid",
            Self::InstallVscode => "install-vscode",
            Self::InstallBoxbuddy => "install-boxbuddy",
            Self::InstallJetbrainsToolbox => "install-jetbrains-toolbox",
            Self::GamingMode => "gaming-mode",
            Self::BalancedMode => "balanced-mode",
            Self::HdrPerGame => "hdr-per-game",
            Self::EnableBpftune => "enable-bpftune",
            Self::DisableBpftune => "disable-bpftune",
            Self::InstallSteam => "install-steam",
            Self::InstallHeroic => "install-heroic",
            Self::InstallLutris => "install-lutris",
            Self::InstallBottles => "install-bottles",
            Self::InstallPrismlauncher => "install-prismlauncher",
            Self::InstallItch => "install-itch",
            Self::InstallEpicLauncher => "install-epic-launcher",
            Self::InstallBattlenet => "install-battlenet",
            Self::InstallEaApp => "install-ea-app",
            Self::InstallUbisoftConnect => "install-ubisoft-connect",
            Self::PreheatShaders => "preheat-shaders",
            Self::EnableObsCapture => "enable-obs-capture",
            Self::GameBoost => "game-boost",
            Self::ControllerCheck => "controller-check",
            Self::ExportSteamGames => "export-steam-games",
            Self::InstallObs => "install-obs",
            Self::InstallGpuScreenRecorder => "install-gpu-screen-recorder",
            Self::InstallGoverlay => "install-goverlay",
            Self::InstallMangojuice => "install-mangojuice",
            Self::InstallUmu => "install-umu",
            Self::InstallLact => "install-lact",
            Self::InstallPiper => "install-piper",
            Self::InstallSolaar => "install-solaar",
            Self::NvidiaStatus => "nvidia-status",
            Self::ListPresets => "list-presets",
            Self::SetupPrinter => "setup-printer",
            Self::EnrollSecureboot => "enroll-secureboot",
            Self::SystemAudit => "system-audit",
            Self::GamingAudit => "gaming-audit",
        }
    }
}

fn start_hub_action_job(action: HubAction) -> Result<HubActionLaunch, String> {
    let recipe = action.recipe();
    let argv = kyth_shared::system::just::command_for(recipe, &[])
        .ok_or_else(|| "Hub action is not allowlisted".to_string())?;
    kyth_shared::commands::normalize_command(&argv)
        .map_err(|_| "recipe produced an invalid command".to_string())?;
    let job = format!(
        "hub-action-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    hub_action_jobs()
        .lock()
        .map_err(|_| "Hub action job store is unavailable".to_string())?
        .insert(
            job.clone(),
            ("running".into(), format!("Running {recipe}…")),
        );
    let job_for_thread = job.clone();
    let recipe_for_thread = recipe.to_string();
    std::thread::spawn(move || {
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]);
        let inherited = std::env::vars().collect::<std::collections::BTreeMap<_, _>>();
        let sanitized = kyth_shared::commands::environment_for(
            kyth_shared::commands::EnvironmentPolicy::Sanitized,
            &inherited,
        );
        command.env_clear().envs(sanitized);
        kyth_shared::system::just::configure_command(&mut command);
        if std::path::Path::new("/usr/bin/ksshaskpass").exists() {
            command.env("SUDO_ASKPASS", "/usr/bin/ksshaskpass");
        }
        let result =
            kyth_shared::system::process::run_bounded_command(command, Duration::from_secs(900));
        let (state, detail) = match result {
            Ok(output) => {
                let state = if output.status.success() {
                    "complete"
                } else {
                    "failed"
                };
                (
                    state.to_string(),
                    just_output_detail(&recipe_for_thread, &output),
                )
            }
            Err(error) => (
                "failed".to_string(),
                format!("Could not start {recipe_for_thread}: {error}"),
            ),
        };
        if let Ok(mut store) = hub_action_jobs().lock() {
            store.insert(job_for_thread, (state, detail));
        }
    });
    Ok(HubActionLaunch {
        job,
        state: "running".into(),
        detail: format!("Running {recipe}…"),
    })
}

/// Start an Updates-page operation as a native Rust-managed job. The command
/// is always a fixed argv; `just` is intentionally not involved here. The
/// privileged safety helper remains the root boundary for upgrade policy and
/// boot-health recording, while Rust owns lifecycle, timeout, and UI output.
#[derive(Serialize)]
pub(crate) struct UpdateActionLaunch {
    pub(crate) job: String,
    pub(crate) state: String,
    pub(crate) detail: String,
}

fn start_update_job(
    operation: &str,
    argv: Vec<String>,
    timeout: Duration,
) -> Result<UpdateActionLaunch, String> {
    kyth_shared::commands::normalize_command(&argv)
        .map_err(|_| "update produced an invalid command".to_string())?;
    let job = format!(
        "update-{}-{}",
        operation,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    update_jobs()
        .lock()
        .map_err(|_| "update job store is unavailable".to_string())?
        .insert(
            job.clone(),
            ("running".into(), format!("{operation} is running…")),
        );
    let job_for_thread = job.clone();
    let operation_for_thread = operation.to_string();
    std::thread::spawn(move || {
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]);
        let inherited = std::env::vars().collect::<std::collections::BTreeMap<_, _>>();
        let desktop = kyth_shared::commands::environment_for(
            kyth_shared::commands::EnvironmentPolicy::Desktop,
            &inherited,
        );
        command.env_clear().envs(desktop);
        if std::path::Path::new("/usr/bin/ksshaskpass").exists() {
            command.env("SUDO_ASKPASS", "/usr/bin/ksshaskpass");
        }
        let result = kyth_shared::system::process::run_bounded_command(command, timeout);
        let (state, detail) = match result {
            Ok(output) => {
                let mut detail = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if !stderr.is_empty() {
                    if !detail.is_empty() {
                        detail.push('\n');
                    }
                    detail.push_str(&stderr);
                }
                let detail: String = kyth_shared::system::process::redact_sensitive_text(
                    kyth_shared::system::process::strip_ansi(&detail).as_str(),
                )
                .chars()
                .rev()
                .take(1200)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
                let state = if output.status.success() {
                    "complete"
                } else {
                    "failed"
                };
                let detail = if detail.is_empty() {
                    if output.status.success() {
                        format!("{operation_for_thread} complete.")
                    } else {
                        format!(
                            "{operation_for_thread} failed (exit code {}).",
                            output.status.code().unwrap_or(-1)
                        )
                    }
                } else {
                    detail
                };
                (state.to_string(), detail)
            }
            Err(error) => (
                "failed".to_string(),
                format!("{operation_for_thread} could not complete: {error}"),
            ),
        };
        if let Ok(mut store) = update_jobs().lock() {
            store.insert(job_for_thread, (state, detail));
        }
    });
    Ok(UpdateActionLaunch {
        job,
        state: "running".into(),
        detail: format!("{operation} is running…"),
    })
}

#[tauri::command]
pub(crate) fn run_hub_action(action: HubAction) -> Result<HubActionLaunch, String> {
    start_hub_action_job(action)
}

#[tauri::command]
pub(crate) fn hub_action_status(job: String) -> crate::InstallStatus {
    let (state, detail) = hub_action_jobs()
        .lock()
        .ok()
        .and_then(|store| store.get(&job).cloned())
        .unwrap_or(("unknown".into(), "Hub action job not found.".into()));
    crate::InstallStatus {
        id: job,
        state,
        detail,
    }
}

#[tauri::command]
pub(crate) fn bootc_upgrade() -> Result<UpdateActionLaunch, String> {
    if !std::path::Path::new("/usr/bin/kyth-safe-upgrade").exists() {
        return Err("The native KythOS update helper is not installed on this system.".to_string());
    }
    start_update_job(
        "Download and stage",
        vec!["sudo", "-A", "/usr/bin/kyth-safe-upgrade"]
            .into_iter()
            .map(String::from)
            .collect(),
        Duration::from_secs(3600),
    )
}

#[tauri::command]
pub(crate) fn bootc_rollback() -> Result<UpdateActionLaunch, String> {
    start_update_job(
        "Rollback",
        vec!["sudo", "-A", "/usr/bin/bootc", "rollback"]
            .into_iter()
            .map(String::from)
            .collect(),
        Duration::from_secs(300),
    )
}

#[tauri::command]
pub(crate) fn bootc_switch_branch(branch: String) -> Result<UpdateActionLaunch, String> {
    let channel = kyth_shared::system::bootc_policy::switch_channel_arg(&branch)
        .ok_or_else(|| "unknown channel".to_string())?;
    let operation = format!(
        "switch-{}",
        if channel == "stable" {
            "latest"
        } else {
            channel
        }
    );
    let mut argv = vec!["sudo", "-A", "/usr/bin/kyth-bootc-guard"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    argv.push(operation);
    start_update_job("Switch channel", argv, Duration::from_secs(300))
}

#[tauri::command]
pub(crate) fn apply_staged() -> Result<UpdateActionLaunch, String> {
    if !std::path::Path::new("/usr/libexec/kyth-finalize-staged").exists() {
        return Err("The staged-update finalizer is not installed on this system.".to_string());
    }
    start_update_job(
        "Apply staged update",
        vec!["sudo", "-A", "/usr/libexec/kyth-finalize-staged", "reboot"]
            .into_iter()
            .map(String::from)
            .collect(),
        Duration::from_secs(300),
    )
}

#[derive(Serialize)]
pub(crate) struct UpdateWatcherStatusResponse {
    pub(crate) available: bool,
    pub(crate) enabled: bool,
    pub(crate) active: bool,
}

fn systemd_unit_is(unit: &str, state: &str) -> bool {
    let argv = vec![
        "systemctl".to_string(),
        state.to_string(),
        "--quiet".to_string(),
        unit.to_string(),
    ];
    kyth_shared::system::process::run_bounded(&argv, Duration::from_secs(5))
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[tauri::command]
pub(crate) fn update_watcher_status() -> UpdateWatcherStatusResponse {
    let available = std::path::Path::new("/usr/bin/systemctl").exists()
        || std::path::Path::new("/bin/systemctl").exists();
    UpdateWatcherStatusResponse {
        available,
        enabled: available && systemd_unit_is("kyth-update-watcher.timer", "is-enabled"),
        active: available && systemd_unit_is("kyth-update-watcher.timer", "is-active"),
    }
}

#[tauri::command]
pub(crate) fn set_update_watcher_enabled(enabled: bool) -> Result<UpdateActionLaunch, String> {
    let action = if enabled { "enable" } else { "disable" };
    let operation = if enabled {
        "Enable automatic updates"
    } else {
        "Disable automatic updates"
    };
    start_update_job(
        operation,
        vec![
            "sudo",
            "-A",
            "systemctl",
            action,
            "--now",
            "kyth-update-watcher.timer",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        Duration::from_secs(300),
    )
}

#[tauri::command]
pub(crate) fn check_for_updates_now() -> Result<UpdateActionLaunch, String> {
    start_update_job(
        "Check for updates now",
        vec![
            "sudo",
            "-A",
            "systemctl",
            "start",
            "kyth-update-watcher.service",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        Duration::from_secs(300),
    )
}

#[tauri::command]
pub(crate) fn defer_update_watcher() -> Result<UpdateActionLaunch, String> {
    start_update_job(
        "Defer automatic updates",
        vec![
            "sudo",
            "-A",
            "systemctl",
            "stop",
            "kyth-update-watcher.timer",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        Duration::from_secs(300),
    )
}

#[tauri::command]
pub(crate) fn update_job_status(job: String) -> crate::InstallStatus {
    let (state, detail) = update_jobs()
        .lock()
        .ok()
        .and_then(|store| store.get(&job).cloned())
        .unwrap_or(("unknown".into(), "Update job not found.".into()));
    crate::InstallStatus {
        id: job,
        state,
        detail,
    }
}

#[tauri::command]
pub(crate) fn branch_display_name(tag: Option<String>) -> String {
    kyth_shared::system::bootc_policy::branch_display_name(tag.as_deref())
}

#[derive(Serialize)]
pub(crate) struct UpdateAvailabilityViewResponse {
    pub(crate) card_style: String,
    pub(crate) icon_text: String,
    pub(crate) icon_style: String,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) update_btn_visible: bool,
    pub(crate) restart_btn_visible: bool,
}

#[tauri::command]
pub(crate) fn update_availability_view(
    staged: bool,
    check_state: String,
    flatpak_count: u32,
    check_ts: String,
    check_ts_details: String,
    staged_ts: Option<String>,
) -> UpdateAvailabilityViewResponse {
    let view = kyth_shared::system::bootc_policy::update_availability_view(
        staged,
        &check_state,
        flatpak_count,
        &check_ts,
        &check_ts_details,
        staged_ts.as_deref(),
    );
    UpdateAvailabilityViewResponse {
        card_style: view.card_style,
        icon_text: view.icon_text,
        icon_style: view.icon_style,
        title: view.title,
        body: view.body,
        update_btn_visible: view.update_btn_visible,
        restart_btn_visible: view.restart_btn_visible,
    }
}

#[tauri::command]
pub(crate) async fn pending_updates_summary() -> std::collections::HashMap<String, String> {
    tauri::async_runtime::spawn_blocking(
        kyth_shared::system::updates_unified::pending_updates_summary,
    )
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub(crate) async fn update_status() -> UpdateStatusResponse {
    tauri::async_runtime::spawn_blocking(update_status_response)
        .await
        .unwrap_or_else(|_| UpdateStatusResponse {
            booted: None,
            staged: false,
            rollback: false,
            remote_digest: None,
            blocked_reason: Some("Could not read update status.".to_string()),
            retry_cmd: Some("bootc upgrade --check".to_string()),
            check_state: "error".to_string(),
            detail: "Could not read update status.".to_string(),
        })
}

fn update_status_response() -> UpdateStatusResponse {
    let status = kyth_shared::system::update_status::check_update_status();
    UpdateStatusResponse {
        booted: status.booted,
        staged: status.staged,
        rollback: status.rollback,
        remote_digest: status.remote_digest,
        blocked_reason: status.blocked_reason,
        retry_cmd: status.retry_cmd,
        check_state: status.check_state,
        detail: status.detail,
    }
}

#[derive(Serialize)]
pub(crate) struct UpdateStatusResponse {
    pub(crate) booted: Option<String>,
    pub(crate) staged: bool,
    pub(crate) rollback: bool,
    pub(crate) remote_digest: Option<String>,
    pub(crate) blocked_reason: Option<String>,
    pub(crate) retry_cmd: Option<String>,
    pub(crate) check_state: String,
    pub(crate) detail: String,
}

#[derive(Serialize)]
pub(crate) struct AvailabilityStatusResponse {
    pub(crate) state: String,
    pub(crate) detail: String,
    pub(crate) flatpak_count: i32,
    pub(crate) flatpak_detail: String,
    pub(crate) staged: bool,
    pub(crate) manifest_raw: String,
    pub(crate) blocked_reason: String,
}

#[tauri::command]
pub(crate) async fn collect_availability(
    branch: Option<String>,
    use_cached: Option<bool>,
) -> AvailabilityStatusResponse {
    let status = tauri::async_runtime::spawn_blocking(move || {
        kyth_shared::system::update_availability::collect_availability(
            branch.as_deref(),
            use_cached.unwrap_or(true),
        )
    })
    .await
    .unwrap_or_else(
        |_| kyth_shared::system::update_availability::AvailabilityStatus {
            state: "error".to_string(),
            detail: "Could not check update availability.".to_string(),
            flatpak_count: 0,
            flatpak_detail: String::new(),
            staged: false,
            manifest_raw: String::new(),
            blocked_reason: "Could not check update availability.".to_string(),
        },
    );
    AvailabilityStatusResponse {
        state: status.state,
        detail: status.detail,
        flatpak_count: status.flatpak_count,
        flatpak_detail: status.flatpak_detail,
        staged: status.staged,
        manifest_raw: status.manifest_raw,
        blocked_reason: status.blocked_reason,
    }
}

/// Resolve the active channel without making the short-lived probe cache a
/// hard dependency. The fallback can query bootc, so keep it off the Tauri
/// command/UI thread just like the update probes above.
#[tauri::command]
pub(crate) async fn current_update_channel() -> Option<String> {
    tauri::async_runtime::spawn_blocking(kyth_shared::system::bootc::current_branch)
        .await
        .ok()
        .flatten()
}

#[tauri::command]
pub(crate) fn updater_available() -> bool {
    kyth_shared::system::updater::updater_available()
}

#[derive(Serialize)]
pub(crate) struct UpdateHealthResponse {
    pub(crate) status: String,
    pub(crate) pending_digest: String,
    pub(crate) last_healthy_digest: String,
    pub(crate) failures: i64,
    pub(crate) quarantined: usize,
    pub(crate) detail: String,
}

fn native_health_fallback() -> Option<(String, String, String)> {
    // Prefer the same disk cache used by the rest of the Hub, but recover on
    // systems whose probe service has not populated it yet. This is still a
    // bounded native read and runs inside update_health's blocking worker.
    let status_data = kyth_shared::system::probe::read_section("bootc-status-data")
        .or_else(kyth_shared::system::bootc_query::fetch_status_data)?;
    let digest = kyth_shared::system::registry::booted_image_digest(&status_data)?;
    let os_release = std::fs::read_to_string("/usr/lib/os-release")
        .or_else(|_| std::fs::read_to_string("/etc/os-release"))
        .ok()?;
    let identity_ok = os_release
        .lines()
        .any(|line| line.trim() == "ID=kythos" || line.trim() == "ID=\"kythos\"");
    let runtime = kyth_shared::system::boot_runtime::boot_runtime_checks_with_deadline(
        std::time::Duration::from_secs(5),
        std::time::Duration::from_millis(100),
    );
    let mut failures = runtime
        .iter()
        .filter(|check| !check.passed)
        .map(|check| format!("{}: {}", check.name, check.detail))
        .collect::<Vec<_>>();
    if !identity_ok {
        failures.push("KythOS identity: /usr/lib/os-release is not ID=kythos".to_string());
    }
    if failures.is_empty() {
        Some((
            "healthy".to_string(),
            format!("Native boot checks passed for {digest}; no persistent boot-health record was available."),
            digest,
        ))
    } else {
        Some((
            "unhealthy".to_string(),
            format!("Native boot checks failed: {}", failures.join("; ")),
            digest,
        ))
    }
}

fn update_health_response() -> UpdateHealthResponse {
    let state = kyth_shared::system::boot_health::read_default_state();
    if state.status == "unknown"
        && state.current_digest.is_empty()
        && state.last_healthy_digest.is_empty()
        && state.updated_at == 0
    {
        if let Some((status, detail, digest)) = native_health_fallback() {
            return UpdateHealthResponse {
                status,
                pending_digest: state.pending_digest,
                last_healthy_digest: digest,
                failures: state.failures,
                quarantined: state.quarantined.len(),
                detail,
            };
        }
    }
    let invariants = state.invariants();
    let detail = if invariants.is_empty() {
        if state.status == "unknown" {
            "Boot health has not been recorded yet; native checks could not establish a live result.".to_string()
        } else {
            format!(
                "Boot health is {} · {} quarantined digest(s).",
                state.status,
                state.quarantined.len()
            )
        }
    } else {
        format!(
            "Boot health state needs attention: {}",
            invariants.join(", ")
        )
    };
    UpdateHealthResponse {
        status: state.status,
        pending_digest: state.pending_digest,
        last_healthy_digest: state.last_healthy_digest,
        failures: state.failures,
        quarantined: state.quarantined.len(),
        detail,
    }
}

#[tauri::command]
pub(crate) async fn update_health() -> UpdateHealthResponse {
    tauri::async_runtime::spawn_blocking(update_health_response)
        .await
        .unwrap_or_else(|_| UpdateHealthResponse {
            status: "unknown".to_string(),
            pending_digest: String::new(),
            last_healthy_digest: String::new(),
            failures: 0,
            quarantined: 0,
            detail: "Native boot-health check could not complete.".to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::HubAction;

    #[test]
    fn hub_action_deserializes_allowlisted_recipe() {
        let action: HubAction =
            serde_json::from_str("\"enroll-secureboot\"").expect("known action");
        assert_eq!(action.recipe(), "enroll-secureboot");
    }

    #[test]
    fn hub_action_rejects_unknown_recipe() {
        let result = serde_json::from_str::<HubAction>("\"run-arbitrary-command\"");
        assert!(result.is_err());
    }
}
