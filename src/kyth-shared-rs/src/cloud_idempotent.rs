//! Bounded cloud-sync idempotency helpers.
//!
//! This ports the key/manifest contract from `kyth_shared.cloud_idempotent`.
//! It does not run rclone. Manifest persistence is explicit and uses the
//! shared atomic writer so callers choose the destination deliberately.

use std::path::Path;

pub fn sync_key(remote: &str) -> String {
    format!("rclone-sync:{remote}")
}

pub fn dry_run_message(remote: &str) -> String {
    format!("{} dry-run", sync_key(remote))
}

pub fn manifest_content(remote: &str) -> String {
    let key = sync_key(remote);
    format!(
        "{{\"remote\": {}, \"key\": {}}}",
        serde_json::to_string(remote).expect("a Rust string serializes as JSON"),
        serde_json::to_string(&key).expect("a Rust string serializes as JSON"),
    )
}

pub fn write_manifest(path: impl AsRef<Path>, remote: &str) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(path, &manifest_content(remote), Some(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn builds_stable_sync_key_and_preview() {
        assert_eq!(sync_key("nas:games"), "rclone-sync:nas:games");
        assert_eq!(
            dry_run_message("nas:games"),
            "rclone-sync:nas:games dry-run"
        );
    }

    #[test]
    fn writes_escaped_manifest_to_an_explicit_destination() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("manifest.json");
        write_manifest(&path, "nas:\"games\"").unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(value["remote"], "nas:\"games\"");
        assert_eq!(value["key"], "rclone-sync:nas:\"games\"");
    }
}
