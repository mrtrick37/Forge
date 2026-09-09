//! Port of `kyth_shared.system.process` helpers (pure stdlib, no Qt).
//! Mostly re-exports from `probe` in Python; here we port the standalone
//! helpers: is_live_session, strip_ansi, with_idle_inhibit, disk write bytes,
//! format_elapsed/eta/progress.

use std::fs;
use std::io::{self, Write};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Run an already-validated argv with captured output and a hard wall-clock
/// limit. It never invokes a shell and kills a child that outlives its bound.
pub fn run_bounded(argv: &[String], timeout: Duration) -> io::Result<Output> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "command must not be empty"))?;
    let mut command = Command::new(program);
    command.args(args);
    run_bounded_command(command, timeout)
}

/// Run a fixed argv while supplying bounded sensitive input through stdin.
/// The input is never part of the process arguments or captured status text.
pub fn run_bounded_with_input(
    argv: &[String],
    input: &[u8],
    timeout: Duration,
) -> io::Result<Output> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "command must not be empty"))?;
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::piped());
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input)?;
    }
    let started = Instant::now();
    loop {
        match child.try_wait()? {
            Some(_) => return child.wait_with_output(),
            None if started.elapsed() <= timeout => std::thread::sleep(Duration::from_millis(25)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "command exceeded its time limit",
                ));
            }
        }
    }
}

pub fn run_bounded_command(mut command: Command, timeout: Duration) -> io::Result<Output> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let started = Instant::now();
    loop {
        match child.try_wait()? {
            Some(_) => return child.wait_with_output(),
            None if started.elapsed() <= timeout => std::thread::sleep(Duration::from_millis(25)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "command exceeded its time limit",
                ));
            }
        }
    }
}

pub fn is_live_session() -> bool {
    fs::read_to_string("/proc/cmdline")
        .map(|s| s.contains("kyth.live"))
        .unwrap_or(false)
}

pub fn strip_ansi(text: &str) -> String {
    // Mirrors re.sub(r"\x1b\[[0-9;]*[a-zA-Z]", "", text)
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next(); // '['
                while let Some(&next) = chars.peek() {
                    if next.is_ascii_alphabetic() {
                        chars.next();
                        break;
                    } else if next.is_ascii_digit() || next == ';' {
                        chars.next();
                    } else {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// Remove common secret-bearing `key=value`/`key: value` fields from text
/// before it is placed in a job status, diagnostic, or audit record.
///
/// This is intentionally a defense-in-depth filter. Secret-bearing commands
/// must still use stdin and avoid putting credentials in argv; this helper
/// protects the UI/log boundary if a child unexpectedly echoes a credential.
pub fn redact_sensitive_text(text: &str) -> String {
    text.lines()
        .map(redact_sensitive_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_sensitive_line(line: &str) -> String {
    let mut redacted = line.to_string();
    loop {
        let next = redact_sensitive_line_once(&redacted);
        if next == redacted {
            return redacted;
        }
        redacted = next;
    }
}

fn redact_sensitive_line_once(line: &str) -> String {
    const MARKERS: &[&str] = &[
        "password",
        "passwd",
        "passphrase",
        "secret",
        "token",
        "cookie",
        "authcookie",
        "samlresponse",
        "bitlocker_key",
        "authorization",
        "key",
    ];
    let lower = line.to_ascii_lowercase();
    for marker in MARKERS {
        let mut search_from = 0;
        while let Some(relative) = lower[search_from..].find(marker) {
            let start = search_from + relative;
            let end = start + marker.len();
            let boundary_before = start == 0
                || !lower.as_bytes()[start - 1].is_ascii_alphanumeric()
                    && lower.as_bytes()[start - 1] != b'_';
            let boundary_after = end == lower.len()
                || !lower.as_bytes()[end].is_ascii_alphanumeric() && lower.as_bytes()[end] != b'_';
            if !boundary_before || !boundary_after {
                search_from = end;
                continue;
            }
            let mut delimiter = end;
            if line
                .as_bytes()
                .get(delimiter)
                .is_some_and(|byte| matches!(byte, b'"' | b'\''))
            {
                delimiter += 1;
            }
            while delimiter < line.len() && line.as_bytes()[delimiter].is_ascii_whitespace() {
                delimiter += 1;
            }
            if delimiter >= line.len() || !matches!(line.as_bytes()[delimiter], b'=' | b':') {
                search_from = end;
                continue;
            }
            let mut value_start = delimiter + 1;
            while value_start < line.len() && line.as_bytes()[value_start].is_ascii_whitespace() {
                value_start += 1;
            }
            let quote = line
                .as_bytes()
                .get(value_start)
                .copied()
                .filter(|byte| matches!(byte, b'"' | b'\''));
            if quote.is_some() {
                value_start += 1;
            }
            let mut value_end = value_start;
            if let Some(quote) = quote {
                while value_end < line.len() && line.as_bytes()[value_end] != quote {
                    value_end += 1;
                }
            } else {
                while value_end < line.len()
                    && !line.as_bytes()[value_end].is_ascii_whitespace()
                    && !matches!(
                        line.as_bytes()[value_end],
                        b',' | b';' | b'&' | b'"' | b'\'' | b']' | b'}'
                    )
                {
                    value_end += 1;
                }
            }
            if value_start == value_end {
                search_from = end;
                continue;
            }
            if &line[value_start..value_end] == "<redacted>" {
                search_from = value_end;
                continue;
            }
            let mut redacted = String::with_capacity(line.len());
            redacted.push_str(&line[..value_start]);
            redacted.push_str("<redacted>");
            redacted.push_str(&line[value_end..]);
            return redacted;
        }
    }
    line.to_string()
}

pub fn with_idle_inhibit(cmd: &[String], reason: &str) -> Vec<String> {
    let has = which("systemd-inhibit");
    if !has {
        return cmd.to_vec();
    }
    let mut v = vec![
        "systemd-inhibit".to_string(),
        "--what=idle:sleep".to_string(),
        format!("--why={}", reason),
        "--mode=block".to_string(),
    ];
    v.extend_from_slice(cmd);
    v
}

fn which(cmd: &str) -> bool {
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            if std::path::Path::new(dir).join(cmd).exists() {
                return true;
            }
        }
    }
    false
}

pub fn get_disk_write_bytes() -> u64 {
    if let Ok(text) = fs::read_to_string("/proc/diskstats") {
        let mut total: u64 = 0;
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 10 {
                if let Ok(v) = parts[9].parse::<u64>() {
                    total += v;
                }
            }
        }
        return total * 512;
    }
    0
}

pub fn format_elapsed(seconds: i64) -> String {
    let s = seconds.max(0);
    let mins = s / 60;
    let secs = s % 60;
    if mins > 0 {
        format!("{}m {:02}s", mins, secs)
    } else {
        format!("{}s", secs)
    }
}

pub fn format_eta(seconds: i64) -> String {
    if seconds > 60 {
        format!("~{} remaining", format_elapsed(seconds))
    } else if seconds > 0 {
        format!("~{}s remaining", seconds)
    } else {
        String::new()
    }
}

pub fn format_dl_progress_line(
    downloaded: u64,
    total: u64,
    speed_bps: u64,
    eta_sec: i64,
) -> String {
    let dl_d = human_bytes(downloaded);
    let dl_t = human_bytes(total);
    let sp = human_bytes(speed_bps);
    let mut parts = vec![format!("{} / {}", dl_d, dl_t), format!("{}/s", sp)];
    let eta = format_eta(eta_sec);
    if !eta.is_empty() {
        parts.push(eta);
    }
    parts.join("  ·  ")
}

fn human_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut idx = 0;
    while v >= 1024.0 && idx < UNITS.len() - 1 {
        v /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{} {}", n, UNITS[idx])
    } else {
        format!("{:.1} {}", v, UNITS[idx])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn strip() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
    }

    #[test]
    fn sensitive_output_is_redacted_without_destroying_context() {
        let detail = "operation failed password=super-secret; retryable=true\nstatus: token=token-value\n{\"password\":\"json-secret\"}";
        let redacted = redact_sensitive_text(detail);
        assert!(!redacted.contains("super-secret"));
        assert!(!redacted.contains("token-value"));
        assert!(!redacted.contains("json-secret"));
        assert!(redacted.contains("operation failed password=<redacted>; retryable=true"));
        assert!(redacted.contains("status: token=<redacted>"));
    }

    #[test]
    fn sensitive_output_redacts_multiple_fields_on_one_line() {
        let redacted = redact_sensitive_text("password=first token=second secret=third");
        assert_eq!(
            redacted,
            "password=<redacted> token=<redacted> secret=<redacted>"
        );
    }
    #[test]
    fn elapsed() {
        assert_eq!(format_elapsed(70), "1m 10s");
        assert_eq!(format_elapsed(5), "5s");
    }
    #[test]
    fn eta() {
        assert_eq!(format_eta(90), "~1m 30s remaining");
    }

    #[test]
    fn bounded_runner_captures_a_static_argv_without_a_shell() {
        let output = run_bounded(
            &["sh".into(), "-c".into(), "printf ok".into()],
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"ok");
    }

    #[test]
    fn bounded_runner_terminates_a_stalled_child() {
        let error = run_bounded(
            &["sh".into(), "-c".into(), "sleep 1".into()],
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    }

    #[test]
    fn bounded_runner_supplies_input_without_putting_it_in_argv() {
        let output = run_bounded_with_input(
            &["sh".into(), "-c".into(), "cat".into()],
            b"secret",
            Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(output.stdout, b"secret");
    }
}
