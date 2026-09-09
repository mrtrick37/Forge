//! Port of `kyth_shared.system.updater` — fetch JSON metadata for latest release.

use std::time::Duration;

pub fn updater_available() -> bool {
    // Check if updater binary exists
    std::path::Path::new("/usr/bin/kyth-updater").exists()
        || std::path::Path::new("/usr/bin/kyth-full-update").exists()
}

fn run_with_timeout(cmd: &[String], timeout: Duration) -> Option<(i32, String)> {
    if cmd.is_empty() {
        return None;
    }
    let output = super::process::run_bounded(cmd, timeout).ok()?;
    Some((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
    ))
}

pub fn fetch_updater_metadata() -> Option<String> {
    // Simplified: run updater --check or just return none
    run_with_timeout(
        &["kyth-updater".to_string(), "--check".to_string()],
        Duration::from_secs(10),
    )
    .and_then(|(code, out)| if code == 0 { Some(out) } else { None })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn available_bool() {
        let _ = updater_available();
    }
}
