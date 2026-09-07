#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

source "../../lib/config-helpers.sh"

# ── Autostart log-noise guards ────────────────────────────────────────────────
# Fedora does not define Debian's plugdev group, but several third-party udev
# rules installed below reference it. Seed new deployments at image build time;
# kyth-system-accounts.service performs the equivalent repair after upgrades.
getent group plugdev >/dev/null 2>&1 || groupadd --system plugdev

# nvidia-settings ships an unconditional autostart entry that fails every
# login on AMD-only systems with "ERROR: NVIDIA driver is not loaded".
# Run it only when the NVIDIA kernel module is actually loaded.
write_config /usr/libexec/kyth-nvidia-settings-autostart 0755 <<'NVAUTOSTARTEOF'
#!/usr/bin/bash
[ -e /sys/module/nvidia ] || exit 0
exec nvidia-settings -l
NVAUTOSTARTEOF
if [ -f /etc/xdg/autostart/nvidia-settings-user.desktop ]; then
	sed -i 's|^Exec=.*|Exec=/usr/libexec/kyth-nvidia-settings-autostart|' /etc/xdg/autostart/nvidia-settings-user.desktop
fi

write_config /usr/libexec/kyth-input-remapper-autoload 0755 <<'IRAUTOSTARTEOF'
#!/usr/bin/bash
for _ in $(seq 1 120); do
	systemd-analyze time >/dev/null 2>&1 && break
	sleep 5
done
input-remapper-control --command stop-all && exec input-remapper-control --command autoload
IRAUTOSTARTEOF
if [ -f /etc/xdg/autostart/input-remapper-autoload.desktop ]; then
	sed -i 's|^Exec=.*|Exec=/usr/libexec/kyth-input-remapper-autoload|' /etc/xdg/autostart/input-remapper-autoload.desktop
fi


# input-remapper-service logs ERROR: .../config.json" does not exist on every
# login at 16:24:24 (4×) because the user config dir has never been created.
# Seed an empty skeleton config so the daemon starts quietly; the existing
# kyth-input-remapper-autoload wrapper already handles delayed autoload.
mkdir -p /etc/skel/.config/input-remapper-2
write_config /etc/skel/.config/input-remapper-2/config.json <<'IRMAPPERJSON'
{
  "autoload": {}
}
IRMAPPERJSON

write_config /usr/lib/systemd/system/kyth-system-accounts.service <<'SYSACCOUNTUNITEOF'
[Unit]
Description=Ensure KythOS system accounts are visible in /etc
DefaultDependencies=no
# mkdir -p /var/lib/plasmalogin needs /var writable; without this, the unit races
# ostree-remount.service on every boot and fails with "Read-only file system"
# (it self-heals via a second, later pull-in, but that transiently fails the
# Requires= dependent kyth-dbus-runtime-dir.service too).
After=local-fs.target ostree-remount.service
# Must run before tmpfiles/sysusers/udev so groups like audio,disk,kvm are
# visible when they parse static-nodes/udev rules — otherwise every boot
# logs "Failed to resolve group 'audio': Unknown group" (see host journal).
Before=dbus.socket dbus-broker.service sockets.target plasmalogin.service systemd-udevd.service systemd-udevd-control.socket systemd-udevd-kernel.socket
Before=systemd-tmpfiles-setup.service systemd-sysusers.service

[Service]
Type=oneshot
ExecStart=/usr/libexec/kyth-fix-system-accounts
RemainAfterExit=yes

[Install]
WantedBy=sysinit.target
SYSACCOUNTUNITEOF

install -d -m 0755 /usr/libexec
install -m 0755 /ctx/sysconfig/kyth-fix-system-accounts /usr/libexec/kyth-fix-system-accounts
systemctl enable kyth-system-accounts.service 2>/dev/null || true

# input-remapper.service is the single owner of preset autoloading. The RPM's
# per-event udev rule races the daemon and launches one process for every input
# node during boot, all before the service is ready.
ln -sf /dev/null /etc/udev/rules.d/99-input-remapper.rules

# ublue-os-udev-rules uses negative TEST expressions, so it tries to chmod
# battery attributes specifically when they do not exist. Replace it with
# positive relative sysfs existence tests.
write_config /etc/udev/rules.d/99-thinkpad-thresholds-udev.rules <<'BATTERYRULESEOF'
# KythOS override: expose only threshold attributes provided by this battery.
ACTION=="add|change", SUBSYSTEM=="power_supply", KERNEL=="BAT[0-1]", TEST=="charge_control_start_threshold", RUN+="/bin/chgrp wheel /sys%p/charge_control_start_threshold", RUN+="/bin/chmod 0664 /sys%p/charge_control_start_threshold"
ACTION=="add|change", SUBSYSTEM=="power_supply", KERNEL=="BAT[0-1]", TEST=="charge_control_end_threshold", RUN+="/bin/chgrp wheel /sys%p/charge_control_end_threshold", RUN+="/bin/chmod 0664 /sys%p/charge_control_end_threshold"
ACTION=="add|change", SUBSYSTEM=="power_supply", KERNEL=="BAT[0-1]", TEST=="charge_start_threshold", RUN+="/bin/chgrp wheel /sys%p/charge_start_threshold", RUN+="/bin/chmod 0664 /sys%p/charge_start_threshold"
ACTION=="add|change", SUBSYSTEM=="power_supply", KERNEL=="BAT[0-1]", TEST=="charge_stop_threshold", RUN+="/bin/chgrp wheel /sys%p/charge_stop_threshold", RUN+="/bin/chmod 0664 /sys%p/charge_stop_threshold"
BATTERYRULESEOF

mkdir -p /etc/asusd

write_config /usr/lib/systemd/system/kyth-dbus-runtime-dir.service <<'DBUSRUNDIREOF'
[Unit]
Description=Create D-Bus runtime directory
DefaultDependencies=no
Before=sockets.target dbus.socket
# Ordered after, not Requires=: this unit only mkdirs a tmpfs path and does not
# itself need kyth-system-accounts.service to succeed. A hard Requires= meant a
# transient failure there (see kyth-system-accounts.service) permanently failed
# this unit for the boot even though the later retry succeeded.
After=kyth-system-accounts.service local-fs.target
Wants=kyth-system-accounts.service
# RemainAfterExit=yes below is what stops dbus.socket / sockets.target from
# re-running this unit: a start job on an already-active oneshot no-ops.
# StartLimit only backstops repeated *failures*, so disable it outright
# rather than give it a window — these keys belong in [Unit], not
# [Service]; stranded in [Service] they're silently ignored and the unit
# runs under systemd's compiled-in 10s/5 default instead.
StartLimitIntervalSec=0

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/usr/bin/mkdir -p /run/dbus
ExecStart=/usr/bin/chmod 0755 /run/dbus

[Install]
WantedBy=sysinit.target
DBUSRUNDIREOF
systemctl enable kyth-dbus-runtime-dir.service 2>/dev/null || true

write_config /etc/systemd/system/dbus-broker.service.d/10-kyth-no-audit.conf <<'DBUSBROKEREOF'
[Service]
ExecStart=
ExecStart=/usr/bin/dbus-broker-launch --scope system
DBUSBROKEREOF
