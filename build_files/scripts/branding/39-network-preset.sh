# shellcheck shell=bash
# ── Network preset (DoT + firewalld) ───────────────────────────────────────
# kyth-apply-network is the native Rust binary copied from the
# hub-web-builder stage; no Python launcher remains in the source tree.
# Apply once at build time so resolved.conf.d exists (offline, no network fetch)
if command -v kyth-apply-network >/dev/null 2>&1; then
    kyth-apply-network 2>/dev/null || true
fi
# firewalld zone already via sysconfig; this drop-in is hash-gated
