//! Declarative desktop-polish manifest shared by UI and installers.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderMetadata {
    pub path: &'static str,
    pub content: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MimeDefault {
    pub desktop_file: &'static str,
    pub mime_type: &'static str,
}

pub const VERSION: &str = "v13";
pub const PLACES_VERSION: &str = "v1";
pub const AUTOSTART_VERSION: &str = "v1";

pub const USER_FOLDERS: &[&str] = &[
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

pub const FOLDER_METADATA: &[FolderMetadata] = &[
    FolderMetadata {
        path: "Games/.directory",
        content: "[Desktop Entry]\nIcon=applications-games\nName=Games\n",
    },
    FolderMetadata {
        path: "Screenshots/.directory",
        content: "[Desktop Entry]\nIcon=folder-pictures\nName=Screenshots\n",
    },
    FolderMetadata {
        path: "Templates/Plain Text.txt",
        content: "",
    },
];

pub const MIME_DEFAULTS: &[MimeDefault] = &[
    MimeDefault {
        desktop_file: "org.kde.okular.desktop",
        mime_type: "application/pdf",
    },
    MimeDefault {
        desktop_file: "org.kde.okular.desktop",
        mime_type: "application/epub+zip",
    },
    MimeDefault {
        desktop_file: "org.kde.gwenview.desktop",
        mime_type: "image/jpeg",
    },
    MimeDefault {
        desktop_file: "org.kde.gwenview.desktop",
        mime_type: "image/png",
    },
    MimeDefault {
        desktop_file: "org.kde.gwenview.desktop",
        mime_type: "image/gif",
    },
    MimeDefault {
        desktop_file: "org.kde.gwenview.desktop",
        mime_type: "image/webp",
    },
    MimeDefault {
        desktop_file: "org.videolan.VLC.desktop",
        mime_type: "video/mp4",
    },
    MimeDefault {
        desktop_file: "org.videolan.VLC.desktop",
        mime_type: "video/x-matroska",
    },
    MimeDefault {
        desktop_file: "org.videolan.VLC.desktop",
        mime_type: "video/x-msvideo",
    },
    MimeDefault {
        desktop_file: "org.videolan.VLC.desktop",
        mime_type: "audio/mpeg",
    },
    MimeDefault {
        desktop_file: "org.videolan.VLC.desktop",
        mime_type: "audio/flac",
    },
    MimeDefault {
        desktop_file: "org.kde.kwrite.desktop",
        mime_type: "text/plain",
    },
    MimeDefault {
        desktop_file: "org.kde.kwrite.desktop",
        mime_type: "text/markdown",
    },
    MimeDefault {
        desktop_file: "org.kde.ark.desktop",
        mime_type: "application/zip",
    },
    MimeDefault {
        desktop_file: "org.kde.ark.desktop",
        mime_type: "application/x-7z-compressed",
    },
    MimeDefault {
        desktop_file: "org.kde.ark.desktop",
        mime_type: "application/x-rar",
    },
    MimeDefault {
        desktop_file: "org.kde.ark.desktop",
        mime_type: "application/x-tar",
    },
    MimeDefault {
        desktop_file: "kyth-exe-handler.desktop",
        mime_type: "application/x-ms-dos-executable",
    },
    MimeDefault {
        desktop_file: "kyth-exe-handler.desktop",
        mime_type: "application/x-msdos-program",
    },
    MimeDefault {
        desktop_file: "kyth-exe-handler.desktop",
        mime_type: "application/x-dosexec",
    },
    MimeDefault {
        desktop_file: "kyth-exe-handler.desktop",
        mime_type: "application/x-msi",
    },
    MimeDefault {
        desktop_file: "kyth-exe-handler.desktop",
        mime_type: "application/x-msdownload",
    },
    MimeDefault {
        desktop_file: "kyth-exe-handler.desktop",
        mime_type: "application/vnd.microsoft.portable-executable",
    },
    MimeDefault {
        desktop_file: "kyth-exe-handler.desktop",
        mime_type: "application/x-rpm",
    },
    MimeDefault {
        desktop_file: "kyth-exe-handler.desktop",
        mime_type: "application/x-redhat-package-manager",
    },
    MimeDefault {
        desktop_file: "com.brave.Browser.desktop",
        mime_type: "x-scheme-handler/http",
    },
    MimeDefault {
        desktop_file: "com.brave.Browser.desktop",
        mime_type: "x-scheme-handler/https",
    },
    MimeDefault {
        desktop_file: "com.getmailspring.Mailspring.desktop",
        mime_type: "x-scheme-handler/mailto",
    },
    MimeDefault {
        desktop_file: "org.kde.dolphin.desktop",
        mime_type: "inode/directory",
    },
];

pub fn defaults_for(desktop_file: &str) -> Vec<&'static str> {
    MIME_DEFAULTS
        .iter()
        .filter(|entry| entry.desktop_file == desktop_file)
        .map(|entry| entry.mime_type)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_matches_desktop_polish_shape() {
        assert_eq!(USER_FOLDERS.len(), 10);
        assert_eq!(FOLDER_METADATA.len(), 3);
        assert_eq!(defaults_for("kyth-exe-handler.desktop").len(), 8);
        assert!(MIME_DEFAULTS
            .iter()
            .any(|entry| entry.mime_type == "inode/directory"));
    }
}
