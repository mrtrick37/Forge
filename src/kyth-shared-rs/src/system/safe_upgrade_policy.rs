//! Pure policy/config helpers for the privileged safe-upgrade workflow.
//!
//! The command boundary remains in the binary, while this module keeps the
//! rollout, manifest-fallback, digest, and fixed `/boot` argv policy reviewable
//! and testable without starting an upgrade or mounting anything.

use std::path::Path;

pub const DEFAULT_CONFIG_PATH: &str = "/etc/kyth/auto-update.toml";
pub const DEFAULT_ROLLOUT_RING: &str = "follow-image";

/// Decode the rollout setting from captured TOML without performing I/O.
pub fn rollout_ring_from_toml(raw: &str) -> String {
    let Ok(value) = raw.parse::<toml::Value>() else {
        return DEFAULT_ROLLOUT_RING.into();
    };
    let Some(section) = value.get("auto_update").and_then(toml::Value::as_table) else {
        return DEFAULT_ROLLOUT_RING.into();
    };
    match section.get("rollout_ring") {
        Some(toml::Value::String(value)) => value.clone(),
        Some(toml::Value::Boolean(value)) => if *value { "True" } else { "False" }.into(),
        Some(toml::Value::Integer(value)) => value.to_string(),
        Some(toml::Value::Float(value)) => value.to_string(),
        Some(toml::Value::Datetime(value)) => value.to_string(),
        Some(_) | None => DEFAULT_ROLLOUT_RING.into(),
    }
}

/// Read the configured rollout ring with the same fail-safe default as the
/// Python helper. Missing, unreadable, and malformed files follow-image.
pub fn load_rollout_ring(path: impl AsRef<Path>) -> String {
    std::fs::read_to_string(path)
        .map(|raw| rollout_ring_from_toml(&raw))
        .unwrap_or_else(|_| DEFAULT_ROLLOUT_RING.into())
}

/// The fixed remount attempts safe-upgrade makes, in order of preference.
/// Returning argv keeps execution and privilege decisions with the caller.
pub fn boot_remount_commands() -> [Vec<String>; 2] {
    [
        ["mount", "-o", "remount,bind,rw", "/boot"]
            .into_iter()
            .map(String::from)
            .collect(),
        ["mount", "-o", "remount,rw", "/boot"]
            .into_iter()
            .map(String::from)
            .collect(),
    ]
}

pub fn bind_sysroot_boot_command() -> Vec<String> {
    ["mount", "--bind", "/boot", "/sysroot/boot"]
        .into_iter()
        .map(String::from)
        .collect()
}

pub fn finalize_staged_command() -> Vec<String> {
    ["ostree", "admin", "finalize-staged"]
        .into_iter()
        .map(String::from)
        .collect()
}

/// Validate the digest bootc reports after staging an update.
///
/// A successful remote manifest probe gives us an immutable digest to compare
/// against. If that independent probe was unavailable, bootc's staged digest
/// is still authoritative for what it actually fetched, but a digest already
/// quarantined locally must remain blocked.
pub fn validate_staged_digest(
    remote_digest: Option<&str>,
    staged_digest: Option<&str>,
    quarantine_reason: Option<&str>,
) -> Result<String, String> {
    let staged = staged_digest
        .filter(|digest| !digest.is_empty())
        .ok_or_else(|| "bootc did not stage an image".to_string())?;
    if let Some(remote) = remote_digest {
        if staged != remote {
            return Err("bootc did not stage the requested image".to_string());
        }
    } else if let Some(reason) = quarantine_reason {
        return Err(format!("Update blocked: {reason}"));
    }
    Ok(staged.to_string())
}

/// Convert a registry check into the digest gate used by safe-upgrade.
///
/// A local status failure remains fail-closed. A remote probe failure is
/// explicitly represented as `Ok(None)` so bootc can be the authoritative
/// fetcher for the update.
pub fn remote_digest_for_safe_upgrade(
    state: &str,
    detail: &str,
    remote_probe_failed: bool,
    manifest_raw: &[u8],
) -> Result<Option<String>, String> {
    if state == "error" && !remote_probe_failed {
        return Err(detail.to_string());
    }
    if remote_probe_failed {
        return Ok(None);
    }
    crate::system::registry::remote_digest_and_timestamp(manifest_raw)
        .0
        .map(Some)
        .ok_or_else(|| "Could not resolve the remote image digest; update not staged".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn parses_rollout_ring_and_safe_defaults() {
        assert_eq!(
            rollout_ring_from_toml("[auto_update]\nrollout_ring = \"testing\"\n"),
            "testing"
        );
        assert_eq!(
            rollout_ring_from_toml("[auto_update]\nrollout_ring = true\n"),
            "True"
        );
        assert_eq!(rollout_ring_from_toml("not toml"), DEFAULT_ROLLOUT_RING);
        assert_eq!(
            rollout_ring_from_toml("[other]\nvalue = 1\n"),
            DEFAULT_ROLLOUT_RING
        );
    }

    #[test]
    fn loads_rollout_ring_from_an_explicit_path() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("auto-update.toml");
        fs::write(&path, "[auto_update]\nrollout_ring = \"canary\"\n").unwrap();
        assert_eq!(load_rollout_ring(&path), "canary");
        assert_eq!(
            load_rollout_ring(directory.path().join("missing.toml")),
            DEFAULT_ROLLOUT_RING
        );
    }

    #[test]
    fn projects_only_the_fixed_upgrade_boundary_commands() {
        assert_eq!(
            boot_remount_commands()[0],
            vec!["mount", "-o", "remount,bind,rw", "/boot"]
        );
        assert_eq!(
            boot_remount_commands()[1],
            vec!["mount", "-o", "remount,rw", "/boot"]
        );
        assert_eq!(
            bind_sysroot_boot_command(),
            vec!["mount", "--bind", "/boot", "/sysroot/boot"]
        );
        assert_eq!(
            finalize_staged_command(),
            vec!["ostree", "admin", "finalize-staged"]
        );
    }

    #[test]
    fn staged_digest_must_match_a_successful_remote_probe() {
        assert_eq!(
            validate_staged_digest(Some("sha256:remote"), Some("sha256:remote"), None),
            Ok("sha256:remote".into())
        );
        assert_eq!(
            validate_staged_digest(Some("sha256:remote"), Some("sha256:other"), None),
            Err("bootc did not stage the requested image".into())
        );
    }

    #[test]
    fn bootc_digest_is_accepted_when_remote_probe_is_unavailable() {
        assert_eq!(
            validate_staged_digest(None, Some("sha256:bootc"), None),
            Ok("sha256:bootc".into())
        );
    }

    #[test]
    fn degraded_path_still_blocks_a_locally_quarantined_digest() {
        assert_eq!(
            validate_staged_digest(None, Some("sha256:bad"), Some("sha256:bad is quarantined")),
            Err("Update blocked: sha256:bad is quarantined".into())
        );
    }

    #[test]
    fn staging_without_a_digest_is_never_successful() {
        assert_eq!(
            validate_staged_digest(None, None, None),
            Err("bootc did not stage an image".into())
        );
    }

    #[test]
    fn remote_probe_failure_is_degraded_but_local_status_failure_is_not() {
        assert_eq!(
            remote_digest_for_safe_upgrade(
                "error",
                "Timed out checking ghcr.io/kyth-os/kyth:testing.",
                true,
                &[]
            ),
            Ok(None)
        );
        assert_eq!(
            remote_digest_for_safe_upgrade(
                "error",
                "Could not read the current booted image digest.",
                false,
                &[]
            ),
            Err("Could not read the current booted image digest.".into())
        );
    }
}
