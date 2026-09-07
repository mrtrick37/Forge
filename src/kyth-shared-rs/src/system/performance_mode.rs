//! Performance mode switch: power profiles, EPP, and KDE composition.
//!
//! Mirrors the `kyth-performance-mode` launcher plus the four
//! `kyth_shared.performance` helpers it uses: state save/restore around a
//! runtime-dir state file, a five-mode table, and status reporting. Only
//! the `*_bin.rs` entry point touches processes and the state file.

use std::path::{Path, PathBuf};
use std::time::Duration;

pub const STATE_FILE_NAME: &str = "kyth-performance-mode.state";

/// (power profile, EPP, animation factor, blur) per mode, exactly as the
/// Python launcher ordered them.
pub fn mode_settings(mode: &str) -> Option<(&'static str, &'static str, &'static str, &'static str)> {
    match mode {
        "max" => Some(("performance", "performance", "0", "false")),
        "gaming" => Some(("performance", "performance", "0.5", "false")),
        "performance" => Some(("performance", "performance", "0.75", "false")),
        "balanced" => Some(("balanced", "balance_performance", "1", "true")),
        "powersave" => Some(("power-saver", "power", "1.25", "true")),
        _ => None,
    }
}

pub fn state_path() -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })));
    dir.join(STATE_FILE_NAME)
}

/// Mirrors the `^[A-Za-z0-9_. -]*$` state-value sanitizer.
pub fn sanitize_state_value(value: &str) -> Option<String> {
    value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ' ' | '-'))
        .then(|| value.to_string())
}

pub fn read_state_key(text: &str, key: &str) -> String {
    let prefix = format!("{key}=");
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(&prefix) {
            let value = rest.trim();
            if let Some(clean) = sanitize_state_value(value) {
                return clean;
            }
        }
    }
    String::new()
}

pub fn render_state(power_profile: &str, anim_factor: &str, blur_enabled: &str, epp: &str) -> String {
    format!(
        "POWER_PROFILE={power_profile}\nANIMATION_DURATION_FACTOR={anim_factor}\nBLUR_ENABLED={blur_enabled}\nEPP={epp}\n"
    )
}

pub fn power_profile_argv(profile: &str) -> Vec<String> {
    vec!["powerprofilesctl".to_string(), "set".to_string(), profile.to_string()]
}

pub fn epp_helper_argv(value: &str) -> Vec<String> {
    vec!["sudo".to_string(), "-n".to_string(), "/usr/bin/kyth-set-epp".to_string(), value.to_string()]
}

pub fn epp_sysfs_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let Ok(entries) = std::fs::read_dir("/sys/devices/system/cpu") else { return paths };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("cpu") && name[3..].chars().all(|c| c.is_ascii_digit()) {
            let candidate = entry.path().join("cpufreq/energy_performance_preference");
            if candidate.is_file() {
                paths.push(candidate);
            }
        }
    }
    paths.sort();
    paths
}

/// Mirrors `set_epp`: the sudo helper first (rc 0 wins), then best-effort
/// sysfs writes; true if any path succeeded.
pub fn set_epp(value: &str) -> bool {
    let helper_ok = crate::system::process::run_bounded(&epp_helper_argv(value), Duration::from_secs(30))
        .map(|output| output.status.success())
        .unwrap_or(false);
    if helper_ok {
        return true;
    }
    let mut success = false;
    for path in epp_sysfs_paths() {
        if std::fs::write(&path, value).is_ok() {
            success = true;
        }
    }
    success
}

/// Mirrors `set_power_profile` (rc decides; missing binary is false).
pub fn set_power_profile(profile: &str) -> bool {
    crate::system::process::run_bounded(&power_profile_argv(profile), Duration::from_secs(30))
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Mirrors `get_power_profile` (`powerprofilesctl get` stdout verbatim, or
/// `n/a` when the binary is missing). Note: stdout is returned regardless
/// of exit status upstream, even when empty.
pub fn get_power_profile() -> String {
    crate::system::process::run_bounded(
        &["powerprofilesctl".to_string(), "get".to_string()],
        Duration::from_secs(10),
    )
    .ok()
    .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
    .unwrap_or_else(|| "n/a".to_string())
}

/// Mirrors `get_current_epp` (cpu0 preference file stripped, or `n/a`
/// when unreadable — an empty file yields empty upstream, not `n/a`).
pub fn get_current_epp() -> String {
    std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference")
        .ok()
        .map(|text| text.trim().to_string())
        .unwrap_or_else(|| "n/a".to_string())
}

pub fn state_file_in(dir: &Path) -> PathBuf {
    dir.join(STATE_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_table_matches_python_launcher() {
        assert_eq!(mode_settings("max"), Some(("performance", "performance", "0", "false")));
        assert_eq!(mode_settings("powersave"), Some(("power-saver", "power", "1.25", "true")));
        assert_eq!(mode_settings("nope"), None);
    }

    #[test]
    fn state_round_trips_with_sanitizer() {
        let text = render_state("balanced", "1", "true", "balance_performance");
        assert_eq!(read_state_key(&text, "POWER_PROFILE"), "balanced");
        assert_eq!(read_state_key(&text, "EPP"), "balance_performance");
        assert_eq!(read_state_key(&text, "MISSING"), "");
        assert_eq!(read_state_key("EPP=perf;rm\n", "EPP"), "");
    }

    #[test]
    fn projects_power_argv() {
        assert_eq!(power_profile_argv("performance"), vec!["powerprofilesctl", "set", "performance"]);
        assert_eq!(
            epp_helper_argv("performance"),
            vec!["sudo", "-n", "/usr/bin/kyth-set-epp", "performance"]
        );
    }
}
