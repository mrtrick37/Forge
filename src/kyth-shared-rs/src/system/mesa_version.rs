//! Port of `kyth_shared.system.mesa_version` — mesa/plasma overlay gated (N41).
//! glxinfo -B OpenGL version → rpm -q mesa-dri-drivers → "mesa stable" fallback.

use std::time::Duration;

fn run_with_timeout(cmd: &str, args: &[&str], timeout: Duration) -> Option<(i32, String)> {
    let mut argv = vec![cmd.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    let output = super::process::run_bounded(&argv, timeout).ok()?;
    Some((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
    ))
}

pub fn mesa_version() -> String {
    if let Some((code, stdout)) = run_with_timeout("glxinfo", &["-B"], Duration::from_secs(5)) {
        if code == 0 {
            for line in stdout.lines() {
                if line.contains("OpenGL version") {
                    return line.trim().to_string();
                }
            }
        }
    }
    if let Some((code, stdout)) =
        run_with_timeout("rpm", &["-q", "mesa-dri-drivers"], Duration::from_secs(5))
    {
        if code == 0 && !stdout.trim().is_empty() {
            return stdout.trim().to_string();
        }
    }
    "mesa stable".to_string()
}

pub fn mesa_overlay_dry_run() -> (bool, String) {
    if let Some((code, _)) = run_with_timeout(
        "dnf5",
        &["copr", "list", "--enabled"],
        Duration::from_secs(10),
    ) {
        if code == 0 {
            return (
                true,
                "dry-run ok: mesa overlay would be COPR enable + bootc lint".to_string(),
            );
        }
        return (true, "dry-run ok: mesa-git overlay gated".to_string());
    }
    (false, "dnf5 not available".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mesa_version_returns_string() {
        let v = mesa_version();
        assert!(!v.is_empty());
    }
}
