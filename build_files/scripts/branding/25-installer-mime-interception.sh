# shellcheck shell=bash
# ── Downloaded installer MIME interception ───────────────────────────────────
# When a Windows user double-clicks a .exe/.msi or downloaded .rpm in Dolphin,
# validate it, explain known limits, then prepare and launch it in Bottles. Keep
# native KythOS paths visible instead of teaching the wrong mutable-system model.
# The handler is registered as the system-wide default for these installer MIME
# types; users can override per-app via Dolphin's "Open With" dialog.
# The native launcher forwards the MIME file path into the existing
# Rust/Tauri Hub dialog; it is installed by the image builder.
install -m 0644 /ctx/kyth-exe-handler.desktop \
	/usr/share/applications/kyth-exe-handler.desktop
mkdir -p /usr/share/kyth
install -m 0644 /ctx/exe-handler-apps.json /usr/share/kyth/exe-handler-apps.json

# Keep expert tools installed without crowding a new user's app launcher.
# System Hub still exposes the relevant guided actions, and every binary remains
# available from a terminal. /usr/local/share takes precedence over RPM entries.
mkdir -p "$(realpath -m /usr/local)/share/applications"
for _hidden_desktop in \
	com.gerbilsoft.rom-properties.rp-config.desktop \
	htop.desktop \
	jstest-gtk.desktop \
	mpv.desktop \
	nvim.desktop \
	nvtop.desktop \
	org.corectrl.CoreCtrl.desktop \
	org.kde.drkonqi.coredump.gui.desktop \
	org.kde.kdebugsettings.desktop \
	org.kde.kjournaldbrowser.desktop \
	remote-viewer.desktop; do
	write_config "/usr/local/share/applications/${_hidden_desktop}" <<'HIDDENDESKTOPEOF'
[Desktop Entry]
Type=Application
Name=Hidden expert tool
Hidden=true
HIDDENDESKTOPEOF
done
unset _hidden_desktop

# Register as system-wide default for common installer MIME types.
# /etc/xdg/mimeapps.list is the XDG-standard location for system defaults;
# it is read before per-user ~/.config/mimeapps.list so new users get it
# automatically, and existing users can still override per-app.
mkdir -p /etc/xdg
cat >>/etc/xdg/mimeapps.list <<'MIMEAPPSEOF'
[Default Applications]
application/pdf=org.kde.okular.desktop;okularApplication_pdf.desktop;
application/epub+zip=org.kde.okular.desktop;okularApplication_epub.desktop;
image/jpeg=org.kde.gwenview.desktop;gwenview.desktop;
image/png=org.kde.gwenview.desktop;gwenview.desktop;
image/gif=org.kde.gwenview.desktop;gwenview.desktop;
image/webp=org.kde.gwenview.desktop;gwenview.desktop;
video/mp4=org.videolan.VLC.desktop;mpv.desktop;org.kde.haruna.desktop;
video/x-matroska=org.videolan.VLC.desktop;mpv.desktop;org.kde.haruna.desktop;
video/x-msvideo=org.videolan.VLC.desktop;mpv.desktop;org.kde.haruna.desktop;
audio/mpeg=org.videolan.VLC.desktop;mpv.desktop;org.kde.elisa.desktop;
audio/flac=org.videolan.VLC.desktop;mpv.desktop;org.kde.elisa.desktop;
text/plain=org.kde.kwrite.desktop;org.kde.kate.desktop;
text/markdown=org.kde.kwrite.desktop;org.kde.kate.desktop;
application/zip=org.kde.ark.desktop;ark.desktop;
application/x-7z-compressed=org.kde.ark.desktop;ark.desktop;
application/x-rar=org.kde.ark.desktop;ark.desktop;
application/x-tar=org.kde.ark.desktop;ark.desktop;
application/x-ms-dos-executable=kyth-exe-handler.desktop
application/x-msdos-program=kyth-exe-handler.desktop
application/x-dosexec=kyth-exe-handler.desktop
application/x-msi=kyth-exe-handler.desktop
application/x-msdownload=kyth-exe-handler.desktop
application/vnd.microsoft.portable-executable=kyth-exe-handler.desktop
application/x-rpm=kyth-exe-handler.desktop
application/x-redhat-package-manager=kyth-exe-handler.desktop
x-scheme-handler/http=com.brave.Browser.desktop;chromium-browser.desktop
x-scheme-handler/https=com.brave.Browser.desktop;chromium-browser.desktop
x-scheme-handler/mailto=com.getmailspring.Mailspring.desktop
inode/directory=org.kde.dolphin.desktop
MIMEAPPSEOF

# Rebuild the MIME/desktop database so KDE picks up the new handler immediately.
update-desktop-database /usr/share/applications/ 2>/dev/null || true

# Add nearby sharing to Dolphin's file context menu. KDE Connect handles
# discovery and transfer; the helper prompts when multiple paired devices are
# reachable.
mkdir -p /usr/share/kio/servicemenus
install -m 0644 /ctx/kyth-nearby-share.desktop \
	/usr/share/kio/servicemenus/kyth-nearby-share.desktop

# Surface a plain "Open Terminal Here" action in Dolphin. This is a small
# everyday comfort affordance for support notes, development, and modding.
write_config /usr/share/kio/servicemenus/kyth-open-terminal-here.desktop <<'TERMHEREDESKTOPEOF'
[Desktop Entry]
Type=Service
MimeType=inode/directory;
Actions=kythOpenTerminalHere;
X-KDE-Priority=TopLevel

[Desktop Action kythOpenTerminalHere]
Name=Open Terminal Here
Icon=utilities-terminal
Exec=konsole --workdir %f
TERMHEREDESKTOPEOF
