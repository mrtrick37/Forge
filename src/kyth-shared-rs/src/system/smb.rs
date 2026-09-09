//! Port of `kyth_shared.system.smb` — Aurora autodiscover parity (N33).
//! avahi-browse / smbclient discovery + gio mount, no auto-mount on boot.

use std::time::Duration;

pub fn smb_discover_command(host: Option<&str>) -> Vec<String> {
    if let Some(h) = host {
        vec![
            "smbclient".to_string(),
            "-L".to_string(),
            h.to_string(),
            "-N".to_string(),
        ]
    } else {
        vec![
            "avahi-browse".to_string(),
            "-r".to_string(),
            "_smb._tcp".to_string(),
        ]
    }
}

pub fn smb_mount_command(share: &str) -> Vec<String> {
    vec!["gio".to_string(), "mount".to_string(), share.to_string()]
}

fn run_with_timeout(cmd: &[String], timeout: Duration) -> Option<(i32, String, String)> {
    if cmd.is_empty() {
        return None;
    }
    let output = super::process::run_bounded(cmd, timeout).ok()?;
    Some((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    ))
}

pub fn smb_browse_dry_run(host: Option<&str>) -> (bool, String) {
    let cmd = smb_discover_command(host);
    match run_with_timeout(&cmd, Duration::from_secs(10)) {
        Some((0, stdout, _)) => (true, stdout.chars().take(500).collect()),
        Some((_, _, stderr)) if !stderr.is_empty() => (false, stderr.chars().take(500).collect()),
        Some((_, _, _)) => (false, format!("{} failed", cmd.join(" "))),
        None => {
            // distinguish not-installed
            let help = vec![cmd[0].clone(), "--help".into()];
            let exists = super::process::run_bounded(&help, Duration::from_secs(3)).is_ok();
            if !exists {
                (false, format!("{} not installed", cmd[0]))
            } else {
                (false, "timeout".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discover_no_host() {
        assert_eq!(
            smb_discover_command(None),
            vec!["avahi-browse", "-r", "_smb._tcp"]
        );
    }
    #[test]
    fn discover_host() {
        assert_eq!(
            smb_discover_command(Some("host")),
            vec!["smbclient", "-L", "host", "-N"]
        );
    }
    #[test]
    fn mount() {
        assert_eq!(
            smb_mount_command("smb://host/share"),
            vec!["gio", "mount", "smb://host/share"]
        );
    }
}
