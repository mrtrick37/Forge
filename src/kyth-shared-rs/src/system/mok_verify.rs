//! Port of `kyth_shared.system.mok_verify` — Nobara parity (N40).
//!
//! Faithful: `mokutil --sb-state` → enabled/disabled/unknown + `mokutil
//! --list-enrolled` → KythOS Secure Boot enrolled check. 5s timeout each,
//! `FileNotFound` → unknown/mokutil not installed.

use std::time::Duration;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MokStatus {
    pub sb_state: String,
    pub enrolled: String,
}

fn run_with_timeout(cmd: &str, args: &[&str], timeout: Duration) -> Option<(i32, String, String)> {
    let mut argv = vec![cmd.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    let output = super::process::run_bounded(&argv, timeout).ok()?;
    Some((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    ))
}

pub fn mok_status() -> MokStatus {
    let sb_result = run_with_timeout("mokutil", &["--sb-state"], Duration::from_secs(5));
    let (sb_state, enrolled) = match sb_result {
        None => {
            // Check if mokutil missing vs timeout — try to detect FileNotFound
            // `run_with_timeout` returns None on spawn failure or timeout. Distinguish
            // by probing existence via `which`-like check: attempt spawn and see error kind.
            // A bounded help probe distinguishes a missing binary from a timed-out probe.
            // as unknown / mokutil not installed mirrors Python's FileNotFound branch.
            // A non-zero help exit still counts as an installed binary.
            let exists = super::process::run_bounded(
                &["mokutil", "--help"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                Duration::from_secs(2),
            )
            .is_ok();
            if !exists {
                return MokStatus {
                    sb_state: "unknown".to_string(),
                    enrolled: "mokutil not installed".to_string(),
                };
            }
            ("unknown".to_string(), "unknown".to_string())
        }
        Some((code, stdout, _)) => {
            let lower = stdout.to_lowercase();
            let sb = if code == 0 && lower.contains("secureboot enabled") {
                "enabled"
            } else if lower.contains("disabled") {
                "disabled"
            } else {
                "unknown"
            };
            // second call
            let r2 = run_with_timeout("mokutil", &["--list-enrolled"], Duration::from_secs(5));
            let enrolled = match r2 {
                Some((c2, out2, _)) if c2 == 0 && out2.contains("KythOS Secure Boot") => "enrolled",
                Some((c2, _, _)) if c2 == 0 => "not enrolled",
                _ => "not enrolled",
            };
            (sb.to_string(), enrolled.to_string())
        }
    };
    MokStatus { sb_state, enrolled }
}

// Test helper: parse logic without spawning
pub fn parse_sb(stdout: &str, code: i32) -> &'static str {
    let lower = stdout.to_lowercase();
    if code == 0 && lower.contains("secureboot enabled") {
        "enabled"
    } else if lower.contains("disabled") {
        "disabled"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_enabled() {
        assert_eq!(parse_sb("SecureBoot enabled", 0), "enabled");
        assert_eq!(parse_sb("SecureBoot enabled\n", 0), "enabled");
    }

    #[test]
    fn parse_disabled() {
        assert_eq!(parse_sb("SecureBoot disabled", 0), "disabled");
        assert_eq!(parse_sb("disabled", 1), "disabled");
    }

    #[test]
    fn parse_unknown() {
        assert_eq!(parse_sb("", 0), "unknown");
        assert_eq!(parse_sb("some error", 1), "unknown");
    }
}
