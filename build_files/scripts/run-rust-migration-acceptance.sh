#!/usr/bin/env bash
# Run the exact-image Rust migration acceptance flow and preserve evidence.
# This wrapper intentionally leaves its artifact directory for review.
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: run-rust-migration-acceptance.sh --iso PATH --image-ref IMAGE [options]

Options:
  --iso PATH                 Exact live ISO to boot.
  --image-ref IMAGE          Exact promoted image ref or digest to test.
  --artifacts DIR            Evidence directory (default: temporary directory).
  --timeout-minutes N        Disposable VM timeout (default: 75).
  --allow-tcg                Allow software emulation (normally too slow).
  --help                     Show this help.

The wrapper records source-ledger and runtime-report results, image metadata
when available, then runs the disposable install/update/rollback harness.
Destructive hardware and dual-boot operations remain manual disposable-device
steps documented in docs/rust-migration-acceptance-gates.md.
EOF
}

ISO=""
IMAGE_REF=""
ARTIFACTS=""
TIMEOUT_MINUTES=75
ALLOW_TCG=0

while (($#)); do
    case "$1" in
        --iso)
            ISO="${2:?missing ISO path}"
            shift 2
            ;;
        --image-ref)
            IMAGE_REF="${2:?missing image reference}"
            shift 2
            ;;
        --artifacts)
            ARTIFACTS="${2:?missing artifact directory}"
            shift 2
            ;;
        --timeout-minutes)
            TIMEOUT_MINUTES="${2:?missing timeout}"
            shift 2
            ;;
        --allow-tcg)
            ALLOW_TCG=1
            shift
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            usage >&2
            exit 64
            ;;
    esac
done

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"

[[ -n "${ISO}" && -r "${ISO}" ]] || {
    echo "--iso must name a readable ISO" >&2
    exit 64
}
[[ -n "${IMAGE_REF}" && "${IMAGE_REF}" =~ ^[A-Za-z0-9._/@:+-]+$ ]] || {
    echo "--image-ref contains unsupported characters" >&2
    exit 64
}
[[ "${TIMEOUT_MINUTES}" =~ ^[1-9][0-9]*$ ]] || {
    echo "--timeout-minutes must be a positive integer" >&2
    exit 64
}

for command in git python3 qemu-img qemu-system-x86_64; do
    command -v "${command}" >/dev/null || {
        echo "Missing required command: ${command}" >&2
        exit 69
    }
done

if [[ -z "${ARTIFACTS}" ]]; then
    ARTIFACTS="$(mktemp -d /tmp/kyth-rust-migration-acceptance.XXXXXXXX)"
else
    mkdir -p "${ARTIFACTS}"
fi
ARTIFACTS="$(realpath "${ARTIFACTS}")"
mkdir -p "${ARTIFACTS}/vm"

{
    printf 'schema_version=1\n'
    printf 'started_at_utc=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf 'source_commit=%s\n' "$(git -C "${REPO_ROOT}" rev-parse HEAD)"
    printf 'image_ref=%s\n' "${IMAGE_REF}"
    printf 'iso=%s\n' "$(realpath "${ISO}")"
    printf 'artifacts=%s\n' "${ARTIFACTS}"
} >"${ARTIFACTS}/run-metadata.txt"

python3 "${REPO_ROOT}/build_files/scripts/check-runtime-recipe-inventory.py" \
    >"${ARTIFACTS}/recipe-ledger.txt"
python3 "${REPO_ROOT}/build_files/scripts/check-runtime-migration-inventory.py" \
    >"${ARTIFACTS}/runtime-report.txt"

if command -v skopeo >/dev/null 2>&1 && [[ "${IMAGE_REF}" == */* ]]; then
    skopeo inspect "docker://${IMAGE_REF}" >"${ARTIFACTS}/image-metadata.json"
elif command -v podman >/dev/null 2>&1; then
    podman image inspect "${IMAGE_REF}" >"${ARTIFACTS}/image-metadata.json"
else
    echo "No skopeo or podman available for image metadata; VM run may still proceed." \
        >"${ARTIFACTS}/image-metadata-unavailable.txt"
fi

VM_ARGS=(
    --iso "${ISO}"
    --update-ref "${IMAGE_REF}"
    --artifacts "${ARTIFACTS}/vm"
    --timeout-minutes "${TIMEOUT_MINUTES}"
)
if ((ALLOW_TCG)); then
    VM_ARGS+=(--allow-tcg)
fi

"${REPO_ROOT}/build_files/scripts/vm-acceptance.sh" "${VM_ARGS[@]}"

[[ -s "${ARTIFACTS}/vm/serial.log" ]] || {
    echo "VM acceptance did not produce serial.log" >&2
    exit 1
}
[[ -s "${ARTIFACTS}/vm/qualification.json" ]] || {
    echo "VM acceptance did not produce qualification.json" >&2
    exit 1
}

cat >"${ARTIFACTS}/evidence-status.txt" <<'EOF'
source_owner_ledger=pass
runtime_reachability_report=pass
exact_image_identity=review_image_metadata
live_install_and_first_boot=pass
update_and_rollback=pass
destructive_hardware_paths=manual_disposable_device_required
dual_boot_paths=manual_disposable_device_required
observation_window=not_started
EOF

printf 'Acceptance evidence written to %s\n' "${ARTIFACTS}"
