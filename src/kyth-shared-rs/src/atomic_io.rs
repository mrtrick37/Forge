//! Crash-safe, symlink-resistant file replacement helpers.
//!
//! These are the Rust counterpart of `kyth_shared.atomic_io`. They are kept
//! deliberately small: callers provide the serialized bytes and this module
//! only owns the replace protocol.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;

fn refuse_symlink(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("refusing to replace symlink: {}", path.display()),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Atomically replace `path` after writing and syncing a sibling temporary file.
pub fn atomic_write_bytes(
    path: impl AsRef<Path>,
    data: &[u8],
    mode: Option<u32>,
) -> std::io::Result<()> {
    let path = path.as_ref();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    refuse_symlink(path)?;

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("kyth");
    let temporary = parent.join(format!(".{file_name}.{}.{}.tmp", std::process::id(), nonce));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        if let Some(mode) = mode {
            options.mode(mode);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);

        refuse_symlink(path)?;
        fs::rename(&temporary, path)?;
        File::open(parent).and_then(|directory| directory.sync_all())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn atomic_write_text(
    path: impl AsRef<Path>,
    content: &str,
    mode: Option<u32>,
) -> std::io::Result<()> {
    atomic_write_bytes(path, content.as_bytes(), mode)
}

pub fn atomic_write_json<T: serde::Serialize>(
    path: impl AsRef<Path>,
    value: &T,
    mode: Option<u32>,
) -> std::io::Result<()> {
    let content = serde_json::to_vec_pretty(value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut content = content;
    content.push(b'\n');
    atomic_write_bytes(path, &content, mode)
}

/// Read JSON state or return the supplied fallback when a file is missing,
/// malformed, or temporarily unavailable during recovery.
pub fn read_json_or_default<T: DeserializeOwned>(path: impl AsRef<Path>, default: T) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;
    use tempfile::tempdir;

    #[derive(Serialize)]
    struct Example {
        value: &'static str,
    }

    #[derive(serde::Deserialize)]
    struct LoadedExample {
        value: String,
    }

    #[test]
    fn replaces_and_syncs_text() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("state.txt");
        atomic_write_text(&path, "first", Some(0o600)).unwrap();
        atomic_write_text(&path, "second", Some(0o600)).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "second");
    }

    #[test]
    fn writes_json_with_newline() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("state.json");
        atomic_write_json(&path, &Example { value: "ok" }, Some(0o600)).unwrap();
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "{\n  \"value\": \"ok\"\n}\n"
        );
    }

    #[test]
    fn reads_valid_json_and_falls_back_on_missing_or_malformed_state() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("state.json");
        fs::write(&path, r#"{"value":"loaded"}"#).unwrap();
        let loaded: LoadedExample = read_json_or_default(
            &path,
            LoadedExample {
                value: "default".to_string(),
            },
        );
        assert_eq!(loaded.value, "loaded");
        fs::write(&path, "not json").unwrap();
        let fallback: LoadedExample = read_json_or_default(
            &path,
            LoadedExample {
                value: "default".to_string(),
            },
        );
        assert_eq!(fallback.value, "default");
        assert_eq!(
            read_json_or_default(directory.path().join("missing.json"), 7_u32),
            7
        );
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_target() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("target");
        let link = directory.path().join("link");
        fs::write(&target, "original").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(atomic_write_text(&link, "changed", None).is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "original");
    }
}
