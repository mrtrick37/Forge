#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# ── Windows environment management tools ─────────────────────────────────────
# RDP, Active Directory, Kerberos, and SMB tooling in standard Fedora repos.
# Azure CLI is provided via the kyth-ai-dev container to keep the core OS slim.

/usr/bin/kyth-build-support container-wrapper \
	--tool az --description "Azure CLI" --output /usr/bin/az


# RDP and SMB tooling — standard Fedora repos. samba-client is a required base
# capability alongside cifs-utils: it supplies smbclient/net/wbinfo for share
# diagnostics and Windows-domain interoperability, while mount.cifs handles
# persistent mounts and libsmbclient backs desktop browsing.
# freerdp: best-in-class RDP client; powers Remmina's RDP backend.
# samba-client: smbclient + net ads + wbinfo for SMB share browsing.
dnf5 install -y cifs-utils libsmbclient samba-client
dnf5 install -y --skip-unavailable freerdp
