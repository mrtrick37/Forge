//! Port of `kyth_shared.system.fonts_ready` — msttcorefonts parity (N35).
//! Checks Noto Sans + Arial via fc-list; mirrors Python's 5s timeout each.

use std::time::Duration;

fn run_fc(pattern: &str) -> Option<String> {
    let argv = ["fc-list".to_string(), pattern.to_string()];
    let out = super::process::run_bounded(&argv, Duration::from_secs(5)).ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        None
    }
}

pub fn fonts_ready() -> (bool, String) {
    // Use timeout via Command with manual poll — like mok_verify, 5s each
    // Simpler: rely on Command::output blocking; fc-list is fast. Match Python's
    // behavior: has_noto = bool(stdout.strip()) if returncode==0 else False
    let has_noto = run_fc(":family=Noto Sans")
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let has_ms = run_fc(":family=Arial")
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if has_noto && has_ms {
        (true, "Noto + MS fonts ready".to_string())
    } else if has_noto {
        (
            false,
            "Noto ready, MS via ujust install-ms-fonts".to_string(),
        )
    } else {
        (false, "fonts check pending".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn returns_tuple() {
        let (ok, msg) = fonts_ready();
        // Just verify it doesn't panic and returns expected strings
        assert!(
            msg == "Noto + MS fonts ready" || msg.contains("Noto") || msg == "fonts check pending"
        );
        let _ = ok;
    }
}
