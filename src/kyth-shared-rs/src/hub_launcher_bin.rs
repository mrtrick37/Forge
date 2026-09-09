//! Native launcher for the React/Tauri System Hub.
//!
//! This keeps the historical `kyth-welcome-launch` command name so existing
//! desktop entries, notifications, and recipes remain compatible, but the
//! launch boundary itself is Rust-owned. It never imports or invokes Python.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const TARGET: &str = "/usr/bin/kyth-hub-shell";

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn state_root() -> PathBuf {
    env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".local/state"))
                .unwrap_or_else(|| PathBuf::from("/tmp"))
        })
        .join("kyth")
}

fn cache_root() -> PathBuf {
    env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("kyth")
}

fn append_log(message: &str) -> Option<PathBuf> {
    let directory = cache_root();
    fs::create_dir_all(&directory).ok()?;
    let path = directory.join("kyth-welcome.log");
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;
    let _ = writeln!(log, "{message}");
    Some(path)
}

fn write_failure_report(status: &str, log_path: Option<&PathBuf>) {
    let directory = state_root();
    if fs::create_dir_all(&directory).is_err() {
        return;
    }
    let path = directory.join(format!("system-hub-crash-{}.md", now()));
    let log_tail = log_path
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|text| {
            text.lines()
                .rev()
                .take(120)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let body = format!(
        "## What happened\n\nKyth Hub exited unexpectedly.\n\n## Crash context\n\n- Exit status: {status}\n- Session: {}\n- Display: DISPLAY={} WAYLAND_DISPLAY={}\n- WebKit DMA-BUF workaround: {}\n\n## Recent Hub launch log\n\n```text\n{log_tail}\n```\n\n## Notes\n\nAdd what you were doing immediately before the crash, then submit the issue.\n",
        env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into()),
        env::var("DISPLAY").unwrap_or_default(),
        env::var("WAYLAND_DISPLAY").unwrap_or_default(),
        env::var("WEBKIT_DISABLE_DMABUF_RENDERER").unwrap_or_else(|_| "default".into()),
    );
    let _ = fs::write(&path, body);

    let reporter = PathBuf::from("/usr/bin/kyth-report-issue");
    if reporter.is_file() {
        let _ = Command::new(reporter)
            .arg("--title")
            .arg("Kyth Hub crashed on launch")
            .arg("--body-file")
            .arg(&path)
            .arg("--label")
            .arg("bug")
            .arg("--no-open")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn main() -> ExitCode {
    let timestamp = now();
    let log_path = append_log(&format!(
        "==== {timestamp} kyth-welcome-launch (native Rust) ====\nuser={} DISPLAY={} WAYLAND_DISPLAY={} XDG_SESSION_TYPE={}",
        env::var("USER").unwrap_or_else(|_| "unknown".into()),
        env::var("DISPLAY").unwrap_or_default(),
        env::var("WAYLAND_DISPLAY").unwrap_or_default(),
        env::var("XDG_SESSION_TYPE").unwrap_or_default(),
    ));

    let status = match Command::new(TARGET).args(env::args().skip(1)).status() {
        Ok(status) => status,
        Err(error) => {
            let message = format!("Kyth Hub shell could not start: {error}");
            let _ = append_log(&message);
            eprintln!("{message}");
            write_failure_report("127", log_path.as_ref());
            return ExitCode::from(127);
        }
    };
    let code = status.code().unwrap_or(1);
    let _ = append_log(&format!(
        "kyth-welcome-launch completed with exit status {code}"
    ));
    if code != 0 {
        write_failure_report(&code.to_string(), log_path.as_ref());
    }
    ExitCode::from(code.clamp(0, 255) as u8)
}
