//! Pure parsing helpers for the read-only system probe facade.

use std::collections::BTreeMap;

pub fn firewall_state(output: &str) -> &'static str {
    super::runtime_output::parse_systemd_state(output)
}

pub fn selinux_state(output: &str) -> String {
    output.trim().to_string()
}

pub fn secure_boot_state(output: &str) -> &'static str {
    super::runtime_output::parse_secure_boot_state(output)
}

/// Parse the exact `[Autologin]`/`User` pair read by the Python facade.
pub fn autologin_user(files: &[&str]) -> String {
    for file in files {
        let mut section = "";
        for line in file.lines() {
            let line = line.trim();
            if line.starts_with('[') && line.ends_with(']') {
                section = &line[1..line.len() - 1];
            } else if section == "Autologin" {
                if let Some((key, value)) = line.split_once('=') {
                    if key.trim() == "User" && !value.trim().is_empty() {
                        return value.trim().to_string();
                    }
                }
            }
        }
    }
    String::new()
}

pub fn config_bool(value: Option<&str>, default: bool) -> bool {
    !matches!(
        value.map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("false" | "0")
    ) && value.is_some()
        || value.is_none() && default
}

/// Interpret the two KDE screen-lock settings using the Python facade's
/// fail-safe default: an unreadable or empty value means enabled.
pub fn screen_lock_status(autolock: Option<&str>, lock_on_resume: Option<&str>) -> (bool, bool) {
    (
        config_bool(autolock, true),
        config_bool(lock_on_resume, true),
    )
}

/// Interpret the KDE wallet setting using the same enabled-by-default rule.
pub fn kwallet_enabled(value: Option<&str>) -> bool {
    config_bool(value, true)
}

pub fn snapshot(
    values: impl IntoIterator<Item = (&'static str, String)>,
) -> BTreeMap<&'static str, String> {
    values.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_probe_state_and_autologin() {
        assert_eq!(firewall_state("ACTIVE\n"), "active");
        assert_eq!(selinux_state("Enforcing\n"), "Enforcing");
        assert_eq!(secure_boot_state("SecureBoot disabled"), "disabled");
        assert_eq!(autologin_user(&["[Autologin]\nUser=kyth\n"]), "kyth");
        assert_eq!(
            autologin_user(&["[Autologin]\nUser=\n", "[Autologin]\nUser=second\n"]),
            "second"
        );
    }

    #[test]
    fn honors_boolean_defaults() {
        assert!(config_bool(None, true));
        assert!(!config_bool(Some("false"), true));
        assert!(config_bool(Some("true"), false));
        assert_eq!(screen_lock_status(None, Some("0")), (true, false));
        assert!(kwallet_enabled(Some("")));
        assert!(!kwallet_enabled(Some("false")));
    }
}
