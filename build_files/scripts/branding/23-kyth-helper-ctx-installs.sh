# shellcheck shell=bash
# ── KythOS Hub launcher and native installer packaging ───────────────────────
# The supported Hub launcher is compiled Rust. The desktop metadata is kept
# next to the React/Tauri source so packaging has no Python Hub dependency.
_hub_data_src="/src/kyth-hub-web/src/data"
install -m 0644 "${_hub_data_src}/kyth-welcome.desktop" \
	/usr/share/applications/kyth-welcome.desktop

# Hub search in KRunner — generated from the same route manifest imported by
# the React frontend. The generator is a build-time Rust binary and has no
# dependency on the retired Python/Qt Hub package.
/usr/bin/kyth-hub-desktop-entries \
	/src/kyth-hub-web/src/data/hubRoutes.json \
	/usr/share/applications/kyth-hub
# Keep the same route manifest available to installed-image acceptance. The
# frontend remains the runtime authority; this copy only lets the guest derive
# the complete --page matrix without duplicating page names in the harness.
install -Dm0644 /src/kyth-hub-web/src/data/hubRoutes.json \
	/usr/share/kyth/hubRoutes.json

unset _hub_data_src
write_config /usr/share/applications/kyth-app-store.desktop <<'APPSTOREEOF'
[Desktop Entry]
Type=Application
Name=KythOS App Store
GenericName=App Store
Comment=Find and install trusted apps on KythOS
Exec=/usr/bin/kyth-welcome-launch --page "App Store"
Icon=plasmadiscover
Terminal=false
Categories=Settings;PackageManager;
Keywords=apps;store;software;flatpak;install;remove;
StartupNotify=true
StartupWMClass=kyth-welcome
APPSTOREEOF
# The native Rust helper is copied into the base stage by Dockerfile. Keep the
# stable /usr/libexec path used by kyth-privileged, but do not install the
# legacy Python wrapper from the build context.
install -m 0755 /usr/bin/kyth-network-share /usr/libexec/kyth-network-share
install -m 0755 /ctx/kyth-set-sleep-mode /usr/libexec/kyth-set-sleep-mode
install -m 0755 /ctx/kyth-retry-hardware-setup /usr/libexec/kyth-retry-hardware-setup

# Place Kyth Hub on the desktop for all new users. The executable bit is
# required so KDE Plasma 6 treats it as trusted without prompting the user.
mkdir -p /etc/skel/Desktop
install -m 0755 "/src/kyth-hub-web/src/data/kyth-welcome.desktop" \
	/etc/skel/Desktop/kyth-welcome.desktop

# Recycle Bin on the desktop keeps deletion recovery visible. Type=Link entries
# open in Dolphin and need no executable/trust bit. Kept in /usr/share/kyth so
# the user-polish pass can seed existing accounts too.
write_config /usr/share/kyth/kyth-recycle-bin.desktop <<'TRASHEOF'
[Desktop Entry]
Type=Link
URL=trash:/
Name=Recycle Bin
GenericName=Trash
Icon=user-trash
TRASHEOF
install -m 0644 /usr/share/kyth/kyth-recycle-bin.desktop \
	/etc/skel/Desktop/kyth-recycle-bin.desktop
