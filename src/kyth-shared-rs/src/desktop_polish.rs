//! Declarative user-polish manifest and desktop-entry drift checks.
//!
//! KDE writes, folder creation, and session commands remain caller-owned. The
//! constants and comparison helper are safe to share with native clients.

pub const VERSION: &str = "v13";
pub const PLACES_VERSION: &str = "v1";
pub const AUTOSTART_VERSION: &str = "v1";

pub const USER_FOLDERS: [&str; 10] = [
    "Desktop",
    "Documents",
    "Downloads",
    "Games",
    "Music",
    "Pictures",
    "Public",
    "Screenshots",
    "Templates",
    "Videos",
];

pub const FOLDER_METADATA: [(&str, &str); 3] = [
    (
        "Games/.directory",
        "[Desktop Entry]\nIcon=applications-games\nName=Games\n",
    ),
    (
        "Screenshots/.directory",
        "[Desktop Entry]\nIcon=folder-pictures\nName=Screenshots\n",
    ),
    ("Templates/Plain Text.txt", ""),
];

pub const MIME_DEFAULTS: [(&str, &str); 29] = [
    ("org.kde.okular.desktop", "application/pdf"),
    ("org.kde.okular.desktop", "application/epub+zip"),
    ("org.kde.gwenview.desktop", "image/jpeg"),
    ("org.kde.gwenview.desktop", "image/png"),
    ("org.kde.gwenview.desktop", "image/gif"),
    ("org.kde.gwenview.desktop", "image/webp"),
    ("org.videolan.VLC.desktop", "video/mp4"),
    ("org.videolan.VLC.desktop", "video/x-matroska"),
    ("org.videolan.VLC.desktop", "video/x-msvideo"),
    ("org.videolan.VLC.desktop", "audio/mpeg"),
    ("org.videolan.VLC.desktop", "audio/flac"),
    ("org.kde.kwrite.desktop", "text/plain"),
    ("org.kde.kwrite.desktop", "text/markdown"),
    ("org.kde.ark.desktop", "application/zip"),
    ("org.kde.ark.desktop", "application/x-7z-compressed"),
    ("org.kde.ark.desktop", "application/x-rar"),
    ("org.kde.ark.desktop", "application/x-tar"),
    (
        "kyth-exe-handler.desktop",
        "application/x-ms-dos-executable",
    ),
    ("kyth-exe-handler.desktop", "application/x-msdos-program"),
    ("kyth-exe-handler.desktop", "application/x-dosexec"),
    ("kyth-exe-handler.desktop", "application/x-msi"),
    ("kyth-exe-handler.desktop", "application/x-msdownload"),
    (
        "kyth-exe-handler.desktop",
        "application/vnd.microsoft.portable-executable",
    ),
    ("kyth-exe-handler.desktop", "application/x-rpm"),
    (
        "kyth-exe-handler.desktop",
        "application/x-redhat-package-manager",
    ),
    ("com.brave.Browser.desktop", "x-scheme-handler/http"),
    ("com.brave.Browser.desktop", "x-scheme-handler/https"),
    (
        "com.getmailspring.Mailspring.desktop",
        "x-scheme-handler/mailto",
    ),
    ("org.kde.dolphin.desktop", "inode/directory"),
];

fn desktop_entry_field<'a>(text: &'a str, key: &str) -> &'a str {
    let prefix = format!("{key}=");
    text.lines()
        .find_map(|line| line.strip_prefix(&prefix).map(str::trim))
        .unwrap_or_default()
}

/// True when an existing Kyth desktop shortcut has drifted from the shipped
/// Name/Comment/GenericName fields and should be refreshed.
pub fn should_refresh_pulse_desktop_shortcut(existing: &str, shipped: &str) -> bool {
    !existing.is_empty()
        && !shipped.is_empty()
        && existing.contains("kyth-welcome")
        && ["Name", "Comment", "GenericName"]
            .into_iter()
            .any(|key| desktop_entry_field(existing, key) != desktop_entry_field(shipped, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_manifest_data_without_side_effects() {
        assert_eq!(VERSION, "v13");
        assert!(USER_FOLDERS.contains(&"Games"));
        assert!(MIME_DEFAULTS.contains(&("kyth-exe-handler.desktop", "application/x-rpm")));
        assert_eq!(FOLDER_METADATA.len(), 3);
    }

    #[test]
    fn detects_only_owned_shortcut_drift() {
        let shipped = "[Desktop Entry]\nName=KythOS\nComment=Welcome\nGenericName=System Hub\n";
        let stale = "[Desktop Entry]\nName=KythOS\nComment=Old\nGenericName=System Hub\nExec=kyth-welcome\n";
        assert!(should_refresh_pulse_desktop_shortcut(stale, shipped));
        assert!(!should_refresh_pulse_desktop_shortcut(shipped, shipped));
        assert!(!should_refresh_pulse_desktop_shortcut(
            "Exec=other-app\n",
            shipped
        ));
    }
}
