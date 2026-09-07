//! Locked atomic coordinator for boot-health/staged-update state.
//!
//! This ports the synchronization primitive from `update_coordinator.py`.
//! Callers still decide which state transition is valid; the coordinator only
//! guarantees that a read/transform/write transaction cannot lose a concurrent
//! update.

use super::boot_health::{
    clear_quarantine, mark_healthy, note_rollback_attempted, read_state, record_failure,
    record_staged, BootHealthState,
};
use rustix::fs::{flock, FlockOperation};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct UpdateCoordinator {
    path: PathBuf,
}

impl UpdateCoordinator {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn read(&self) -> BootHealthState {
        read_state(&self.path)
    }

    pub fn record_staged(
        &self,
        digest: &str,
        rollout_ring: &str,
        now: i64,
    ) -> std::io::Result<BootHealthState> {
        self.transaction(|state| record_staged(&state, digest, rollout_ring, now))
    }

    pub fn record_failure(
        &self,
        digest: &str,
        boot_id: &str,
        reason: &str,
        threshold: i64,
        now: i64,
    ) -> std::io::Result<BootHealthState> {
        self.transaction(|state| record_failure(&state, digest, boot_id, reason, threshold, now))
    }

    pub fn mark_healthy(&self, digest: &str, now: i64) -> std::io::Result<BootHealthState> {
        self.transaction(|state| mark_healthy(&state, digest, now))
    }

    pub fn clear_quarantine(&self, digest: &str, now: i64) -> std::io::Result<BootHealthState> {
        self.transaction(|state| clear_quarantine(&state, digest, now))
    }

    pub fn note_rollback_attempted(
        &self,
        digest: &str,
        error: Option<&str>,
        now: i64,
    ) -> std::io::Result<BootHealthState> {
        self.transaction(|state| note_rollback_attempted(&state, digest, error, now))
    }

    pub fn transaction<F>(&self, transform: F) -> std::io::Result<BootHealthState>
    where
        F: FnOnce(BootHealthState) -> BootHealthState,
    {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let lock_path = PathBuf::from(format!("{}.lock", self.path.display()));
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&lock_path)?;
        flock(&lock, FlockOperation::LockExclusive)?;
        let current = read_state(&self.path);
        let updated = transform(current);
        let mut document = serde_json::to_value(&updated)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        document
            .as_object_mut()
            .expect("BootHealthState serializes as an object")
            .insert("schema_version".into(), serde_json::json!(1));
        if !updated.invariants().is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "refusing to persist invalid boot-health state: {:?}",
                    updated.invariants()
                ),
            ));
        }
        let payload = serde_json::to_string_pretty(&document)
            .map(|value| format!("{value}\n"))
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        // Boot-health state contains digests and health metadata, not secrets;
        // the desktop Hub and notifier must be able to read it after a root
        // update worker writes it.
        let result = crate::atomic_io::atomic_write_text(&self.path, &payload, Some(0o644));
        let _ = flock(&lock, FlockOperation::Unlock);
        result.map(|()| updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn transaction_serializes_a_single_writer_update() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("boot-health.json");
        let coordinator = UpdateCoordinator::new(&path);
        let updated = coordinator
            .transaction(|mut state| {
                state.status = "staged".into();
                state.pending_digest = "sha256:new".into();
                state
            })
            .unwrap();
        assert_eq!(updated.status, "staged");
        assert_eq!(coordinator.read().pending_digest, "sha256:new");
        assert!(path.with_file_name("boot-health.json.lock").is_file());
        let document: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(document["schema_version"], 1);
    }

    #[test]
    fn convenience_transitions_use_the_same_locked_writer() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("boot-health.json");
        let coordinator = UpdateCoordinator::new(&path);
        coordinator
            .record_staged("sha256:new", "testing", 1)
            .unwrap();
        let state = coordinator
            .record_failure("sha256:new", "boot-1", "display", 3, 2)
            .unwrap();
        let state = coordinator
            .record_failure("sha256:new", "boot-2", "display", 3, 3)
            .unwrap();
        let state = coordinator
            .record_failure("sha256:new", "boot-3", "display", 3, 4)
            .unwrap();
        assert_eq!(state.status, "quarantined");
        coordinator
            .note_rollback_attempted("sha256:new", Some("failed"), 3)
            .unwrap();
        let healthy = coordinator.mark_healthy("sha256:new", 4).unwrap();
        assert_eq!(healthy.status, "healthy");
        assert_eq!(healthy.rollback_attempted_for, "sha256:new");
        assert_eq!(healthy.last_rollback_error, "failed");
    }
}
