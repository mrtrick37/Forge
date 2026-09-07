#!/bin/bash
# shellcheck shell=bash
# 00-sysctl-compose — consolidated sysctl generator (single writer for 99-kyth-*.conf).
# Runs in sysconfig-static layer; replaces fragmented write_config sysctl fragments.
set -euo pipefail

if [[ -x /usr/bin/kyth-sysctl-compose ]]; then
    /usr/bin/kyth-sysctl-compose --emit-all
elif [[ -x src/kyth-shared-rs/target/release/kyth-sysctl-compose ]]; then
    src/kyth-shared-rs/target/release/kyth-sysctl-compose --emit-all
else
    cargo run --quiet --manifest-path src/kyth-shared-rs/Cargo.toml --bin kyth-sysctl-compose -- --emit-all
fi

if [[ -f /ctx/config/sysctl.conf ]]; then
    echo "00-sysctl-compose: dead file /ctx/config/sysctl.conf still present — delete after migrating keys" >&2
    exit 1
fi

# shellcheck disable=SC2012
echo "00-sysctl-compose: emitted $(ls -1 /etc/sysctl.d/99-kyth-*.conf 2>/dev/null | tr '\n' ' ')"
