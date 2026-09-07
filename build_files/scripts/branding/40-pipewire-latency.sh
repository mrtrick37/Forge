# shellcheck shell=bash
# ── PipeWire low-latency presets ───────────────────────────────────────────
# kyth-apply-pipewire-latency is the native Rust binary copied from the
# hub-web-builder stage; no Python launcher remains in the source tree.
# Best-effort apply during image build (usually a no-op without user toml);
# at runtime, users re-run kyth-apply-pipewire-latency after editing
# ~/.config/kyth/pipewire-latency.toml to write real pipewire.conf.d drop-ins.
if command -v kyth-apply-pipewire-latency >/dev/null 2>&1; then
    kyth-apply-pipewire-latency 2>/dev/null || true
fi
