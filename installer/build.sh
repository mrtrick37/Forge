#!/usr/bin/bash
# Based directly on Bazzite's installer/build.sh
# Ref: https://github.com/ublue-os/bazzite/blob/main/installer/build.sh

set -exo pipefail

# shellcheck source=build_files/scripts/lib/plymouth-initrd-checks.sh disable=SC1091
source /src/build_files/scripts/lib/plymouth-initrd-checks.sh

# Tools required by the live installer's NTFS shrink-and-install path.
if command -v dnf5 >/dev/null 2>&1; then
	dnf5 install -y ntfs-3g parted btrfs-progs gdisk
	dnf5 clean all
else
	dnf install -y ntfs-3g parted btrfs-progs gdisk
	dnf clean all
fi

SOURCE_TAG=${SOURCE_TAG:?}
BASE_IMAGE=${BASE_IMAGE:?}
INSTALL_SOURCE_IMAGE=${INSTALL_SOURCE_IMAGE:-${BASE_IMAGE}}

# bwrap tries to write /proc/sys/user/max_user_namespaces which is mounted as ro
mount -o remount,rw /proc/sys

# ── Native installer runtime ──────────────────────────────────────────────────
# The Containerfile supplies the Rust shell, daemon, and typed execution
# helper. No Python installer package is installed into the live image.
install -Dm755 /src/build_files/kyth-launch-installer /usr/bin/kyth-launch-installer
install -Dm644 /src/build_files/kyth-installerd.service /usr/lib/systemd/system/kyth-installerd.service
install -Dm755 /src/build_files/scripts/plymouth-branding-guard.sh \
	/usr/libexec/kyth-plymouth-branding-guard

cat >/usr/share/applications/kyth-install.desktop <<'EOF'
[Desktop Entry]
Name=Install KythOS
Comment=Install KythOS to this computer
Exec=/usr/bin/kyth-launch-installer
Icon=kyth
Terminal=false
Type=Application
Categories=System;
EOF

# Bundle the exact image used for this live payload. The default Fedora install
# can then complete without a network connection while retaining the public
# registry reference for future bootc updates. Optional kernel variants remain
# registry-backed because they are separate images.
mkdir -p /usr/share/kyth/image
skopeo_source_args=()
source_imgref="${INSTALL_SOURCE_IMAGE}"
case "${source_imgref}" in
	containers-storage:*|oci:*|dir:*|ostree:*)
		;;
	docker://*)
		source_imgref="docker://${source_imgref#docker://}"
		;;
	*)
		source_imgref="docker://${source_imgref}"
		;;
esac
case "${source_imgref#docker://}" in
	localhost:*|127.0.0.1:*|\[::1\]:*)
		# Local test registries are intentionally HTTP-only. Keep normal
		# registry pulls TLS-verified; relax verification only for loopback.
		skopeo_source_args+=(--src-tls-verify=false)
		;;
esac
skopeo copy --retry-times 3 \
	"${skopeo_source_args[@]}" \
	"${source_imgref}" \
	"oci:/usr/share/kyth/image:latest"
embedded_digest="$(skopeo inspect --format '{{.Digest}}' 'oci:/usr/share/kyth/image:latest')"
case "${embedded_digest}" in
	sha256:[0-9a-f][0-9a-f]*) ;;
	*)
		echo "ERROR: embedded installer image has no valid sha256 digest: ${embedded_digest}" >&2
		exit 1
		;;
esac
expected_digest="${INSTALL_SOURCE_IMAGE##*@}"
release_digest="${embedded_digest}"
[[ "${expected_digest}" == sha256:* ]] && release_digest="${expected_digest}"
target_image="ghcr.io/kyth-os/kyth:${SOURCE_TAG}"
printf 'KYTH_SOURCE_IMAGE=oci:/usr/share/kyth/image:latest\nKYTH_TARGET_IMAGE=%s\nKYTH_SOURCE_DIGEST=%s\nKYTH_INSTALLER_SOCKET=/run/kyth-installer/api.sock\nKYTH_INSTALLER_SOCKET_GROUP=liveuser\nKYTH_INSTALLER_TOKEN_FILE=/run/kyth-installer/session-token\n' \
	"${target_image}" "${embedded_digest}" >/etc/kyth-installer.env
printf '{"schema_version":1,"digest":"%s","release_digest":"%s","target_image":"%s","source_image":"%s"}\n' \
	"${embedded_digest}" "${release_digest}" "${target_image}" "${INSTALL_SOURCE_IMAGE}" \
	>/usr/share/kyth/image-source.json

# Install live-only packages in one transaction so dependency solving and
# repository metadata work happen once. Browsers from the installed image are
# intentionally deferred to Flatpak first-boot setup.
dnf install -y \
	webkit2gtk4.1 \
	gtk3 \
	dracut-live \
	grub2-efi-x64-cdboot \
	livesys-scripts

# ── Live desktop: installer shortcut + software rendering (via /etc/skel) ────
# The installed image seeds System Hub for a user's first login. The live
# session should open the installer instead and keep the desktop uncluttered.
rm -f \
	/etc/skel/Desktop/kyth-welcome.desktop \
	/etc/skel/Desktop/system-hub.desktop \
	/etc/skel/.config/autostart/kyth-welcome.desktop
mkdir -p /etc/skel/Desktop /etc/skel/.config/autostart
cat >/etc/skel/Desktop/install-kyth.desktop <<'EOF'
[Desktop Entry]
Name=Install KythOS
Comment=Install KythOS to this computer
Exec=/usr/bin/kyth-launch-installer
Icon=kyth
Terminal=false
Type=Application
Categories=System;
EOF
chmod +x /etc/skel/Desktop/install-kyth.desktop

# The live user is ephemeral, so do not interrupt Wi-Fi setup with KWallet's
# first-use encryption wizard. Installed users keep the normal encrypted wallet.
cat >/etc/skel/.config/kwalletrc <<'EOF'
[Wallet]
Enabled=false
First Use=false
EOF

# Plasma normally starts the PAM wallet bridge during login. The live account
# has no persistent secrets, so keep that bridge out of its autologin session.
for pam_file in /etc/pam.d/sddm-autologin /usr/lib/pam.d/plasmalogin-autologin; do
	[ -f "${pam_file}" ] && sed -i '/pam_kwallet/d' "${pam_file}"
done
mkdir -p /etc/xdg/autostart /etc/systemd/user
cat >/etc/xdg/autostart/pam_kwallet_init.desktop <<'EOF'
[Desktop Entry]
Type=Application
Hidden=true
EOF
ln -sf /dev/null /etc/systemd/user/plasma-kwallet-pam.service

install -Dm755 /src/build_files/scripts/kyth-live-owe-wifi-setup.sh \
	/usr/libexec/kyth-live-owe-wifi-setup

cat >/etc/systemd/system/kyth-live-owe-wifi.service <<'EOF'
[Unit]
Description=Seed live ISO OWE Wi-Fi profiles
ConditionKernelCommandLine=kyth.live=1
Wants=NetworkManager.service
After=NetworkManager.service network-pre.target
Before=network.target network-online.target

[Service]
Type=oneshot
TimeoutStartSec=120
RemainAfterExit=yes
ExecStart=/usr/libexec/kyth-live-owe-wifi-setup

[Install]
WantedBy=network.target
EOF
systemctl enable kyth-live-owe-wifi.service

cat >/etc/skel/.config/autostart/kyth-installer.desktop <<'EOF'
[Desktop Entry]
Type=Application
Name=Install KythOS
Exec=/usr/bin/kyth-launch-installer
X-KDE-autostart-after=panel
Hidden=false
NoDisplay=true
EOF

mkdir -p /etc/skel/.config/plasma-workspace/env
cat >/etc/skel/.config/plasma-workspace/env/live.sh <<'EOF'
#!/bin/bash
export LIBGL_ALWAYS_SOFTWARE=1
export GALLIUM_DRIVER=llvmpipe
export MESA_LOADER_DRIVER_OVERRIDE=llvmpipe
export QT_QUICK_BACKEND=software
export KWIN_COMPOSE=Q
EOF
chmod +x /etc/skel/.config/plasma-workspace/env/live.sh

# livesys-session-extra: runs after livesys-kde sets up the KDE session
mkdir -p /var/lib/livesys
cat >/var/lib/livesys/livesys-session-extra <<'EOF'
#!/bin/sh
rm -f \
    /home/liveuser/Desktop/liveinst.desktop \
    /home/liveuser/Desktop/kyth-welcome.desktop \
    /home/liveuser/Desktop/system-hub.desktop \
    /home/liveuser/.config/autostart/kyth-welcome.desktop \
    2>/dev/null || true
mkdir -p /home/liveuser/.config
cat > /home/liveuser/.config/kwalletrc <<'WALLETRC'
[Wallet]
Enabled=false
First Use=false
WALLETRC
cat > /home/liveuser/.config/kscreenlockerrc <<'SCREENLOCKEOF'
[Daemon]
Autolock=false
LockOnResume=false
SCREENLOCKEOF
chown liveuser:liveuser \
    /home/liveuser/.config/kwalletrc \
    /home/liveuser/.config/kscreenlockerrc
[ -f /home/liveuser/Desktop/install-kyth.desktop ] && \
    chmod +x /home/liveuser/Desktop/install-kyth.desktop
EOF
chmod +x /var/lib/livesys/livesys-session-extra

# ── dracut-live + initramfs ───────────────────────────────────────────────────
# The live ISO must boot the signed Fedora kernel so it works under Secure Boot.
# CachyOS is opt-in: chosen during installation or from System Hub on the
# installed system (a bootc switch to the -cachy image), never in the live
# environment. So always pick the non-CachyOS (Fedora) kernel here, and refuse
# to build a live ISO from a CachyOS-only image rather than silently producing an
# unsignable one.
mapfile -t kernels < <(find /usr/lib/modules -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort -V)
kernel=
for candidate in "${kernels[@]}"; do
	if [[ "${candidate}" != *cachyos* ]]; then
		kernel="${candidate}"
	fi
done
if [[ -z "${kernel}" ]]; then
	echo "ERROR: no signed Fedora kernel in /usr/lib/modules — the live ISO must use the Fedora kernel for Secure Boot. Build the ISO from the Fedora image variant, not the -cachy variant." >&2
	exit 1
fi
/usr/libexec/kyth-plymouth-branding-guard
plymouth-set-default-theme kyth
mkdir -p /etc/plymouth /usr/share/plymouth
cat >/etc/plymouth/plymouthd.conf <<'EOF'
[Daemon]
Theme=kyth
ShowDelay=0
DeviceTimeout=8
UseFirmwareBackground=false
EOF
install -m 0644 /etc/plymouth/plymouthd.conf /usr/share/plymouth/plymouthd.conf
cat >/usr/share/plymouth/plymouthd.defaults <<'EOF'
[Daemon]
Theme=kyth
ShowDelay=0
DeviceTimeout=8
UseFirmwareBackground=false
EOF
kyth_plymouth_include_root="$(mktemp -d)"
mkdir -p \
	"${kyth_plymouth_include_root}/etc/plymouth" \
	"${kyth_plymouth_include_root}/usr/share/plymouth" \
	"${kyth_plymouth_include_root}/usr/share/pixmaps"
install -m 0644 /etc/plymouth/plymouthd.conf \
	"${kyth_plymouth_include_root}/etc/plymouth/plymouthd.conf"
install -m 0644 /usr/share/plymouth/plymouthd.defaults \
	"${kyth_plymouth_include_root}/usr/share/plymouth/plymouthd.defaults"
install -m 0644 /usr/share/kyth/branding/transparent-watermark.png \
	"${kyth_plymouth_include_root}/usr/share/pixmaps/system-logo-white.png"
DRACUT_NO_XATTR=1 dracut -v --force --zstd --no-hostonly \
	--add "kyth-plymouth plymouth dmsquash-live dmsquash-live-autooverlay" \
	--include "${kyth_plymouth_include_root}" / \
	"/usr/lib/modules/${kernel}/initramfs.img" "${kernel}"
rm -rf "${kyth_plymouth_include_root}"

initrd_listing="$(mktemp)"
if command -v lsinitrd >/dev/null 2>&1; then
	initrd_img="/usr/lib/modules/${kernel}/initramfs.img"
	lsinitrd "${initrd_img}" >"${initrd_listing}"

	# Each entry is "pattern|message"; message is appended to the standard
	# "ERROR: live initramfs ..." prefix.
	listing_checks=(
		'usr/share/plymouth/themes/kyth/kyth.plymouth|does not contain KythOS Plymouth theme'
		'usr/share/plymouth/themes/kyth/kyth.script|does not contain KythOS Plymouth script'
		'usr/share/plymouth/themes/kyth/kyth-logo.png|does not contain KythOS Plymouth logo'
		'usr/share/plymouth/themes/default.plymouth|does not force the KythOS Plymouth default theme'
	)
	for entry in "${listing_checks[@]}"; do
		plymouth_require_pattern "${initrd_listing}" "${entry%%|*}" "live initramfs ${entry#*|}"
	done

	plymouth_require_match \
		<(lsinitrd -f /usr/share/pixmaps/system-logo-white.png "${initrd_img}") \
		/usr/share/kyth/branding/transparent-watermark.png \
		"live initramfs still contains distro Plymouth system logo"

	# Theme=kyth/ShowDelay=0/DeviceTimeout=8 must hold in both the Plymouth
	# defaults baked into the initramfs and the daemon config that overrides
	# them, so the same three patterns are checked against both sources.
	daemon_patterns=(
		'^Theme=kyth$|does not force Theme=kyth'
		'^ShowDelay=0$|does not draw immediately'
		'^DeviceTimeout=8$|is missing DeviceTimeout=8'
	)
	for entry in "${daemon_patterns[@]}"; do
		plymouth_require_pattern \
			<(lsinitrd -f /usr/share/plymouth/plymouthd.defaults "${initrd_img}") \
			"${entry%%|*}" "live initramfs Plymouth defaults ${entry#*|}"
	done

	initrd_extract="$(mktemp -d)"
	(cd "${initrd_extract}" && lsinitrd --unpack "${initrd_img}" etc/plymouth/plymouthd.conf)
	for entry in "${daemon_patterns[@]}"; do
		grep -q "${entry%%|*}" "${initrd_extract}/etc/plymouth/plymouthd.conf" || {
			echo "ERROR: live initramfs Plymouth daemon config ${entry#*|}" >&2
			rm -rf "${initrd_extract}"
			exit 1
		}
	done
	rm -rf "${initrd_extract}"

	plymouth_forbid_fallback_theme "${initrd_listing}" "Plymouth fallback theme leaked into live initramfs"
fi
rm -f "${initrd_listing}"

# ── livesys-scripts ───────────────────────────────────────────────────────────
sed -i 's/^livesys_session=.*/livesys_session="kde"/' /etc/sysconfig/livesys
systemctl enable livesys.service livesys-late.service

# ── Log straight into the live desktop ────────────────────────────────────────
# Live media boots on hardware that has never run this OS. Autologin follows
# DefaultSession (Plasma Wayland) plus live.sh llvmpipe/QPainter, including the
# ISO's nomodeset / Basic Graphics entry. Do not pin Session= here.
mkdir -p /etc/plasmalogin.conf.d
cat >/etc/plasmalogin.conf.d/20-kyth-live-autologin.conf <<'EOF'
[Autologin]
User=liveuser
Relogin=false
EOF

# ── Disable services inappropriate for live ───────────────────────────────────
for unit in \
	ostree-remount.service \
	rpm-ostree-countme.service rpm-ostree-countme.timer \
	bootc-fetch-apply-updates.service bootc-fetch-apply-updates.timer \
	systemd-firstboot.service systemd-oomd.service \
	kyth-default-flatpaks.service kyth-flathub-setup.service \
	kyth-proton-cachyos-update.service kyth-proton-cachyos-update.timer \
	kyth-hw-setup.service kyth-local-bin-migrate.service \
	kyth-duperemove.service kyth-duperemove.timer \
	kyth-enroll-mok.service sddm.service akmods.service \
	plasma-setup.service scxd.service \
	fwupd.service fwupd-refresh.service fwupd-refresh.timer; do
	systemctl disable "${unit}" 2>/dev/null || true
	ln -sf /dev/null "/etc/systemd/system/${unit}"
done

# The acceptance unit is intentionally present in the installed image. Keep
# its enablement explicit after the live-image service masking above so the
# QEMU qualification guest starts automatically at graphical.target.
install -m 0644 /src/build_files/kyth-vm-acceptance.service \
	/usr/lib/systemd/system/kyth-vm-acceptance.service
systemctl enable kyth-vm-acceptance.service

# Live-session presets may remove target wants links at first boot. Tie the
# acceptance guest directly to the live-session late setup service as well,
# so QEMU qualification remains runnable without relying on those links.
mkdir -p /etc/systemd/system/livesys-late.service.d
cat >/etc/systemd/system/livesys-late.service.d/kyth-vm-acceptance.conf <<'EOF'
[Unit]
Wants=kyth-vm-acceptance.service
EOF

# ── Larger /var/tmp for bootc install to-disk ─────────────────────────────────
rm -rf /var/tmp
mkdir /var/tmp
cat >/etc/systemd/system/var-tmp.mount <<'EOF'
[Unit]
Description=Larger tmpfs for /var/tmp on live system

[Mount]
What=tmpfs
Where=/var/tmp
Type=tmpfs
Options=size=50%,nr_inodes=1m

[Install]
WantedBy=local-fs.target
EOF
systemctl enable var-tmp.mount

# ── Scoped sudo for liveuser (least-privilege) ───────────────────────────────
# The packaged installer is the sole privileged entry point. It validates the
# installation request before invoking partitioning/bootc tools as its own root
# children. Do not grant those general-purpose tools separately: many of them
# are direct arbitrary-file-write or command-execution primitives.
#
# The empty sudoers argument string means only an argument-free graphical
# launch is passwordless. Headless/answer-file invocations require normal sudo
# authentication. Preserve only the display and image-selection environment
# needed by the native Rust shell and root-owned daemon.
install -Dm440 /dev/stdin /etc/sudoers.d/liveuser-live <<'EOF'
Defaults:liveuser env_keep += "DISPLAY WAYLAND_DISPLAY XAUTHORITY XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS XDG_SESSION_TYPE LIBGL_ALWAYS_SOFTWARE GALLIUM_DRIVER MESA_LOADER_DRIVER_OVERRIDE QT_QUICK_BACKEND"
liveuser ALL=(root) NOPASSWD: /usr/bin/kyth-launch-installer ""
EOF
# Validate sudoers syntax — fail the ISO build instead of shipping a broken file.
visudo -c -f /etc/sudoers.d/liveuser-live

# ── Timezone + machine-id (same as Bazzite) ───────────────────────────────────
rm -f /etc/localtime
ln -sf /usr/share/zoneinfo/UTC /etc/localtime
echo "uninitialized" >/etc/machine-id

# ── EFI binaries for ISO boot (exactly as Bazzite does it) ───────────────────
mkdir -p /boot/efi
cp -av /usr/lib/efi/*/*/EFI /boot/efi/
cp -v /boot/efi/EFI/fedora/grubx64.efi /boot/efi/EFI/BOOT/fbx64.efi || true

# ── iso.yaml for the GRUB menu ────────────────────────────────────────────────
mkdir -p /usr/lib/bootc-image-builder
cp /src/installer/iso.yaml /usr/lib/bootc-image-builder/iso.yaml

dnf clean all
