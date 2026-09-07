# shellcheck shell=bash
# ── Tailscale mesh ───────────────────────────────────────────────────────
# kyth-apply-tailscale is the native Rust binary copied from the
# hub-web-builder stage; no Python launcher remains in the source tree.
# firewalld trusted zone for tailscale0 (offline, hash-gated)
if command -v firewall-cmd >/dev/null 2>&1; then
    firewall-cmd --permanent --zone=trusted --add-interface=tailscale0 2>/dev/null || true
fi
