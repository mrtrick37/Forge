//! Idempotent single-file work migration.
//!
//! This ports the narrow `work_migration_idempotent` helper. It only handles
//! one caller-selected source and destination, skips destinations at least as
//! new as the source, and replaces the destination atomically after copying.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temporary_path(destination: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("kyth-work");
    destination.with_file_name(format!(".{name}.{}.{}.tmp", std::process::id(), nonce))
}

/// Copy `source` to `destination` only when the source is newer.
///
/// Returns `true` when a replacement was performed and `false` for a missing
/// invalid source, an up-to-date destination, or an I/O failure, matching the
/// Python helper's best-effort boolean contract.
pub fn copy_if_newer(source: impl AsRef<Path>, destination: impl AsRef<Path>) -> bool {
    let source = source.as_ref();
    let destination = destination.as_ref();
    if destination.exists() {
        if let (Ok(source_meta), Ok(destination_meta)) =
            (std::fs::metadata(source), std::fs::metadata(destination))
        {
            if destination_meta.modified().ok() >= source_meta.modified().ok() {
                return false;
            }
        }
    }
    let temporary = temporary_path(destination);
    let copied = std::fs::copy(source, &temporary).is_ok();
    if !copied {
        let _ = std::fs::remove_file(&temporary);
        return false;
    }
    if std::fs::rename(&temporary, destination).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return false;
    }
    if let Some(parent) = destination.parent() {
        if let Ok(directory) = std::fs::File::open(parent) {
            let _ = directory.sync_all();
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn copies_missing_destination_once_and_then_skips_it() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("source.txt");
        let destination = directory.path().join("nested").join("destination.txt");
        fs::write(&source, "work data").unwrap();
        assert!(!copy_if_newer(&source, &destination));
        // The Python helper also expects the caller to create the destination
        // parent; this keeps an accidental path typo from creating directories.
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        assert!(copy_if_newer(&source, &destination));
        assert_eq!(fs::read_to_string(&destination).unwrap(), "work data");
        assert!(!copy_if_newer(&source, &destination));
    }

    #[test]
    fn missing_source_is_non_fatal() {
        let directory = tempdir().unwrap();
        assert!(!copy_if_newer(
            directory.path().join("missing"),
            directory.path().join("out")
        ));
    }
}
