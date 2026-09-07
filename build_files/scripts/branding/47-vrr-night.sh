# shellcheck shell=bash
# ── VRR + Night color scheduler ──────────────────────────────────────────
# kyth-apply-vrr is the native Rust binary copied from the hub-web-builder
# stage; no Python launcher remains in the source tree. It writes [Wayland]
# VrrPolicy + [NightColor] from ~/.config/kyth/vrr.toml (and best-effort
# per-output via kscreen-doctor).
