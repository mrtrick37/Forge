#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# ── Tailscale repo bootstrap ──────────────────────────────────────────────────
# WireGuard-based mesh VPN with no port forwarding required. Useful for LAN party
# gaming over the internet and remote desktop access.
# Not installed at build time — the package itself is fetched on demand via
# `ujust setup-tailscale`, which enables this repo just-in-time. Vendoring the
# repo config here (rather than fetching it at first use) keeps a transient
# CDN blip from being able to break `ujust setup-tailscale` on a fresh install,
# and disabling it immediately keeps it from persisting as an active package
# source in images that never opt in.
mkdir -p /etc/yum.repos.d
/usr/bin/kyth-build-support repo-render \
	--config /ctx/config/repos.json \
	--name tailscale-stable \
	--output /etc/yum.repos.d/tailscale-stable.repo
dnf5 config-manager setopt tailscale-stable.enabled=0
