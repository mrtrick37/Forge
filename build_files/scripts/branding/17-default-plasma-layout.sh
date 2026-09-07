# shellcheck shell=bash
# ── KythOS default Plasma layout preset ───────────────────────────────────────
# kyth-apply-desktop-layout is the native Rust binary copied from the
# hub-web-builder stage; no Python launcher remains in the source tree.
install -m 0755 /ctx/kyth-refresh-taskbar-pins /usr/bin/kyth-refresh-taskbar-pins
install -m 0644 /ctx/kyth-scripts/kyth-refresh-taskbar-pins.service \
	/usr/lib/systemd/user/kyth-refresh-taskbar-pins.service
install -m 0644 /ctx/kyth-scripts/kyth-refresh-taskbar-pins.path \
	/usr/lib/systemd/user/kyth-refresh-taskbar-pins.path
mkdir -p /etc/systemd/user/default.target.wants
