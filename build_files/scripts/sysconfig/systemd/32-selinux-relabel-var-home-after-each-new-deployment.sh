#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

source "../../lib/config-helpers.sh"

# ── SELinux: relabel /var/home after each new deployment ──────────────────────
# bootc/ostree relabels the OS tree (/usr, /etc) on every deployment, but /var
# is writable state — it is never touched. On enforcing systems, /var/home
# files with missing labels cause PAM and dbus-broker to be denied, making
# login impossible.
#
# A full `restorecon -RF /var/home` scales with the size of the home
# directory: on a home with a few million files (game installs, container
# storage, build caches) it can run past the timeout this unit used to run
# under, at which point systemd kills it, the completion stamp is never
# written, and every subsequent boot repeats the same doomed relabel — an
# every-boot multi-minute stall instead of a one-time cost.
#
# Split the work in two:
#   - kyth-selinux-relabel-home.service (this unit): stays Before=plasmalogin
#     and only relabels the small, fixed set of top-level paths PAM/dbus-broker
#     actually stat during login. It never recurses into a user's bulk data,
#     so its runtime is bounded by the number of local accounts, not by home
#     directory size.
#   - kyth-selinux-relabel-home-full.service: does the exhaustive `-R` pass in
#     the background, off the login-critical path, at idle I/O/CPU priority.
#
# Each is gated on its own per-deployment stamp: only relabel when the booted
# deployment differs from the last one relabeled for. If a user needs to force
# a relabel, they can remove /var/lib/kyth/selinux-relabel-home*.stamp.
write_config /usr/lib/systemd/system/kyth-selinux-relabel-home.service <<'RELABELEOF'
[Unit]
Description=SELinux relabel /var/home login-critical paths (once per deployment)
DefaultDependencies=no
After=local-fs.target
Before=plasmalogin.service display-manager.service
ConditionSecurity=selinux
# StartLimit: this unit is Before=plasmalogin and WantedBy=multi-user;
# without explicit limits a restart during the boot transaction could
# hit systemd's default start-limit and fail a healthy boot (see
# greenboot 10-kyth.conf for same pattern). Keep generous burst.
# Must live in [Unit], not [Service] — systemd logs "Unknown key
# 'StartLimitIntervalSec' in section [Service], ignoring" otherwise and
# silently drops both limits.
StartLimitIntervalSec=300
StartLimitBurst=5

[Service]
Type=oneshot
ExecStart=/usr/libexec/kyth-selinux-relabel-home
RemainAfterExit=yes
TimeoutStartSec=60

[Install]
WantedBy=multi-user.target
RELABELEOF

# Deliberately NOT Before=plasmalogin: this must never be able to delay
# login. IOSchedulingClass=idle/Nice=19 keep the full-tree walk from starving
# foreground disk I/O in an already-running session.
write_config /usr/lib/systemd/system/kyth-selinux-relabel-home-full.service <<'RELABELFULLEOF'
[Unit]
Description=SELinux relabel /var/home (full tree, background, once per deployment)
DefaultDependencies=no
After=local-fs.target kyth-selinux-relabel-home.service
ConditionSecurity=selinux
# DefaultDependencies=no drops the implicit Conflicts=/Before=shutdown.target;
# restore it explicitly so a reboot orders this oneshot's teardown instead of
# racing it (the stamp is only written on success, so an interrupted run just
# retries next boot — this is about a clean shutdown, not correctness).
Conflicts=shutdown.target
Before=shutdown.target
# See kyth-selinux-relabel-home.service above: these two must live in [Unit],
# not [Service], or systemd silently ignores them.
StartLimitIntervalSec=3600
StartLimitBurst=3

[Service]
Type=oneshot
ExecStart=/usr/libexec/kyth-selinux-relabel-home-full
RemainAfterExit=yes
IOSchedulingClass=idle
CPUSchedulingPolicy=idle
Nice=19
# Generous but finite: a hung restorecon should eventually be killed and
# retried next boot instead of running forever, but this must stay far
# looser than the login-path unit's cap above.
TimeoutStartSec=3600

[Install]
WantedBy=multi-user.target
RELABELFULLEOF

install -d -m 0755 /usr/libexec
install -m 0755 /ctx/sysconfig/kyth-selinux-relabel-home /usr/libexec/kyth-selinux-relabel-home
install -m 0755 /ctx/sysconfig/kyth-selinux-relabel-home-full /usr/libexec/kyth-selinux-relabel-home-full

systemctl enable kyth-selinux-relabel-home.service 2>/dev/null || true
systemctl enable kyth-selinux-relabel-home-full.service 2>/dev/null || true
