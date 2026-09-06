# shellcheck shell=bash
# ── KythOS Hub launcher and native installer packaging ───────────────────────
# /ctx/kyth-welcome is a symlink to ../src/... inside
# build_files. When build_files is bind-mounted as /ctx, those symlinks dangle
# and BuildKit overlay mounts do not reliably hide them. Use /src fallback.
_resolve_ctx_src() {
    local name="$1"
    # Check /src first (explicit mount of repo src), then /ctx if it is a real dir
    if [[ -d "/src/${name}" ]]; then
        echo "/src/${name}"
        return
    fi
    if [[ -d "/ctx/src/${name}" ]]; then
        echo "/ctx/src/${name}"
        return
    fi
    # /ctx/${name} may be a dangling symlink; check if it is a real directory with content
    if [[ -d "/ctx/${name}" ]] && [[ ! -L "/ctx/${name}" || -e "/ctx/${name}/pyproject.toml" ]]; then
        # If it's a symlink, verify it resolves
        if [[ -L "/ctx/${name}" ]]; then
            if [[ -f "/ctx/${name}/pyproject.toml" ]]; then
                echo "/ctx/${name}"
                return
            fi
        else
            echo "/ctx/${name}"
            return
        fi
    fi
    # Fallback to /ctx even if dangling - will fail with clear error
    echo "/ctx/${name}"
}
_welcome_src="$(_resolve_ctx_src kyth-welcome)"
echo "branding: welcome_src=${_welcome_src} (ctx_welcome_exists=$(test -d /ctx/kyth-welcome && echo yes || echo no) src_exists=$(test -d /src/kyth-welcome && echo yes || echo no) ctx_islink=$(test -L /ctx/kyth-welcome && echo yes || echo no))" >&2
ls -ld "/ctx/kyth-welcome" "/src/kyth-welcome" 2>&1 | head -n 5 >&2 || true
install -m 0755 "${_welcome_src}/kyth-welcome-launch" /usr/bin/kyth-welcome-launch
install -m 0644 "${_welcome_src}/kyth-welcome.desktop" \
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

unset _welcome_src
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
# _welcome_src was unset above; re-resolve for desktop seeding
if [[ -d "/ctx/kyth-welcome" && -f "/ctx/kyth-welcome/pyproject.toml" ]]; then
    _welcome_src="/ctx/kyth-welcome"
elif [[ -d "/src/kyth-welcome" ]]; then
    _welcome_src="/src/kyth-welcome"
else
    _welcome_src="/ctx/kyth-welcome"
fi
install -m 0755 "${_welcome_src}/kyth-welcome.desktop" \
	/etc/skel/Desktop/kyth-welcome.desktop
unset _welcome_src

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
