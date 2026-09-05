// Unprivileged shell for the React installer frontend.
//
// The Python installer still owns all disk/boot operations. This process only
// embeds the frontend and exposes the fixed, allowlisted transport supplied
// by the root-owned launcher. There is deliberately no generic filesystem or
// command bridge.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod installer_accounts;
mod installer_bootc;
mod installer_configuration;
#[allow(dead_code)]
mod installer_disk;
mod installer_executor;
mod installer_journal;
mod installer_mount;
mod installer_plan;
mod installer_recovery;
mod installer_secure_boot;
mod installer_storage;
mod installer_stream;
mod installer_transaction;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

const BACKEND_URL: &str = "http://127.0.0.1:7777";
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

struct InstallerTokens(Mutex<Option<InstallerConnection>>);
struct InstallerStream(Mutex<Option<Arc<AtomicBool>>>);

#[derive(Clone, Deserialize, Serialize)]
struct InstallerConnection {
    base_url: String,
    bootstrap_token: String,
    session_token: String,
    transport: String,
    socket_path: Option<String>,
}

#[derive(Serialize)]
struct InstallerResponse {
    status: u16,
    body: String,
}

fn arg_value<S: AsRef<str>>(argv: &[S], name: &str) -> Option<String> {
    argv.iter()
        .position(|arg| arg.as_ref() == name)
        .and_then(|index| argv.get(index + 1))
        .map(|value| value.as_ref().to_string())
}

#[tauri::command]
fn installer_connection(
    state: tauri::State<InstallerTokens>,
) -> Result<InstallerConnection, String> {
    state
        .0
        .lock()
        .map_err(|_| "installer connection state unavailable".to_string())?
        .clone()
        .ok_or_else(|| "installer shell was not given backend tokens".to_string())
}

#[tauri::command]
fn installer_validate_plan(
    request: serde_json::Value,
) -> Result<installer_plan::InstallerPlan, String> {
    let input: installer_plan::InstallerPlanInput = serde_json::from_value(request)
        .map_err(|error| format!("invalid installer request: {error}"))?;
    installer_plan::build_plan(input)
}

#[tauri::command]
fn installer_recovery_guidance(status: String) -> installer_recovery::RecoveryGuidance {
    installer_recovery::rescue_guidance(Some(&status))
}

#[tauri::command]
fn installer_execution_plan(
    request: serde_json::Value,
) -> Result<installer_executor::InstallerExecutionPlan, String> {
    let input: installer_executor::InstallerExecutionInput = serde_json::from_value(request)
        .map_err(|error| format!("invalid installer execution request: {error}"))?;
    installer_executor::build_plan(input)
}

fn connection(state: &tauri::State<InstallerTokens>) -> Result<InstallerConnection, String> {
    state
        .0
        .lock()
        .map_err(|_| "installer connection state unavailable".to_string())?
        .clone()
        .ok_or_else(|| "installer shell was not given backend tokens".to_string())
}

fn allowlisted_path(method: &str, path: &str) -> bool {
    if path.is_empty() || path.len() > 4096 || path.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
    {
        return false;
    }
    let route = path.split('?').next().unwrap_or(path);
    match method {
        "GET" => matches!(
            route,
            "/api/config"
                | "/api/disks"
                | "/api/partitions"
                | "/api/free-space"
                | "/api/timezones"
                | "/api/locales"
                | "/api/keymaps"
                | "/api/disk/pending"
                | "/api/disk/filesystems"
                | "/api/report"
                | "/api/rescue/probe"
                | "/api/log"
        ),
        "POST" => matches!(
            route,
            "/api/start"
                | "/api/cancel"
                | "/api/reboot"
                | "/api/disk/new-table"
                | "/api/disk/create"
                | "/api/disk/delete"
                | "/api/disk/resize"
                | "/api/disk/format"
                | "/api/disk/set-mountpoint"
                | "/api/disk/pending/remove"
                | "/api/disk/commit"
                | "/api/disk/rollback"
                | "/api/rescue/logs-to-usb"
        ),
        _ => false,
    }
}

fn socket_path(value: &InstallerConnection) -> Result<&str, String> {
    if value.transport != "unix" {
        return Err("Unix-socket transport is not enabled".to_string());
    }
    value
        .socket_path
        .as_deref()
        .ok_or_else(|| "installer socket path is missing".to_string())
}

fn read_http_response<R: Read>(stream: R) -> Result<InstallerResponse, String> {
    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader
        .read_line(&mut status_line)
        .map_err(|err| format!("could not read installer response: {err}"))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "installer returned an invalid HTTP status".to_string())?
        .parse::<u16>()
        .map_err(|_| "installer returned an invalid HTTP status".to_string())?;
    let mut content_length = None;
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|err| format!("could not read installer headers: {err}"))?;
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "invalid installer response length".to_string())?,
            );
        }
    }
    let length =
        content_length.ok_or_else(|| "installer response had no bounded body".to_string())?;
    if length > MAX_RESPONSE_BYTES {
        return Err("installer response exceeded the size limit".to_string());
    }
    let mut body = vec![0_u8; length];
    reader
        .read_exact(&mut body)
        .map_err(|err| format!("could not read installer response body: {err}"))?;
    let body =
        String::from_utf8(body).map_err(|_| "installer response was not UTF-8".to_string())?;
    Ok(InstallerResponse { status, body })
}

fn send_socket_request(
    value: &InstallerConnection,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<InstallerResponse, String> {
    if !allowlisted_path(method, path) {
        return Err("installer route is not allowlisted".to_string());
    }
    let mut stream = UnixStream::connect(socket_path(value)?)
        .map_err(|err| format!("could not connect to installer service: {err}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(610))).ok();
    let payload = body.unwrap_or("");
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: kyth-installer.local\r\nX-Kyth-Session-Token: {}\r\nAccept: application/json\r\nContent-Length: {}\r\n\r\n{payload}",
        value.session_token, payload.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| format!("could not contact installer service: {err}"))?;
    read_http_response(stream)
}

fn send_http_request(
    value: &InstallerConnection,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<InstallerResponse, String> {
    if value.base_url != BACKEND_URL || !allowlisted_path(method, path) {
        return Err("installer HTTP route is not allowlisted".to_string());
    }
    let mut stream = TcpStream::connect("127.0.0.1:7777")
        .map_err(|err| format!("could not connect to installer HTTP service: {err}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(610))).ok();
    let payload = body.unwrap_or("");
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Kyth-Session-Token: {}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        value.session_token, payload.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| format!("could not contact installer HTTP service: {err}"))?;
    read_http_response(stream)
}

#[tauri::command]
fn installer_request(
    method: String,
    path: String,
    body: Option<String>,
    state: tauri::State<InstallerTokens>,
) -> Result<InstallerResponse, String> {
    let value = connection(&state)?;
    match value.transport.as_str() {
        "unix" => send_socket_request(&value, &method, &path, body.as_deref()),
        "http" => send_http_request(&value, &method, &path, body.as_deref()),
        _ => Err("unknown installer transport".to_string()),
    }
}

fn start_socket_stream(
    app: tauri::AppHandle,
    value: InstallerConnection,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut stream = UnixStream::connect(socket_path(&value)?)
        .map_err(|err| format!("could not connect to installer stream: {err}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(1))).ok();
    let request = format!(
        "GET /api/stream HTTP/1.1\r\nHost: kyth-installer.local\r\nX-Kyth-Session-Token: {}\r\nAccept: text/event-stream\r\n\r\n",
        value.session_token
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| format!("could not contact installer stream: {err}"))?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                continue;
            }
            Err(err) => return Err(format!("could not read installer stream: {err}")),
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if let Some(payload) = line.strip_prefix("data: ") {
            let event: serde_json::Value = serde_json::from_str(payload.trim())
                .map_err(|err| format!("invalid installer event: {err}"))?;
            app.emit("installer-event", event)
                .map_err(|err| format!("could not deliver installer event: {err}"))?;
        }
    }
    Ok(())
}

#[tauri::command]
fn installer_stream(
    app: tauri::AppHandle,
    state: tauri::State<InstallerTokens>,
    stream_state: tauri::State<InstallerStream>,
) -> Result<(), String> {
    let value = connection(&state)?;
    let stop = Arc::new(AtomicBool::new(false));
    *stream_state
        .0
        .lock()
        .map_err(|_| "installer stream state unavailable".to_string())? = Some(stop.clone());
    std::thread::spawn(move || {
        if let Err(error) = start_socket_stream(app.clone(), value, stop) {
            let _ = app.emit("installer-stream-error", error);
        }
    });
    Ok(())
}

#[tauri::command]
fn installer_stream_stop(stream_state: tauri::State<InstallerStream>) -> Result<(), String> {
    if let Some(stop) = stream_state
        .0
        .lock()
        .map_err(|_| "installer stream state unavailable".to_string())?
        .take()
    {
        stop.store(true, Ordering::Relaxed);
    }
    Ok(())
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let tokens = InstallerConnection {
        base_url: BACKEND_URL.to_string(),
        bootstrap_token: arg_value(&argv, "--bootstrap-token").unwrap_or_default(),
        session_token: arg_value(&argv, "--session-token")
            .or_else(|| std::env::var("KYTH_INSTALLER_SESSION_TOKEN").ok())
            .unwrap_or_default(),
        socket_path: arg_value(&argv, "--socket-path"),
        transport: if argv.iter().any(|arg| arg == "--socket-path") {
            "unix".to_string()
        } else {
            "http".to_string()
        },
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .manage(InstallerTokens(Mutex::new(Some(tokens))))
        .manage(InstallerStream(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            installer_connection,
            installer_validate_plan,
            installer_recovery_guidance,
            installer_execution_plan,
            installer_request,
            installer_stream,
            installer_stream_stop
        ])
        .run(tauri::generate_context!())
        .expect("error while running the KythOS installer shell");
}

#[cfg(test)]
mod transport_tests {
    use super::*;

    #[test]
    fn routes_are_strictly_allowlisted() {
        assert!(allowlisted_path("GET", "/api/disks"));
        assert!(allowlisted_path("POST", "/api/start"));
        assert!(!allowlisted_path("POST", "/api/exec"));
        assert!(!allowlisted_path("GET", "http://127.0.0.1:7777/api/disks"));
        assert!(!allowlisted_path("GET", "/api/disks\nX-Bad: yes"));
    }

    #[test]
    fn http_transport_is_pinned_to_loopback_backend() {
        let value = InstallerConnection {
            base_url: "http://127.0.0.1:7778".to_string(),
            bootstrap_token: String::new(),
            session_token: String::new(),
            transport: "http".to_string(),
            socket_path: None,
        };
        assert!(send_http_request(&value, "GET", "/api/disks", None).is_err());
    }
}
