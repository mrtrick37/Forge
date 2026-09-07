# shellcheck shell=bash
# ── Backup Full (restic + btrfs send) ────────────────────────────────────
# kyth-backup is the native Rust binary copied from the
# hub-web-builder stage; no Python launcher remains in the source tree.
# backup.toml + restic repo /var/cache/kyth/backup + btrfs send hash-gated
