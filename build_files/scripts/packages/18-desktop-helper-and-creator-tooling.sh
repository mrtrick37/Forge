#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# shellcheck source=../lib/packages-helpers.sh disable=SC1091
source "../lib/packages-helpers.sh"

# ── Desktop helper, Plymouth, mutable-workspace, and creator tooling ─────────
# Keep required desktop helper packages in one transaction. Optional niceties
# use a batched fast path with individual fallback so a transient RPM/scriptlet
# issue in a font or hardware utility does not block the image.
dnf5 install -y --skip-unavailable \
	python3-pip \
	python3-setuptools \
	python3-defusedxml \
	curl \
	qt6-qtwayland \
	xdg-desktop-portal \
	xdg-desktop-portal-kde \
	xdg-desktop-portal-gtk \
	webkit2gtk4.1 \
	gtk3 \
	libsoup3 \
	plymouth \
	plymouth-plugin-script \
	librsvg2-tools \
	distrobox \
	unzip \
	git \
	spice-vdagent \
	virt-viewer \
	kscreen \
	neovim \
	zsh \
	openconnect \
	vpnc \
	kde-connect \
	plasma-browser-integration \
	zoxide \
	starship \
	eza \
	bat \
	git-delta \
	direnv \
	jq \
	yq

# gum (TUI menu builder used by interactive ujust recipes) lives in Terra, whose
# repo file is written and then disabled by packages/12; enable it for just this
# transaction. Kept as a real host binary so recipes don't spin up a container
# per menu. --skip-unavailable keeps the build resilient if Terra is unreachable.
dnf5 install -y --skip-unavailable --enablerepo=terra gum

# Generic Distrobox wrapper — delegates to kyth-ai-dev container dynamically
install -Dm 0755 /dev/stdin /usr/libexec/kyth-distrobox-wrapper <<'WRAPPEREOF'
#!/usr/bin/env bash
set -euo pipefail
tool="$(basename "$0")"

# Mapping human-readable descriptions of tools
declare -A descriptions=(
	[shellcheck]="shellcheck"
	[shfmt]="shfmt"
	[gh]="GitHub CLI"
	[hx]="Helix editor"
	[zellij]="Zellij"
	[fastfetch]="fastfetch system summary"
	[evtest]="evtest input event monitor"
	[sensors]="lm_sensors hardware monitor"
	[i2cget]="i2cget I2C utility"
	[i2cset]="i2cset I2C utility"
	[i2cdetect]="i2cdetect I2C utility"
	[v4l2-ctl]="v4l2-ctl Video4Linux utility"
	[hyperfine]="hyperfine benchmarking tool"
	[tmux]="tmux terminal multiplexer"
	[rclone]="rclone cloud storage sync"
	[flatpak-builder]="flatpak-builder package creator"
	[pipx]="pipx Python application runner"
	[uv]="uv fast Python package installer"
	[7z]="7z archive extractor"
	[7za]="7za archive extractor"
	[cabextract]="cabextract archive utility"
	[readpst]="readpst PST converter"
)
desc="${descriptions[$tool]:-$tool}"

if [[ -x "${HOME}/.local/bin/${tool}" ]]; then
	exec "${HOME}/.local/bin/${tool}" "$@"
fi

box="${KYTH_AI_DEV_BOX:-kyth-ai-dev}"
if command -v distrobox >/dev/null 2>&1 && distrobox list --no-color 2>/dev/null | awk '{print $3}' | grep -qx "${box}"; then
	# Only delegate if the binary exists inside the container — otherwise return
	# a clean 127 without crun's stack trace so shell rc guards stay silent.
	if distrobox enter "${box}" -- sh -c "command -v ${tool} >/dev/null 2>&1" 2>/dev/null; then
		exec distrobox enter "${box}" -- "${tool}" "$@"
	else
		echo "${desc} not found in container ${box} (run: kyth-ai-dev setup)" >&2
		exit 127
	fi
fi

echo "${desc} is managed in the KythOS AI Developer container (${box})."
echo "Initializing ${box} environment..."
kyth-ai-dev setup
exec distrobox enter "${box}" -- "${tool}" "$@"
WRAPPEREOF

# Create host symlinks to the generic distrobox wrapper.
# Interactive shell / prompt / pager tooling is host-native (dnf installed above)
# so shell init, aliases (eza→ls, bat→cat), the git pager (delta), direnv hooks,
# jq/yq pipes and gum menus never require the container. Only heavyweight or
# occasionally-used dev tools stay containerized in the wrapper loop below.
for tool in shellcheck shfmt gh hx zellij fastfetch evtest sensors i2cget i2cset i2cdetect v4l2-ctl hyperfine tmux rclone flatpak-builder pipx uv 7z 7za cabextract readpst; do
	ln -sf /usr/libexec/kyth-distrobox-wrapper "/usr/bin/${tool}"
done

# Atomic systems map /usr/local to the root-owned /var/usrlocal. npm's
# system default therefore makes `npm install -g` fail for desktop users.
# npmrc supports environment expansion, and ~/.local/bin is already on the
# Fedora user PATH, so global CLI tools belong in the user's home directory.
cat >/etc/npmrc <<'EOF'
prefix=${HOME}/.local
EOF

# Fedora has historically moved between versioned and unversioned Python tool
# entrypoints. Keep the familiar `pip` command present on PATH for users while
# leaving the RPM-owned pip3 binary untouched.
if ! command -v pip >/dev/null 2>&1; then
	pip3_path="$(command -v pip3 || true)"
	if [[ -z "${pip3_path}" ]]; then
		echo "ERROR: python3-pip installed without pip3 on PATH." >&2
		exit 1
	fi
	mkdir -p "$(realpath -m /usr/local)/bin"
	ln -s "${pip3_path}" /usr/local/bin/pip
fi
pip --version

optional_desktop_packages=(
	jetbrains-mono-fonts
	liberation-fonts-all
	inter-fonts
	papirus-icon-theme
	# Emoji rendering — without this, emoji in browsers and terminals render as
	# empty boxes on systems that only have the liberation/inter font set.
	google-noto-emoji-fonts
	# Modern CLI tools (heavy ones like fish, btop, ddcutil are provided via
	# kyth-ai-dev container or Flatpaks to keep base OS lean).
	fd-find
	ripgrep
	fzf
	# zsh enhancements — sourced automatically by the /etc/skel/.zshrc below.
	zsh-autosuggestions
	zsh-syntax-highlighting
	# ydotool — Wayland-compatible xdotool; required for Wayland automation scripts.
	ydotool
	# iio-sensor-proxy — exposes orientation sensors (accelerometer) over D-Bus
	# for auto-rotation on convertibles and handhelds.
	iio-sensor-proxy
)

install_available_optional_packages desktop "${optional_desktop_packages[@]}"
# spice-vdagentd is socket/udev-activated — no systemctl enable needed.
# kde-connect: Phone Link equivalent for Android — pairs over LAN/Bluetooth.
# plasma-browser-integration: native host for browser media controls, download
#   progress, and desktop integration once the browser extension is enabled.
# cups-browsed is intentionally NOT installed: it is the legacy LAN printer
#   auto-discovery daemon (2024 CUPS RCE vector on UDP 631) and is purged in
#   packages/17-desktop-package-cleanup.sh. Driverless printing still works via
#   cups + Avahi/mDNS (IPP Everywhere), which remain installed and enabled.
# liberation-fonts-all: metric-compatible substitutes for Arial/Times/Courier.
#   mscore-fonts-all (RPM Fusion) was removed — its %post downloads from
#   SourceForge at install time, which is unreliable in CI builds.
# openrgb: RGB peripheral control installed by default; udev rules grant LED device
#   access to the logged-in user. Autostarted at login via XDG autostart entry.
# libwacom/libwacom-data: tablet pressure-curve database used by KWin/libinput on
#   Wayland for Wacom and Wacom-compatible tablets. Without this, pressure sensitivity
#   maps incorrectly and drawing feels like a binary on/off signal.
# hplip: HP printer driver stack. Auto-detects most HP USB/network printers without
#   manual CUPS configuration.
# input-remapper is already installed in the gaming packages block
# (packages/06-gaming-core.sh).
# webkit2gtk4.1/gtk3/libsoup3: runtime deps for /usr/bin/kyth-hub-shell (the
# React+Tauri Kyth Hub rewrite, src/kyth-hub-web — built in the hub-web-builder
# stage of the top-level Dockerfile). Installed unconditionally since the
# binary itself ships on every channel; kyth-welcome-launch requires this binary
# and reports a clear failure if packaging is incomplete. dbus is not listed
# here — it's already a hard dependency of the base Plasma desktop.
