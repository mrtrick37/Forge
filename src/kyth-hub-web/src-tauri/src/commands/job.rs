//! Shared background-job runner for bounded shell/argv actions that take too
//! long to block a Tauri command — installs, container lifecycle steps,
//! recipe-adjacent fixes. One job store, one `job_status` poll command;
//! every domain module (`security`, `gaming`) gets its own `start_job`
//! prefix so job ids stay readable in logs, but the store and polling
//! contract are identical everywhere. Reports running/complete/failed, not
//! a live percentage — see `security_container`'s module doc for why.

use std::collections::HashMap;
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

static JOBS: OnceLock<Mutex<HashMap<String, (String, String)>>> = OnceLock::new();

fn jobs() -> &'static Mutex<HashMap<String, (String, String)>> {
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn new_job_id(prefix: &str) -> String {
    format!(
        "{prefix}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}

pub(crate) fn start_job(prefix: &str, pending: &str) -> Result<String, String> {
    let job = new_job_id(prefix);
    jobs()
        .lock()
        .map_err(|_| "job store is unavailable".to_string())?
        .insert(job.clone(), ("running".into(), pending.to_string()));
    Ok(job)
}

fn finish_job(job: String, state: &str, detail: String) {
    if let Ok(mut store) = jobs().lock() {
        store.insert(job, (state.to_string(), detail));
    }
}

#[tauri::command]
pub(crate) fn job_status(job: String) -> crate::InstallStatus {
    let (state, detail) = jobs()
        .lock()
        .ok()
        .and_then(|store| store.get(&job).cloned())
        .unwrap_or(("unknown".into(), "Job not found.".into()));
    crate::InstallStatus {
        id: job,
        state,
        detail,
    }
}

/// Same truncation/direction convention as the Hub action output helper:
/// keep the tail of combined stdout+stderr, since that's where the actual
/// error usually is in apt/flatpak/distrobox output.
pub(crate) fn failure_detail(action: &str, output: &Output) -> String {
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
    let tail: String = text
        .chars()
        .rev()
        .take(500)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if tail.trim().is_empty() {
        match output.status.code() {
            Some(code) => format!("{action} failed (exit {code})."),
            None => format!("{action} stopped before it could complete."),
        }
    } else {
        format!("{action} failed — {}", tail.trim())
    }
}

fn askpass_env(command: &mut Command) {
    if std::path::Path::new("/usr/bin/ksshaskpass").exists() {
        command.env("SUDO_ASKPASS", "/usr/bin/ksshaskpass");
    }
}

pub(crate) fn spawn_argv_job(
    job: String,
    argv: Vec<String>,
    timeout: Duration,
    on_done: impl FnOnce(Result<Output, std::io::Error>) -> (String, String) + Send + 'static,
) {
    std::thread::spawn(move || {
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]);
        askpass_env(&mut command);
        let result = kyth_shared::system::process::run_bounded_command(command, timeout);
        let (state, detail) = on_done(result);
        finish_job(job, &state, detail);
    });
}
