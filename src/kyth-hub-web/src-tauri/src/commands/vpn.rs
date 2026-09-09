use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use url::Url;

use crate::InstallStatus;

static JOBS: OnceLock<Mutex<HashMap<String, Arc<VpnRuntime>>>> = OnceLock::new();

fn jobs() -> &'static Mutex<HashMap<String, Arc<VpnRuntime>>> {
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

struct VpnRuntime {
    status: Mutex<(String, String)>,
    child: Mutex<Option<Child>>,
    stopped: AtomicBool,
    generation: AtomicU64,
    gateway: String,
    protocol: String,
    os_emulation: String,
    username: String,
    interface: Mutex<String>,
}

fn status(runtime: &VpnRuntime, state: &str, detail: impl Into<String>) {
    if let Ok(mut current) = runtime.status.lock() {
        *current = (state.to_string(), detail.into());
    }
}

fn get_job(job: &str) -> Result<Arc<VpnRuntime>, String> {
    jobs()
        .lock()
        .map_err(|_| "VPN job store is unavailable".to_string())?
        .get(job)
        .cloned()
        .ok_or_else(|| "VPN job not found".to_string())
}

fn save_profile(
    gateway: &str,
    protocol: &str,
    os_emulation: &str,
    username: &str,
) -> Result<(), String> {
    let home = std::env::var_os("HOME").ok_or_else(|| "HOME is unavailable".to_string())?;
    let path = PathBuf::from(home).join(".config/kyth-vpn-connect");
    let parent = path
        .parent()
        .ok_or_else(|| "VPN config path is invalid".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create VPN config directory: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    let content = format!("[vpn]\ngateway = {gateway}\nprotocol = {protocol}\nos = {os_emulation}\nusername = {username}\n");
    kyth_shared::atomic_io::atomic_write_text(&path, &content, Some(0o600))
        .map_err(|error| format!("could not save VPN profile: {error}"))
}

fn terminate_child(runtime: &VpnRuntime) {
    if let Ok(mut child) = runtime.child.lock() {
        if let Some(child) = child.as_mut() {
            let _ = child.kill();
        }
    }
}

fn reader<R: std::io::Read + Send + 'static>(stream: R, tx: mpsc::Sender<String>) {
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        let _ = tx.send(line);
    }
}

fn start_process(
    runtime: Arc<VpnRuntime>,
    app: AppHandle,
    job: String,
    command: kyth_shared::system::vpn_saml::OpenconnectCommand,
) {
    let generation = runtime.generation.fetch_add(1, Ordering::SeqCst) + 1;
    thread::spawn(move || {
        let Some((program, args)) = command.argv.split_first() else {
            status(&runtime, "failed", "VPN command was empty.");
            return;
        };
        let mut child_command = Command::new(program);
        child_command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if std::path::Path::new("/usr/bin/ksshaskpass").exists() {
            child_command.env("SUDO_ASKPASS", "/usr/bin/ksshaskpass");
        }
        let mut child = match child_command.spawn() {
            Ok(child) => child,
            Err(error) => {
                status(&runtime, "failed", format!("Could not start VPN: {error}"));
                return;
            }
        };
        if let Some(mut stdin) = child.stdin.take() {
            if let Some(input) = command.stdin {
                let _ = stdin.write_all(input.as_bytes());
            }
        }
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        if let Ok(mut slot) = runtime.child.lock() {
            *slot = Some(child);
        }
        let (tx, rx) = mpsc::channel();
        let mut readers = 0;
        if let Some(stdout) = stdout {
            readers += 1;
            let tx = tx.clone();
            thread::spawn(move || reader(stdout, tx));
        }
        if let Some(stderr) = stderr {
            readers += 1;
            let tx = tx.clone();
            thread::spawn(move || reader(stderr, tx));
        }
        drop(tx);
        let mut saml_opened = false;
        while readers > 0 {
            match rx.recv() {
                Ok(line) => {
                    let redacted = kyth_shared::system::vpn_saml::redact_log_line(&line);
                    if let Some(interface) =
                        kyth_shared::system::vpn_saml::gp_interface_from_log_line(&line)
                    {
                        if let Ok(mut current) = runtime.interface.lock() {
                            *current = interface.to_string();
                        }
                    }
                    if let Some(saml_url) =
                        kyth_shared::system::vpn_saml::saml_url_from_log_line(&line)
                    {
                        status(
                            &runtime,
                            "authentication_required",
                            "VPN sign-in is required; complete the secure sign-in window.",
                        );
                        if !saml_opened {
                            saml_opened = true;
                            open_saml_window(&app, &job, &runtime.gateway, &saml_url);
                        }
                    } else if kyth_shared::system::vpn_saml::line_is_connected(&line) {
                        status(&runtime, "connected", "VPN connection established.");
                    } else if !redacted.trim().is_empty() {
                        status(&runtime, "connecting", redacted);
                    }
                }
                Err(_) => break,
            }
        }
        let exit_success = runtime
            .child
            .lock()
            .ok()
            .and_then(|mut slot| {
                slot.as_mut()
                    .and_then(|child| child.wait().ok().map(|exit| exit.success()))
            })
            .unwrap_or(false);
        if runtime.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        if let Ok(mut slot) = runtime.child.lock() {
            *slot = None;
        }
        if runtime.stopped.load(Ordering::SeqCst) {
            return;
        }
        let current = runtime
            .status
            .lock()
            .ok()
            .map(|value| value.0.clone())
            .unwrap_or_default();
        if current == "authentication_required" {
            return;
        }
        if exit_success {
            status(&runtime, "disconnected", "VPN connection ended.");
        } else {
            status(&runtime, "failed", "VPN connection ended unexpectedly.");
        }
    });
}

fn start_reconnect(app: AppHandle, job: String, cookie: String) {
    let Ok(runtime) = get_job(&job) else {
        return;
    };
    terminate_child(&runtime);
    let interface = runtime
        .interface
        .lock()
        .ok()
        .map(|value| value.clone())
        .unwrap_or_else(|| "portal".into());
    match kyth_shared::system::vpn_saml::build_reconnect_command(
        &runtime.gateway,
        &runtime.protocol,
        &runtime.os_emulation,
        &interface,
        &cookie,
        &runtime.username,
    ) {
        Ok(command) => {
            runtime.stopped.store(false, Ordering::SeqCst);
            status(
                &runtime,
                "connecting",
                "SAML sign-in complete; reconnecting VPN…",
            );
            start_process(runtime, app, job, command);
        }
        Err(_) => status(
            &runtime,
            "failed",
            "VPN authentication response was invalid.",
        ),
    }
}

fn callback_value(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find_map(|(name, value)| (name == key).then(|| value.into_owned()))
}

fn handle_saml_callback(
    app: AppHandle,
    label: String,
    job: String,
    gateway: String,
    callback: String,
) {
    thread::spawn(move || {
        let Ok(url) = Url::parse(&callback) else {
            if let Ok(runtime) = get_job(&job) {
                status(&runtime, "failed", "VPN sign-in callback was invalid.");
            }
            return;
        };
        if callback.len() > 8 * 1024 * 1024
            || url.scheme() != "kyth-vpn"
            || url.host_str() != Some("saml-acs")
        {
            if let Ok(runtime) = get_job(&job) {
                status(&runtime, "failed", "VPN sign-in callback was rejected.");
            }
            return;
        }
        if let Some(cookie) = callback_value(&url, "cookie") {
            if let Some(window) = app.get_webview_window(&label) {
                let _ = window.close();
            }
            start_reconnect(app, job, cookie);
            return;
        }
        let (Some(action_url), Some(body)) =
            (callback_value(&url, "url"), callback_value(&url, "body"))
        else {
            return;
        };
        let Ok((argv, input)) =
            kyth_shared::system::vpn_saml::replay_saml_command(&action_url, &body, &gateway)
        else {
            if let Ok(runtime) = get_job(&job) {
                status(
                    &runtime,
                    "failed",
                    "VPN sign-in response failed validation.",
                );
            }
            return;
        };
        let response = kyth_shared::system::process::run_bounded_with_input(
            &argv,
            &input,
            std::time::Duration::from_secs(35),
        );
        let cookie = response.ok().and_then(|output| {
            let text = String::from_utf8_lossy(&output.stdout);
            let boundary = text.rfind("\r\n\r\n").or_else(|| text.rfind("\n\n"));
            let (headers, body) = boundary.map_or((text.as_ref(), ""), |index| {
                let split = if text[index..].starts_with("\r\n") {
                    4
                } else {
                    2
                };
                (&text[..index], &text[index + split..])
            });
            kyth_shared::system::vpn_saml::parse_saml_acs_response(headers, body)
        });
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.close();
        }
        match cookie {
            Some(cookie) => start_reconnect(app, job, cookie),
            None => {
                if let Ok(runtime) = get_job(&job) {
                    status(
                        &runtime,
                        "failed",
                        "VPN portal did not return an authentication token.",
                    );
                }
            }
        }
    });
}

fn open_saml_window(app: &AppHandle, job: &str, gateway: &str, saml_url: &str) {
    let label = format!("vpn-saml-{job}");
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    if kyth_shared::system::vpn_saml::validate_saml_redirect_url(saml_url).is_err() {
        if let Ok(runtime) = get_job(job) {
            status(&runtime, "failed", "VPN sign-in redirect was rejected.");
        }
        return;
    }
    let callback_app = app.clone();
    let callback_label = label.clone();
    let callback_job = job.to_string();
    let callback_gateway = gateway.to_string();
    let init_script = r#"(function(){
      function submitToKyth(form){
        var action=form.action||''; if(action.indexOf('/SAML20/SP/ACS')<0)return false;
        var fd; try{fd=new FormData(form)}catch(e){return false}; if(!fd.get('SAMLResponse'))return false;
        var p=new URLSearchParams(); for(var pair of fd.entries())p.append(pair[0],pair[1]);
        window.location.href='kyth-vpn://saml-acs?url='+encodeURIComponent(action)+'&body='+encodeURIComponent(p.toString()); return true;
      }
      var original=HTMLFormElement.prototype.submit; HTMLFormElement.prototype.submit=function(){if(!submitToKyth(this))original.call(this)};
      document.addEventListener('submit',function(e){if(submitToKyth(e.target)){e.preventDefault();e.stopImmediatePropagation()}},true);
    })();"#;
    let Ok(initial_url) = Url::parse(saml_url) else {
        if let Ok(runtime) = get_job(job) {
            status(&runtime, "failed", "VPN sign-in redirect was invalid.");
        }
        return;
    };
    if initial_url.scheme() != "https"
        || initial_url.host_str().is_none()
        || !initial_url.username().is_empty()
        || initial_url.password().is_some()
        || initial_url.fragment().is_some()
    {
        if let Ok(runtime) = get_job(job) {
            status(&runtime, "failed", "VPN sign-in redirect was rejected.");
        }
        return;
    }
    let result = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(initial_url))
        .title("VPN — Secure sign-in")
        .inner_size(960.0, 720.0)
        .initialization_script(init_script)
        .on_navigation(move |url| {
            if url.scheme() == "kyth-vpn" && url.host_str() == Some("saml-acs") {
                handle_saml_callback(
                    callback_app.clone(),
                    callback_label.clone(),
                    callback_job.clone(),
                    callback_gateway.clone(),
                    url.as_str().to_string(),
                );
                false
            } else {
                true
            }
        })
        .build();
    if result.is_err() {
        if let Ok(runtime) = get_job(job) {
            status(
                &runtime,
                "failed",
                "Could not open the secure VPN sign-in window.",
            );
        }
    }
}

#[tauri::command]
pub(crate) fn open_vpn_app(app: AppHandle) -> Result<String, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Hub window is unavailable".to_string())?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    let _ = window.emit("navigate", "VPN");
    Ok("Opened native VPN controls in the Hub.".into())
}

#[tauri::command]
pub(crate) fn vpn_connect(
    app: AppHandle,
    gateway: String,
    protocol: String,
    os_emulation: String,
    username: String,
    password: String,
) -> Result<String, String> {
    let command = kyth_shared::system::vpn_saml::build_initial_command(
        &gateway,
        &protocol,
        &os_emulation,
        &username,
        &password,
    )?;
    save_profile(&gateway, &protocol, &os_emulation, &username)?;
    let job = format!(
        "vpn-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let runtime = Arc::new(VpnRuntime {
        status: Mutex::new(("connecting".into(), "Starting VPN connection…".into())),
        child: Mutex::new(None),
        stopped: AtomicBool::new(false),
        generation: AtomicU64::new(0),
        gateway,
        protocol,
        os_emulation,
        username,
        interface: Mutex::new("portal".into()),
    });
    jobs()
        .lock()
        .map_err(|_| "VPN job store is unavailable".to_string())?
        .insert(job.clone(), runtime.clone());
    start_process(runtime, app, job.clone(), command);
    Ok(job)
}

#[tauri::command]
pub(crate) fn vpn_status(job: String) -> InstallStatus {
    let Ok(runtime) = get_job(&job) else {
        return InstallStatus {
            id: job,
            state: "unknown".into(),
            detail: "VPN job not found.".into(),
        };
    };
    let (state, detail) = runtime
        .status
        .lock()
        .ok()
        .map(|value| value.clone())
        .unwrap_or(("unknown".into(), "VPN status unavailable.".into()));
    InstallStatus {
        id: job,
        state,
        detail,
    }
}

#[tauri::command]
pub(crate) fn vpn_disconnect(job: String) -> Result<String, String> {
    let runtime = get_job(&job)?;
    runtime.stopped.store(true, Ordering::SeqCst);
    runtime.generation.fetch_add(1, Ordering::SeqCst);
    terminate_child(&runtime);
    status(&runtime, "complete", "VPN disconnected.");
    Ok("VPN disconnected.".into())
}
