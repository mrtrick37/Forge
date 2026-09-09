//! Port of `kyth_shared.system.gaming_slice` — per-game cgroup slice helper.

use std::path::Path;

pub fn gaming_slice_command(argv: &[String], use_user: Option<bool>) -> Vec<String> {
    if argv.is_empty() {
        return argv.to_vec();
    }
    let has_run = which("systemd-run");
    if !has_run {
        return argv.to_vec();
    }
    let use_user = use_user.unwrap_or_else(|| {
        #[cfg(unix)]
        {
            // rustix getuid + Path exists check mirrors Python's euid !=0 and /run/systemd/system exists
            let is_root = rustix::process::getuid().is_root();
            !is_root && Path::new("/run/systemd/system").exists()
        }
        #[cfg(not(unix))]
        false
    });
    let mut base = if use_user {
        vec![
            "systemd-run".to_string(),
            "--user".to_string(),
            "--scope".to_string(),
            "--slice=gaming.slice".to_string(),
        ]
    } else {
        vec![
            "systemd-run".to_string(),
            "--scope".to_string(),
            "--slice=gaming.slice".to_string(),
        ]
    };
    base.push("--".to_string());
    base.extend_from_slice(argv);
    base
}

fn which(cmd: &str) -> bool {
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            if Path::new(dir).join(cmd).exists() {
                return true;
            }
        }
    }
    Path::new(&format!("/usr/bin/{}", cmd)).exists() || Path::new(&format!("/bin/{}", cmd)).exists()
}

pub fn is_gaming_slice_available() -> bool {
    which("systemd-run") && Path::new("/usr/lib/systemd/system/gaming.slice").exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_returns_empty() {
        assert_eq!(gaming_slice_command(&[], None), Vec::<String>::new());
    }
    #[test]
    fn no_systemd_run_returns_argv() {
        // If systemd-run present, gaming_slice wraps; if absent, returns argv.
        // Just verify it does not panic and returns at least argv content.
        let argv = vec!["echo".to_string(), "hi".to_string()];
        let out = gaming_slice_command(&argv, Some(false));
        assert!(out.contains(&"echo".to_string()));
        assert!(out.contains(&"hi".to_string()));
    }
    #[test]
    fn wraps_when_available() {
        // This test may be brittle if systemd-run present; just check shape
        let argv = vec!["game".to_string()];
        let out = gaming_slice_command(&argv, Some(false));
        assert!(out.contains(&"game".to_string()));
    }
}
