#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

source "../../lib/config-helpers.sh"

# ── RAM-aware memory tuning ─────────────────────────────────────────────────
# Installs memory-tune generator and systemd unit. The generator reads
# MemTotal once at boot and writes 99-kyth-memory.conf override (lexically
# after 99-kyth-base.conf) with swappiness/watermark/dirty scaling.

write_config /usr/lib/systemd/system/kyth-memory-tune.service <<'MEMSVC'
[Unit]
Description=Kyth RAM-aware memory tuning (MemTotal scaling)
# After=multi-user.target + WantedBy=multi-user.target is a cycle; start
# once filesystems and the stock sysctl pass are up.
After=local-fs.target systemd-sysctl.service
ConditionPathExists=/proc/meminfo

[Service]
Type=oneshot
ExecStart=/usr/bin/kyth-memory-tune apply
# Apply only the file this unit just wrote. `sysctl --system` re-applies
# network keys (tcp_congestion_control=bbr, default_qdisc) and fails the
# whole unit when those modules are absent — ENOENT is not a memory-tune bug.
# '-' keeps a single rejected key (dirty_bytes vs dirty_ratio) from failing
# the boot unit list after the file was written.
ExecStartPost=-/usr/bin/sysctl --load=/etc/sysctl.d/99-kyth-memory.conf
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
MEMSVC

systemctl enable kyth-memory-tune.service 2>/dev/null || true
