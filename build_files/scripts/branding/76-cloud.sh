# shellcheck shell=bash
# ── Cloud Drive parity (rclone + kio) ────────────────────────────────────
# kyth-cloud-mount is the native Rust binary copied from the
# hub-web-builder stage; no Python launcher remains in the source tree.
install -m 0644 /ctx/rclone@.service /usr/lib/systemd/user/rclone@.service
# kio network:/ Dolphin entry via kio-rclone already if rclone present
