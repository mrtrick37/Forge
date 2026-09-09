//! Read-only Windows migration parity checks.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WindowsParity {
    pub bookmarks: String,
    pub drives: String,
    pub files: String,
    pub onedrive: String,
    pub pwa: String,
    pub parity: String,
}

pub fn verify(home: impl AsRef<Path>, var_home_exists: bool) -> WindowsParity {
    let home = home.as_ref();
    let bookmarks = if [
        home.join(".config/chromium/Default/Bookmarks"),
        home.join(".mozilla"),
    ]
    .iter()
    .any(|path| path.exists())
    {
        "found"
    } else {
        "missing"
    };
    let drives = if var_home_exists { "found" } else { "unknown" };
    let files = if home.join(".local/share/kyth/files-copy.json").is_file() {
        "done"
    } else {
        "pending"
    };
    let onedrive = if home.join(".config/rclone/rclone.conf").is_file() {
        "configured"
    } else {
        "missing"
    };
    let pwa_count = home
        .join(".local/share/applications")
        .read_dir()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "desktop")
        })
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .filter(|text| text.contains("Teams") || text.contains("Outlook"))
        .count();
    let pwa = format!("{pwa_count} PWA");
    let missing = [bookmarks, drives, files, onedrive]
        .iter()
        .any(|value| matches!(*value, "missing" | "pending" | "unknown"));
    WindowsParity {
        bookmarks: bookmarks.into(),
        drives: drives.into(),
        files: files.into(),
        onedrive: onedrive.into(),
        pwa,
        parity: if missing {
            "missing migration items".into()
        } else {
            "ok".into()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn reports_migration_markers() {
        let directory = tempdir().unwrap();
        fs::create_dir_all(directory.path().join(".mozilla")).unwrap();
        let result = verify(directory.path(), true);
        assert_eq!(result.bookmarks, "found");
        assert_eq!(result.files, "pending");
        assert_eq!(result.parity, "missing migration items");
    }
}
