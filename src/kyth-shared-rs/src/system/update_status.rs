//! Port of `kyth_shared.system.update_status` — watcher snapshot and
//! TTL-bounded check_state.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_UPDATE_STATUS_PATH: &str = "/var/lib/kyth/update-watcher-status.json";

/// Cross-process update state shared by the native watcher and the Hub.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default)]
pub struct UpdateSnapshot {
    pub result: String,
    pub reason: Option<String>,
    pub output: String,
    pub ts: i64,
    pub flatpak_updates: i64,
    pub image_ref: String,
    pub booted_digest: String,
    pub staged_digest: String,
    pub remote_digest: String,
    pub retryable: bool,
}

impl UpdateSnapshot {
    /// Match Python's `UpdateSnapshot.system_state` projection used by the
    /// welcome screen and notification policy.
    pub fn system_state(&self) -> &'static str {
        if self.result == "quarantined" && self.staged_digest.is_empty() {
            return "uptodate";
        }
        if matches!(self.result.as_str(), "skipped" | "error") && self.staged_digest.is_empty() {
            return "unknown";
        }
        if !self.staged_digest.is_empty()
            || self.result == "upgraded"
            || self
                .reason
                .as_deref()
                .is_some_and(|reason| reason.to_lowercase().contains("already staged"))
        {
            return "staged";
        }
        if !self.booted_digest.is_empty() && !self.remote_digest.is_empty() {
            return if self.booted_digest == self.remote_digest {
                "uptodate"
            } else {
                "available"
            };
        }
        if self.result == "no_change" && self.output.to_lowercase().contains("already up to date") {
            return "uptodate";
        }
        "unknown"
    }
}

/// Read and age-check a watcher snapshot without spawning a command.
/// `now` is injectable so tests never depend on mutable global clock state.
pub fn read_update_snapshot_in(
    path: impl AsRef<Path>,
    max_age: i64,
    now: i64,
) -> Option<UpdateSnapshot> {
    let text = std::fs::read_to_string(path).ok()?;
    let snapshot = serde_json::from_str::<UpdateSnapshot>(&text).ok()?;
    if snapshot.ts <= 0 || now.saturating_sub(snapshot.ts) > max_age {
        return None;
    }
    Some(snapshot)
}

pub fn read_update_snapshot(max_age: i64) -> Option<UpdateSnapshot> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    read_update_snapshot_in(DEFAULT_UPDATE_STATUS_PATH, max_age, now)
}

/// Persist watcher state without exposing partially-written JSON to the Hub.
pub fn write_update_snapshot(snapshot: &UpdateSnapshot) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_json(DEFAULT_UPDATE_STATUS_PATH, snapshot, Some(0o600))
}

/// Testable writer variant for service-level parity tests.
pub fn write_update_snapshot_to(
    path: impl AsRef<Path>,
    snapshot: &UpdateSnapshot,
) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_json(path, snapshot, Some(0o600))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateStatus {
    pub booted: Option<String>,
    pub staged: bool,
    pub rollback: bool,
    pub remote_digest: Option<String>,
    pub blocked_reason: Option<String>,
    pub retry_cmd: Option<String>,
    pub check_state: String,
    pub detail: String,
}

fn project_cached_status(
    data: Option<&serde_json::Value>,
    watcher: Option<&UpdateSnapshot>,
) -> UpdateStatus {
    let staged_from_bootc =
        data.is_some_and(|value| crate::system::bootc::deployment_present(value, "staged"));
    let watcher_staged = watcher.is_some_and(|snapshot| !snapshot.staged_digest.is_empty());
    let staged = staged_from_bootc || watcher_staged;
    let rollback =
        data.is_some_and(|value| crate::system::bootc::deployment_present(value, "rollback"));
    let booted = data.and_then(crate::system::registry::booted_image_digest);
    let Some(_) = data else {
        return UpdateStatus {
            booted: None,
            staged,
            rollback,
            remote_digest: None,
            blocked_reason: Some("Could not read bootc status.".to_string()),
            retry_cmd: Some("bootc upgrade --check".to_string()),
            check_state: "error".to_string(),
            detail: "Could not read bootc status.".to_string(),
        };
    };

    // Mount/refresh must remain a local read. The explicit "Check for
    // updates" action owns the live GHCR request; a page visit must not
    // unexpectedly wait for a registry timeout.
    let remote_digest = watcher.and_then(|snapshot| {
        (!snapshot.remote_digest.is_empty()).then(|| snapshot.remote_digest.clone())
    });
    let mut check_state = watcher
        .map(|snapshot| snapshot.system_state().to_string())
        .unwrap_or_else(|| "idle".to_string());
    let mut detail = watcher
        .and_then(|snapshot| {
            snapshot
                .reason
                .clone()
                .or_else(|| (!snapshot.output.is_empty()).then(|| snapshot.output.clone()))
        })
        .unwrap_or_else(|| "No recent update check.".to_string());
    if staged {
        check_state = "available".to_string();
        if detail.is_empty() {
            detail = watcher
                .and_then(|snapshot| snapshot.reason.clone())
                .unwrap_or_else(|| "staged image pending".to_string());
        }
    }
    UpdateStatus {
        booted,
        staged,
        rollback,
        remote_digest,
        blocked_reason: None,
        retry_cmd: None,
        check_state,
        detail,
    }
}

pub fn check_update_status() -> UpdateStatus {
    // The probe cache is an optimization, not the source of truth. The Hub
    // can be opened before kyth-probe has produced its first snapshot, so use
    // the same bounded bootc query fallback as availability and health.
    let data = crate::system::probe::read_section("bootc-status-data")
        .or_else(crate::system::bootc_query::fetch_status_data);
    let watcher = read_update_snapshot(600);
    project_cached_status(data.as_ref(), watcher.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn reads_fresh_watcher_snapshot() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("update-status.json");
        fs::write(
            &path,
            r#"{"result":"staged","ts":100,"staged_digest":"sha256:test"}"#,
        )
        .unwrap();
        let snapshot = read_update_snapshot_in(path, 600, 150).unwrap();
        assert_eq!(snapshot.result, "staged");
        assert_eq!(snapshot.staged_digest, "sha256:test");
        assert_eq!(snapshot.system_state(), "staged");
    }

    #[test]
    fn ignores_missing_or_stale_watcher_snapshot() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("update-status.json");
        fs::write(&path, r#"{"result":"staged","ts":100}"#).unwrap();
        assert!(read_update_snapshot_in(&path, 600, 701).is_none());
        assert!(read_update_snapshot_in(dir.path().join("missing.json"), 600, 100).is_none());
    }

    #[test]
    fn watcher_projection_respects_quarantine_and_digest_states() {
        let mut snapshot = UpdateSnapshot {
            result: "quarantined".into(),
            ..Default::default()
        };
        assert_eq!(snapshot.system_state(), "uptodate");
        snapshot.result = "checked".into();
        snapshot.booted_digest = "sha256:a".into();
        snapshot.remote_digest = "sha256:b".into();
        assert_eq!(snapshot.system_state(), "available");
        snapshot.staged_digest = "sha256:c".into();
        assert_eq!(snapshot.system_state(), "staged");
    }

    #[test]
    fn cached_status_does_not_require_a_registry_probe() {
        let data = serde_json::json!({
            "status": {
                "booted": {"image": {"imageDigest": "sha256:booted"}},
                "staged": null,
                "rollback": null
            }
        });
        let watcher = UpdateSnapshot {
            result: "checked".into(),
            ts: 100,
            booted_digest: "sha256:booted".into(),
            remote_digest: "sha256:booted".into(),
            ..Default::default()
        };
        let status = project_cached_status(Some(&data), Some(&watcher));
        assert_eq!(status.check_state, "uptodate");
        assert_eq!(status.remote_digest.as_deref(), Some("sha256:booted"));
    }

    #[test]
    fn returns_status() {
        let s = check_update_status();
        assert!(["available", "uptodate", "error", "idle", "checking"]
            .contains(&s.check_state.as_str()));
    }
}
