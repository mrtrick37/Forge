#!/bin/bash
# shellcheck shell=bash
# 01-tunable-dispatcher — install the native dispatcher + 94 aliases.
# Replaces the former Python/bash dispatcher and 94 thin wrappers with one
# Rust binary and symlinks. Preserves symlinks via ln -sf (not cp without -a,
# which would dereference).
set -euo pipefail

# Install the full mutation-capable Rust dispatcher under both names. The
# direct kyth-tunable name is intentionally native too; there is no Python
# fallback in the supported image.
# /usr/bin/kyth-tunable-rs is copied into the image before this static layer.
ln -sfn kyth-tunable-rs /usr/bin/kyth-tunable

# Create compat symlinks for every tunable in the native registry.
mapfile -t tunables < <(/usr/bin/kyth-tunable-rs --list)
mapfile -t native_tunables < <(/usr/bin/kyth-tunable-rs --list-native)
declare -A native_lookup=()
for t in "${native_tunables[@]}"; do
    native_lookup["$t"]=1
done

for t in "${tunables[@]}"; do
    if [[ ! ${native_lookup[$t]+yes} ]]; then
        echo "tunable-dispatcher: registry entry lacks a Rust implementation: ${t}" >&2
        exit 1
    fi
    ln -sf kyth-tunable-rs "/usr/bin/kyth-${t}"
done

echo "tunable-dispatcher: installed kyth-tunable + ${#tunables[@]} symlinks (${#native_tunables[@]} native)"
