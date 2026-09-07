# shellcheck shell=bash
# ── Web app launcher grouping ─────────────────────────────────────────────────
# Chromium-family browsers create PWA launchers without Categories=. KDE cannot
# classify those launchers and drops them into Lost and Found. Add a custom
# category only when the browser did not provide one, preserving any category a
# user assigns later with the menu editor.
# kyth-web-app-categorize is the native Rust binary copied from the
# hub-web-builder stage; no Python launcher remains in the source tree.

mkdir -p /etc/systemd/user/default.target.wants
write_config /etc/systemd/user/kyth-web-app-categorize.service <<'WEBAPPSERVICEEOF'
[Unit]
Description=Place browser-installed web apps in the Web Apps launcher folder

[Service]
Type=oneshot
ExecStart=/usr/bin/kyth-web-app-categorize
WEBAPPSERVICEEOF

write_config /etc/systemd/user/kyth-web-app-categorize.path <<'WEBAPPPATHEOF'
[Unit]
Description=Watch for browser-installed web app launchers

[Path]
PathChanged=%h/.local/share/applications
Unit=kyth-web-app-categorize.service

[Install]
WantedBy=default.target
WEBAPPPATHEOF
ln -sf /etc/systemd/user/kyth-web-app-categorize.path \
	/etc/systemd/user/default.target.wants/kyth-web-app-categorize.path

mkdir -p \
	/etc/skel/Desktop \
	/etc/skel/Documents \
	/etc/skel/Downloads \
	/etc/skel/Games \
	/etc/skel/Music \
	/etc/skel/Pictures \
	/etc/skel/Public \
	/etc/skel/Templates \
	/etc/skel/Videos

write_config /etc/skel/Games/.directory <<'GAMESDIREEOF'
[Desktop Entry]
Icon=applications-games
Name=Games
GAMESDIREEOF

write_config /etc/skel/.config/user-dirs.dirs <<'USERDIRSEOF'
XDG_DESKTOP_DIR="$HOME/Desktop"
XDG_DOWNLOAD_DIR="$HOME/Downloads"
XDG_TEMPLATES_DIR="$HOME/Templates"
XDG_PUBLICSHARE_DIR="$HOME/Public"
XDG_DOCUMENTS_DIR="$HOME/Documents"
XDG_MUSIC_DIR="$HOME/Music"
XDG_PICTURES_DIR="$HOME/Pictures"
XDG_VIDEOS_DIR="$HOME/Videos"
USERDIRSEOF

write_config /etc/skel/.config/plasma-org.kde.plasma.desktop-appletsrc <<'PLASMADESKTOPEOF'
[Containments][1]
wallpaperplugin=org.kde.image

[Containments][1][Wallpaper][org.kde.image][General]
Image=/usr/share/wallpapers/kyth/contents/images/1920x1080.svg
PLASMADESKTOPEOF
