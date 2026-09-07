# shellcheck shell=bash
# ── Scaling + ICC color ────────────────────────────────────────────────────
# ICC drop dir for optional profiles; kyth-apply-scaling writes kscreen scales
# from ~/.config/kyth/scaling.toml at runtime.
if command -v colord >/dev/null 2>&1; then
	mkdir -p /usr/share/color/icc/kyth
fi
# kyth-apply-scaling is the native Rust binary copied from the
# hub-web-builder stage; no Python launcher remains in the source tree.
# kyth-apply-display-hdr is the native Rust binary copied from the
# hub-web-builder stage; no Python launcher remains in the source tree.
