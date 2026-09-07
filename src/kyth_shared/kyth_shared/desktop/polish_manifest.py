"""Declarative polish manifest — single source for folders/MIME/autostart.

`user_polish.py` was a 565-line god module that both declared
`USER_FOLDERS`/`MIME_DEFAULTS` constants and performed `os.makedirs`/`kwriteconfig`
side effects. Tests had to import the whole module to inspect a tuple.

This file holds the pure data so `user_polish.py` and Hub welcome checks can
import it without pulling in `subprocess`/`ET`/`glob`.
"""

from __future__ import annotations

VERSION = "v13"
PLACES_VERSION = "v1"
AUTOSTART_VERSION = "v1"

USER_FOLDERS = (
    "Desktop", "Documents", "Downloads", "Games", "Music", "Pictures",
    "Public", "Screenshots", "Templates", "Videos",
)

FOLDER_METADATA = {
    "Games/.directory": "[Desktop Entry]\nIcon=applications-games\nName=Games\n",
    "Screenshots/.directory": "[Desktop Entry]\nIcon=folder-pictures\nName=Screenshots\n",
    "Templates/Plain Text.txt": "",
}

MIME_DEFAULTS = (
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
    ("kyth-exe-handler.desktop", "application/x-ms-dos-executable"),
    ("kyth-exe-handler.desktop", "application/x-msdos-program"),
    ("kyth-exe-handler.desktop", "application/x-dosexec"),
    ("kyth-exe-handler.desktop", "application/x-msi"),
    ("kyth-exe-handler.desktop", "application/x-msdownload"),
    ("kyth-exe-handler.desktop", "application/vnd.microsoft.portable-executable"),
    ("kyth-exe-handler.desktop", "application/x-rpm"),
    ("kyth-exe-handler.desktop", "application/x-redhat-package-manager"),
    ("com.brave.Browser.desktop", "x-scheme-handler/http"),
    ("com.brave.Browser.desktop", "x-scheme-handler/https"),
    ("com.getmailspring.Mailspring.desktop", "x-scheme-handler/mailto"),
    ("org.kde.dolphin.desktop", "inode/directory"),
)
