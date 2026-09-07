#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# ── greenboot boot-time health checks ────────────────────────────────────────
# greenboot marks each boot good/bad and triggers automatic rollback to the
# previous bootc deployment if health checks fail across three consecutive boots.
# KythOS deliberately does not install greenboot-default-health-checks: its
# required repository-DNS probe can reboot an otherwise healthy offline desktop
# and cannot be repaired by rolling back the OS. KythOS installs immutable,
# rollback-actionable checks during the branding phase instead.
dnf5 install -y greenboot
systemctl enable greenboot-healthcheck.service greenboot-set-rollback-trigger.service

# Upstream unit is Type=oneshot without RemainAfterExit. After it
# succeeds (often in <1s, including a /boot remount), anything that
# Wants= it starts it again — but a start job on an already-active
# oneshot no-ops, so RemainAfterExit=yes below is what actually stops
# that. StartLimit only backstops repeated *failures*; disable it
# outright rather than give it a window, and keep it in [Unit] — in
# [Service] it's silently ignored and the unit runs under systemd's
# compiled-in 10s/5 default instead. Also remount /boot the Kyth way
# (bind,rw — plain remount,rw is EINVAL on the autofs+btrfs bind).
install -d /usr/lib/systemd/system/greenboot-set-rollback-trigger.service.d
cat >/usr/lib/systemd/system/greenboot-set-rollback-trigger.service.d/10-kyth.conf <<'GBROLLBACK'
[Unit]
StartLimitIntervalSec=0
[Service]
RemainAfterExit=yes
ExecStartPre=-/usr/libexec/kyth-finalize-staged prepare-boot
GBROLLBACK
