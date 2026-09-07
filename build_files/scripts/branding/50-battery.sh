# shellcheck shell=bash
# ── Battery health + charge limit ────────────────────────────────────────
# kyth-batteryd is the native Rust binary copied from the
# hub-web-builder stage; no Python launcher remains in the source tree.
install -m 0644 /ctx/kyth-batteryd.service /usr/lib/systemd/system/kyth-batteryd.service
systemctl enable kyth-batteryd.service 2>/dev/null || true
