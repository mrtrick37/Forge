//! Port of `kyth_shared.system.cloud_oauth` — Aurora rclone parity (N36).

use std::time::Duration;

pub fn rclone_oauth_command(remote: &str) -> Vec<String> {
    vec![
        "rclone".to_string(),
        "config".to_string(),
        "create".to_string(),
        remote.to_string(),
        "onedrive".to_string(),
        "--all".to_string(),
    ]
}

fn run_with_timeout(cmd: &str, args: &[&str], timeout: Duration) -> Option<(i32, String)> {
    let mut argv = vec![cmd.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    let output = super::process::run_bounded(&argv, timeout).ok()?;
    Some((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
    ))
}

pub fn cloud_oauth_status() -> (bool, String) {
    match run_with_timeout("rclone", &["listremotes"], Duration::from_secs(5)) {
        Some((0, stdout)) => {
            let rems: Vec<String> = stdout
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            (
                true,
                format!(
                    "rclone remotes: {}",
                    if rems.is_empty() {
                        "none".to_string()
                    } else {
                        rems.join(", ")
                    }
                ),
            )
        }
        Some((_, _)) => (
            false,
            "rclone not configured — use Hub Cloud Storage OAuth".to_string(),
        ),
        None => {
            // FileNotFound vs timeout: probe existence
            let exists = super::process::run_bounded(
                &["rclone", "version"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                Duration::from_secs(5),
            )
            .is_ok();
            if !exists {
                (false, "rclone not installed".to_string())
            } else {
                (false, "rclone timeout".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oauth_command() {
        assert_eq!(
            rclone_oauth_command("onedrive"),
            vec!["rclone", "config", "create", "onedrive", "onedrive", "--all"]
        );
    }
    #[test]
    fn status_returns_tuple() {
        let (ok, msg) = cloud_oauth_status();
        assert!(!msg.is_empty());
        let _ = ok;
    }
}
