# shellcheck shell=bash
# ── Storage Sense ─────────────────────────────────────────────────────────────
# Automatic housekeeping: empty Recycle Bin items older than 30 days, drop unused
# Flatpak runtimes, vacuum the user journal. Opt-in: the timer ships disabled and
# System Hub -> Health Report has the on/off switch.
# kyth-storage-sense is the native Rust binary copied from the
# hub-web-builder stage; no Python launcher remains in the source tree.

write_config /usr/lib/systemd/user/kyth-storage-sense.service <<'STORAGESENSESVCEOF'
[Unit]
Description=KythOS Storage Sense cleanup

[Service]
Type=oneshot
ExecStart=/usr/bin/kyth-storage-sense
STORAGESENSESVCEOF

write_config /usr/lib/systemd/user/kyth-storage-sense.timer <<'STORAGESENSETIMEREOF'
[Unit]
Description=Weekly KythOS Storage Sense cleanup

[Timer]
OnCalendar=weekly
Persistent=true
RandomizedDelaySec=1h

[Install]
WantedBy=timers.target
STORAGESENSETIMEREOF
