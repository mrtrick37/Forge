#!/usr/bin/env bash
set -euo pipefail
# Hash-gate gaming layer — CI computes this and passes --build-arg GAMING_VERSIONS_HASH
# Local validate just ensures the resolver and Dockerfile agree and the label is not stale.
repo_root="$(git rev-parse --show-toplevel)"
cd "${repo_root}"

# Files that define gaming versions (COPRs, umu, Proton)
inputs=(
  build_files/kyth_shared/kyth_shared/gaming_resolve.py
  build_files/kyth_shared/kyth_shared/repos.py
  build_files/config/repos.json
  build_files/scripts/thirdparty.sh
  build_files/scripts/proton-cachyos.sh
)
# Compute SHA256 of sorted file contents (like hash-rpm-set.sh)
if command -v sha256sum >/dev/null 2>&1; then
  hash_cmd="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
  hash_cmd="shasum -a 256"
else
  echo "hash-gaming-versions: no sha256sum" >&2
  exit 1
fi
tmp="$(mktemp)"
for f in "${inputs[@]}"; do
  if [[ -f "${f}" ]]; then
    cat "${f}" >> "${tmp}"
    echo "---${f}---" >> "${tmp}"
  fi
done
computed="$(${hash_cmd} "${tmp}" | cut -d' ' -f1 | cut -c1-12)"
rm -f "${tmp}"

if [[ "${1:-}" == "--print" ]]; then
  echo "${computed}"
  exit 0
fi

# Check Dockerfile label exists and is hash-gated (not hardcoded version)
if ! grep -q 'LABEL org.kyth.gaming-versions="${GAMING_VERSIONS_HASH}"' Dockerfile; then
  echo "gaming hash gate: Dockerfile must have LABEL org.kyth.gaming-versions=\"\${GAMING_VERSIONS_HASH}\"" >&2
  exit 1
fi

# If GAMING_VERSIONS_HASH is pinned (not unset) in Dockerfile ARG, ensure it matches computed
arg_val="$(grep -m1 'ARG GAMING_VERSIONS_HASH=' Dockerfile | cut -d'=' -f2 || echo unset)"
if [[ "${arg_val}" != "unset" && "${arg_val}" != "" && "${arg_val}" != "${computed}" ]]; then
  echo "gaming hash mismatch: Dockerfile ARG GAMING_VERSIONS_HASH=${arg_val} != computed ${computed}" >&2
  echo "Fix: update Dockerfile ARG or re-run hash-gaming-versions logic in CI" >&2
  # Locally allow mismatch when arg is unset (default) — only fail when pinned and wrong
  if [[ "${arg_val}" != "unset" ]]; then
    exit 1
  fi
fi

# Also ensure the native resolver and label path are callable. Prefer the
# packaged binary, then a checkout build, then Cargo's locked fallback.
gaming_support_cmd=()
for candidate in \
  /usr/bin/kyth-build-support \
  "${repo_root}/src/kyth-shared-rs/target/release/kyth-build-support" \
  "${repo_root}/src/kyth-shared-rs/target/debug/kyth-build-support"; do
  if [[ -x "${candidate}" ]]; then
    gaming_support_cmd=("${candidate}")
    break
  fi
done
if ((${#gaming_support_cmd[@]} == 0)); then
  gaming_support_cmd=(cargo run --quiet --locked --manifest-path src/kyth-shared-rs/Cargo.toml --bin kyth-build-support --)
fi
if ! "${gaming_support_cmd[@]}" gaming-label >/dev/null; then
  echo "native gaming resolver failed" >&2
  exit 1
fi

echo "gaming hash gate: ok (computed ${computed})"
