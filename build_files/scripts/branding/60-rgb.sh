# shellcheck shell=bash
# ── RGB peripherals (openrgb/liquidctl) ──────────────────────────────────
# kyth-apply-rgb is the native Rust binary copied from the
# hub-web-builder stage; no Python launcher remains in the source tree.
# kyth-rgb.service already execs the stable /usr/bin path (unchanged).
install -m 0644 /ctx/kyth-rgb.service /usr/lib/systemd/user/kyth-rgb.service
systemctl --global enable kyth-rgb.service 2>/dev/null || true
