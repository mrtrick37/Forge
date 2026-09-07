#!/bin/bash
# Runtime-only post-upgrade wiring for KythOS.
# Static OS configuration is written by sysconfig-static.sh so those files are not
# overwritten after the static build layer has applied network hardening.
# Fix 9: skip re-hashing when sysconfig hash unchanged (saves ~1.2 s on no-op builds)

set -euo pipefail

# Install optional helpers when provided by build context.
# kyth-ai-dev and kyth-smoke-check are installed as kyth-shared entry points.

if [[ -f /ctx/game-performance ]]; then
	install -Dm0755 /ctx/game-performance /usr/bin/game-performance
elif [[ -f /usr/bin/kyth-game-boost ]]; then
	ln -sf /usr/bin/kyth-game-boost /usr/bin/game-performance
fi

if [[ -f /ctx/kyth-game-launch ]]; then
	install -Dm0755 /ctx/kyth-game-launch /usr/bin/kyth-game-launch
fi

if [[ -f /ctx/kyth-shader-prune ]]; then
	install -Dm0755 /ctx/kyth-shader-prune /usr/bin/kyth-shader-prune
fi

# kyth-ntfs-repair is the native Rust binary copied from the hub-web-builder;
# no Python launcher remains in the source tree.

if [[ -f /ctx/kyth-shader-preheat ]]; then
	install -Dm0755 /ctx/kyth-shader-preheat /usr/bin/kyth-shader-preheat
fi

# kyth-health-check is the native Rust binary copied from the hub-web-builder;
# retain the Python launcher in the source tree for parity only.
# Repair accounts/groups that may be missing after layering package changes.
if [[ -x /usr/libexec/kyth-fix-system-accounts ]]; then
	/usr/libexec/kyth-fix-system-accounts || true
fi

for group in docker plugdev polkitd; do
	if ! grep -q "^${group}:" /etc/group && grep -q "^${group}:" /usr/lib/group; then
		grep "^${group}:" /usr/lib/group >>/etc/group
	fi
done

# Keep Plasma Login Manager as the display manager across upgrades.
if [[ ! -f /usr/lib/systemd/system/plasmalogin.service ]]; then
	echo "ERROR: plasmalogin.service missing; cannot set display-manager" >&2
	exit 1
fi
mkdir -p /etc/systemd/system/graphical.target.wants
systemctl unmask plasmalogin.service 2>/dev/null || true
ln -sf /usr/lib/systemd/system/plasmalogin.service /etc/systemd/system/display-manager.service
ln -sf /etc/systemd/system/display-manager.service /etc/systemd/system/graphical.target.wants/display-manager.service
ln -sf /usr/lib/systemd/system/graphical.target /etc/systemd/system/default.target
rm -f /etc/systemd/system/sddm.service
ln -s /dev/null /etc/systemd/system/sddm.service

# Runtime fix for stale /etc overlay on ostree hosts (bootc upgrade preserves
# /etc, so a pre-PLM deployment's display-manager.service -> sddm.service
# survives even though the new image no longer ships sddm). Install a one-shot
# that runs before the greeter on every boot and re-applies the PLM wiring.
install -d -m 0755 /usr/libexec
install -m 0755 /ctx/sysconfig/kyth-migrate-display-manager /usr/libexec/kyth-migrate-display-manager 2>/dev/null || install -m 0755 "$(dirname "${BASH_SOURCE[0]}")/kyth-migrate-display-manager" /usr/libexec/kyth-migrate-display-manager
cat >/usr/lib/systemd/system/kyth-migrate-display-manager.service <<'MIGRATESERVICEEOF'
[Unit]
Description=Migrate stale display-manager from SDDM to Plasma Login Manager
DefaultDependencies=no
After=local-fs.target
Before=plasmalogin.service display-manager.service
ConditionPathExists=/usr/lib/systemd/system/plasmalogin.service
StartLimitIntervalSec=60
StartLimitBurst=5

[Service]
Type=oneshot
ExecStart=/usr/libexec/kyth-migrate-display-manager
RemainAfterExit=yes
TimeoutStartSec=30

[Install]
WantedBy=graphical.target
MIGRATESERVICEEOF
systemctl enable kyth-migrate-display-manager.service 2>/dev/null || true

# Service masks/disables that are intentionally runtime-layer policy.
# NetworkManager-wait-online.service is deliberately NOT disabled here — it is
# enabled later in branding/31-ujust-recipes.sh (which runs after this script
# in the Dockerfile) so kyth-flathub-setup/kyth-default-flatpaks don't race DNS
# at boot. Do not re-add a disable for it here; the two would silently fight
# over the same unit depending on layer order.
systemctl mask systemd-remount-fs.service
# boot.automount: systemd-gpt-auto-generator speculatively claims /boot for the
# EFI System Partition (nvme0n1p2) on GPT+ostree layouts where the ESP is not
# actually meant to be mounted there — bootupd manages it on demand, and /boot
# is really a bind mount of the root subvolume (its real dependency is
# local-fs.target via the boot.mount symlink in local-fs.target.requires/,
# unaffected by this mask). The stray automount conflicts with that bind mount
# during boot ordering. `systemctl mask` writes to /etc, which depends on
# ostree's 3-way /etc merge; masking directly under /usr/lib instead sits below
# generator.late/ (where boot.automount is written) in unit load precedence,
# so it wins without that dependency.
ln -sf /dev/null /usr/lib/systemd/system/boot.automount
systemctl disable packagekit.service 2>/dev/null || true
systemctl disable rpm-ostree-countme.timer 2>/dev/null || true
systemctl disable fedora-atomic-desktop-appstream-cache-refresh.service 2>/dev/null || true
systemctl disable serial-getty@ttyS0.service 2>/dev/null || true

mkdir -p /etc/systemd/user
ln -sf /dev/null /etc/systemd/user/plasma-discover-notifier.service

# Runtime services that should be enabled on the installed system.
systemctl enable rtkit-daemon.service 2>/dev/null || true
systemctl enable input-remapper.service 2>/dev/null || true
systemctl enable joycond.service 2>/dev/null || true
systemctl enable bluetooth.service 2>/dev/null || true
systemctl enable kyth-bluetooth-enable.service 2>/dev/null || true

# Drop the build-context helper copies now that everything needed from /ctx
# (here and in the earlier sysconfig-static layer) has been installed.
# Left in place they'd ship as duplicate content outside any package/kyth
# rechunk group, landing in the churny "unpackaged" catch-all on every build
# that touches one of these small scripts.
rm -f /ctx/kyth-vscode-wallet /ctx/game-performance \
	/ctx/kyth-shader-preheat \
	/ctx/kyth-sched-arbiter /ctx/kyth-power-arbiter /ctx/kyth-power-arbiter.service /ctx/kyth-storage-gate \
	/ctx/kyth-readahead-hint /ctx/kyth-game-launch /ctx/kyth-shader-prune
