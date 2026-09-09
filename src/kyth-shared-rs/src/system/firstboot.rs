//! First-boot application setup markers.

use std::path::{Path, PathBuf};

pub fn default_flatpaks_sentinel(root: impl AsRef<Path>) -> Option<PathBuf> {
    let mut best: Option<(i64, PathBuf)> = None;
    let Ok(entries) = root.as_ref().read_dir() else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let Some(version) = name
            .strip_prefix("default-flatpaks-v")
            .and_then(|name| name.strip_suffix("-done"))
            .and_then(|version| version.parse::<i64>().ok())
        else {
            continue;
        };
        if best.as_ref().is_none_or(|(current, _)| version >= *current) {
            best = Some((version, path));
        }
    }
    best.map(|(_, path)| path)
}

pub fn default_flatpaks_done(root: impl AsRef<Path>) -> bool {
    default_flatpaks_sentinel(root).is_some()
}

pub fn is_live_session(cmdline: impl AsRef<Path>) -> bool {
    std::fs::read_to_string(cmdline)
        .ok()
        .is_some_and(|text| text.split_whitespace().any(|token| token == "kyth.live"))
}

/// Render the status-file contract consumed by the welcome and Hub UIs.
pub fn app_status_content(state: &str, message: &str, updated: &str) -> String {
    format!("state={state}\nmessage={message}\nupdated={updated}\n")
}

/// Persist first-boot status through the shared crash-safe writer.
pub fn write_app_status(
    path: impl AsRef<Path>,
    state: &str,
    message: &str,
    updated: &str,
) -> std::io::Result<()> {
    let content = app_status_content(state, message, updated);
    crate::atomic_io::atomic_write_text(path, &content, Some(0o644))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn selects_newest_valid_flatpak_marker() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("default-flatpaks-v2-done"), "").unwrap();
        fs::write(directory.path().join("default-flatpaks-v12-done"), "").unwrap();
        fs::write(directory.path().join("default-flatpaks-vbad-done"), "").unwrap();
        assert_eq!(
            default_flatpaks_sentinel(directory.path())
                .unwrap()
                .file_name()
                .unwrap(),
            "default-flatpaks-v12-done"
        );
        assert!(default_flatpaks_done(directory.path()));
    }

    #[test]
    fn renders_and_writes_firstboot_status_contract() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("first-run-apps.status");
        assert_eq!(
            app_status_content("ready", "All set", "now"),
            "state=ready\nmessage=All set\nupdated=now\n"
        );
        write_app_status(&path, "ready", "All set", "now").unwrap();
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "state=ready\nmessage=All set\nupdated=now\n"
        );
    }
}
