//! Read-only port of `kyth_shared.boot_health`.
//!
//! The Python module remains authoritative for state transitions, atomic
//! writes, and rollback execution.  This module owns the small read/policy
//! surface Rust consumers need: decoding the on-disk state, finding the
//! newest quarantine, and evaluating image rollout rings.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

pub const SCHEMA_VERSION: u64 = 1;
pub const DEFAULT_FAILURE_THRESHOLD: i64 = 3;
pub const DEFAULT_STATE_PATH: &str = "/var/lib/kyth/boot-health.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuarantineRecord {
    pub digest: String,
    pub failures: i64,
    pub reason: String,
    pub first_failed_at: i64,
    pub last_failed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BootHealthState {
    pub current_digest: String,
    pub last_healthy_digest: String,
    pub pending_digest: String,
    pub status: String,
    pub failures: i64,
    pub last_failure_boot_id: String,
    pub last_reason: String,
    pub last_recovered_digest: String,
    pub last_recovery_at: i64,
    pub rollout_ring: String,
    pub updated_at: i64,
    pub quarantined: HashMap<String, QuarantineRecord>,
    pub rollback_attempted_for: String,
    pub last_rollback_error: String,
    pub last_rollback_at: i64,
}

impl Default for BootHealthState {
    fn default() -> Self {
        Self {
            current_digest: String::new(),
            last_healthy_digest: String::new(),
            pending_digest: String::new(),
            status: "unknown".into(),
            failures: 0,
            last_failure_boot_id: String::new(),
            last_reason: String::new(),
            last_recovered_digest: String::new(),
            last_recovery_at: 0,
            rollout_ring: "follow-image".into(),
            updated_at: 0,
            quarantined: HashMap::new(),
            rollback_attempted_for: String::new(),
            last_rollback_error: String::new(),
            last_rollback_at: 0,
        }
    }
}

impl BootHealthState {
    /// Match Python's invariant checks for the fields used by recovery UI.
    pub fn invariants(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.failures < 0 {
            errors.push("failures<0".to_string());
        }
        if self.status == "healthy" && self.last_healthy_digest.is_empty() {
            errors.push("healthy but last_healthy_digest empty".to_string());
        }
        for (digest, record) in &self.quarantined {
            if record.failures < DEFAULT_FAILURE_THRESHOLD {
                errors.push(format!(
                    "quarantined {digest} failures {} < threshold",
                    record.failures
                ));
            }
            if digest != &record.digest {
                errors.push(format!(
                    "quarantined key {digest} != record.digest {}",
                    record.digest
                ));
            }
        }
        errors
    }

    pub fn newest_quarantine(&self) -> Option<&QuarantineRecord> {
        self.quarantined
            .values()
            .max_by_key(|record| record.last_failed_at)
    }
}

/// Decode a boot-health JSON document with the same fail-safe behavior as
/// Python's `BootHealthState.from_dict`: unknown schema or malformed top-level
/// data becomes an empty state, while malformed individual quarantine entries
/// are ignored.
pub fn state_from_json(text: &str) -> BootHealthState {
    let Ok(mut value) = serde_json::from_str::<Value>(text) else {
        return BootHealthState::default();
    };
    let Some(object) = value.as_object_mut() else {
        return BootHealthState::default();
    };
    let schema_version = match object.get("schema_version") {
        None => SCHEMA_VERSION,
        Some(Value::Number(number)) => number.as_u64().unwrap_or(0),
        Some(_) => 0,
    };
    if schema_version != SCHEMA_VERSION {
        return BootHealthState::default();
    }

    // Python filters quarantine entries independently before constructing the
    // state. Keep that behavior even if one old/corrupt entry is malformed.
    let mut quarantined = HashMap::new();
    if let Some(Value::Object(raw)) = object.get("quarantined") {
        for (digest, record) in raw {
            let Some(record_object) = record.as_object() else {
                continue;
            };
            if record_object.len() != 5
                || ![
                    "digest",
                    "failures",
                    "reason",
                    "first_failed_at",
                    "last_failed_at",
                ]
                .iter()
                .all(|key| record_object.contains_key(*key))
            {
                continue;
            }
            let Ok(record) = serde_json::from_value::<QuarantineRecord>(record.clone()) else {
                continue;
            };
            if record.digest == *digest {
                quarantined.insert(digest.clone(), record);
            }
        }
    }
    object.remove("quarantined");
    let Ok(mut state) = serde_json::from_value::<BootHealthState>(Value::Object(object.clone()))
    else {
        return BootHealthState::default();
    };
    state.quarantined = quarantined;
    state
}

pub fn read_state(path: impl AsRef<Path>) -> BootHealthState {
    std::fs::read_to_string(path)
        .map(|text| state_from_json(&text))
        .unwrap_or_default()
}

pub fn read_default_state() -> BootHealthState {
    read_state(DEFAULT_STATE_PATH)
}

pub fn quarantine_reason(state: &BootHealthState, digest: &str) -> Option<String> {
    state.quarantined.get(digest).map(|record| {
        format!(
            "digest {digest} is quarantined after {} unhealthy boots: {}",
            record.failures, record.reason
        )
    })
}

/// Record that an image was staged without performing any persistence.
pub fn record_staged(
    state: &BootHealthState,
    digest: &str,
    rollout_ring: &str,
    now: i64,
) -> BootHealthState {
    let mut updated = state.clone();
    updated.pending_digest = digest.into();
    updated.rollout_ring = rollout_ring.into();
    updated.updated_at = now;
    updated
}

/// Record one failed boot, deduplicated by deployment and boot identifier.
///
/// This is intentionally a pure state transition. The coordinator or the
/// existing Python service owns locking and persistence; no rollback command
/// is started here.
pub fn record_failure(
    state: &BootHealthState,
    digest: &str,
    boot_id: &str,
    reason: &str,
    threshold: i64,
    now: i64,
) -> BootHealthState {
    let same_deployment = state.current_digest == digest;
    let mut failures = if same_deployment { state.failures } else { 0 };
    if !(same_deployment && state.last_failure_boot_id == boot_id) {
        failures += 1;
    }

    let mut quarantined = state.quarantined.clone();
    if failures >= threshold {
        let first_failed_at = quarantined
            .get(digest)
            .map_or(now, |record| record.first_failed_at);
        quarantined.insert(
            digest.into(),
            QuarantineRecord {
                digest: digest.into(),
                failures,
                reason: reason.into(),
                first_failed_at,
                last_failed_at: now,
            },
        );
    }
    // Keep the field-by-field shape aligned with the Python constructor. In
    // particular, rollback_attempted_for must survive unrelated failures so
    // a bad rollback target cannot create a ping-pong loop.
    BootHealthState {
        current_digest: digest.into(),
        last_healthy_digest: state.last_healthy_digest.clone(),
        pending_digest: if state.pending_digest == digest {
            String::new()
        } else {
            state.pending_digest.clone()
        },
        status: if quarantined.contains_key(digest) {
            "quarantined"
        } else {
            "unhealthy"
        }
        .into(),
        failures,
        last_failure_boot_id: boot_id.into(),
        last_reason: reason.into(),
        last_recovered_digest: state.last_recovered_digest.clone(),
        last_recovery_at: state.last_recovery_at,
        rollout_ring: state.rollout_ring.clone(),
        updated_at: now,
        quarantined,
        rollback_attempted_for: state.rollback_attempted_for.clone(),
        last_rollback_error: state.last_rollback_error.clone(),
        last_rollback_at: state.last_rollback_at,
    }
}

/// Mark a deployment healthy and clear its quarantine record.
pub fn mark_healthy(state: &BootHealthState, digest: &str, now: i64) -> BootHealthState {
    let mut quarantined = state.quarantined.clone();
    quarantined.remove(digest);
    let recovered_digest = if state.current_digest != digest
        && state.quarantined.contains_key(&state.current_digest)
    {
        state.current_digest.clone()
    } else {
        String::new()
    };
    BootHealthState {
        current_digest: digest.into(),
        last_healthy_digest: digest.into(),
        pending_digest: if state.pending_digest == digest {
            String::new()
        } else {
            state.pending_digest.clone()
        },
        status: if recovered_digest.is_empty() {
            "healthy"
        } else {
            "recovered"
        }
        .into(),
        failures: 0,
        last_failure_boot_id: String::new(),
        last_reason: if recovered_digest.is_empty() {
            state.last_reason.clone()
        } else {
            format!("Automatically recovered from quarantined digest {recovered_digest}")
        },
        last_recovered_digest: if recovered_digest.is_empty() {
            state.last_recovered_digest.clone()
        } else {
            recovered_digest.clone()
        },
        last_recovery_at: if recovered_digest.is_empty() {
            state.last_recovery_at
        } else {
            now
        },
        rollout_ring: state.rollout_ring.clone(),
        updated_at: now,
        quarantined,
        rollback_attempted_for: state.rollback_attempted_for.clone(),
        last_rollback_error: state.last_rollback_error.clone(),
        last_rollback_at: state.last_rollback_at,
    }
}

/// Record a one-shot rollback attempt without executing it.
pub fn note_rollback_attempted(
    state: &BootHealthState,
    digest: &str,
    error: Option<&str>,
    now: i64,
) -> BootHealthState {
    let mut updated = state.clone();
    updated.rollback_attempted_for = digest.into();
    updated.last_rollback_error = error.unwrap_or_default().into();
    updated.last_rollback_at = now;
    updated.updated_at = now;
    updated
}

/// Remove one quarantine record while preserving the rest of the state.
pub fn clear_quarantine(state: &BootHealthState, digest: &str, now: i64) -> BootHealthState {
    let mut updated = state.clone();
    updated.quarantined.remove(digest);
    if updated.current_digest == digest && updated.status == "quarantined" {
        updated.status = "unhealthy".into();
    }
    updated.updated_at = now;
    updated
}

const VALID_ROLLOUT_RINGS: [&str; 4] = ["follow-image", "canary", "testing", "stable"];

pub fn image_ring(reference: &str) -> Option<&'static str> {
    let without_digest = reference
        .split_once('@')
        .map_or(reference, |(prefix, _)| prefix);
    let tag = without_digest
        .rsplit_once(':')
        .map(|(_, tag)| tag)
        .unwrap_or("");
    let tag = tag.strip_suffix("-cachy").unwrap_or(tag);
    match tag {
        "canary" => Some("canary"),
        "testing" => Some("testing"),
        "latest" => Some("stable"),
        _ => None,
    }
}

pub fn rollout_policy_reason(reference: &str, configured_ring: &str) -> Option<String> {
    if !VALID_ROLLOUT_RINGS.contains(&configured_ring) {
        return Some(format!("invalid rollout ring '{configured_ring}'"));
    }
    let actual = image_ring(reference);
    if configured_ring == "follow-image" || actual == Some(configured_ring) {
        return None;
    }
    Some(actual.map_or_else(
        || format!("cannot determine rollout ring from booted image '{reference}'"),
        |ring| format!("booted image belongs to {ring} ring, configured for {configured_ring}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn decodes_quarantine_and_ignores_bad_entries() {
        let text = r#"{
          "schema_version": 1,
          "status": "quarantined",
          "quarantined": {
            "sha256:old": {"digest":"sha256:old","failures":3,"reason":"failed","first_failed_at":1,"last_failed_at":4},
            "sha256:mismatch": {"digest":"sha256:other","failures":9},
            "sha256:bad": "not a record"
          }
        }"#;
        let state = state_from_json(text);
        assert_eq!(state.status, "quarantined");
        assert_eq!(state.quarantined.len(), 1);
        assert_eq!(state.newest_quarantine().unwrap().digest, "sha256:old");
        assert_eq!(
            quarantine_reason(&state, "sha256:old").unwrap(),
            "digest sha256:old is quarantined after 3 unhealthy boots: failed"
        );
    }

    #[test]
    fn invalid_schema_and_file_are_empty() {
        assert_eq!(
            state_from_json(r#"{"schema_version": 2}"#),
            BootHealthState::default()
        );
        let directory = tempdir().unwrap();
        let path = directory.path().join("boot-health.json");
        fs::write(&path, "not json").unwrap();
        assert_eq!(read_state(path), BootHealthState::default());
    }

    #[test]
    fn default_state_matches_python_defaults() {
        let state = BootHealthState::default();
        assert_eq!(state.status, "unknown");
        assert_eq!(state.rollout_ring, "follow-image");
        assert_eq!(state.failures, 0);
    }

    #[test]
    fn transitions_dedupe_failures_and_quarantine_at_the_threshold() {
        let digest = "sha256:aaa";
        let mut state = record_staged(&BootHealthState::default(), digest, "testing", 1);
        state = record_failure(&state, digest, "boot-1", "display failed", 3, 2);
        let duplicate = record_failure(&state, digest, "boot-1", "display failed", 3, 3);
        assert_eq!(duplicate.failures, 1);
        state = record_failure(&duplicate, digest, "boot-2", "display failed", 3, 4);
        state = record_failure(&state, digest, "boot-3", "display failed", 3, 5);
        assert_eq!(state.status, "quarantined");
        assert_eq!(state.quarantined[digest].first_failed_at, 5);
        assert_eq!(state.quarantined[digest].last_failed_at, 5);
        assert!(state.pending_digest.is_empty());
    }

    #[test]
    fn recovery_clears_quarantine_and_records_a_changed_digest() {
        let first = "sha256:first";
        let second = "sha256:second";
        let mut state = BootHealthState::default();
        for (index, boot) in ["boot-1", "boot-2", "boot-3"].into_iter().enumerate() {
            state = record_failure(&state, first, boot, "failed", 3, index as i64);
        }
        state = note_rollback_attempted(&state, first, Some("rollback failed"), 4);
        let recovered = mark_healthy(&state, second, 5);
        assert_eq!(recovered.status, "recovered");
        assert_eq!(recovered.last_recovered_digest, first);
        assert!(recovered.quarantined.contains_key(first));
        assert_eq!(recovered.rollback_attempted_for, first);
        assert_eq!(recovered.last_rollback_error, "rollback failed");
        let cleared = clear_quarantine(&recovered, first, 6);
        assert_eq!(cleared.status, "recovered");
        assert!(!cleared.quarantined.contains_key(first));
    }

    #[test]
    fn rollout_policy_matches_python() {
        assert_eq!(
            image_ring("ghcr.io/kyth-os/kyth:latest-cachy@sha256:x"),
            Some("stable")
        );
        assert_eq!(image_ring("ghcr.io/kyth-os/kyth:testing"), Some("testing"));
        assert_eq!(
            rollout_policy_reason("image:testing", "stable"),
            Some("booted image belongs to testing ring, configured for stable".to_string())
        );
        assert_eq!(rollout_policy_reason("image:unknown", "follow-image"), None);
        assert_eq!(
            rollout_policy_reason("image:unknown", "bogus"),
            Some("invalid rollout ring 'bogus'".to_string())
        );
    }
}
