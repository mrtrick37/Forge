#!/usr/bin/env bash
# Remove only KythOS ISO/VM acceptance state.
#
# This script is intentionally narrower than `podman system prune` (or the
# existing general-purpose clean recipes). It is safe to run before a new
# acceptance build and after an interrupted run. The default action removes
# the previous ISO as well, so the next build cannot accidentally test stale
# media. Use --archive-qualification after a run when the small report/log
# files should be retained outside the disposable VM directory.
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: cleanup-vm-acceptance.sh [--dry-run] [--archive-qualification]

Remove disposable KythOS ISO/VM acceptance artifacts, stale test-specific
mounts, and locally tagged kyth-live images. The command refuses to remove
anything while QEMU, a SPICE viewer, or a live-image build is running.

--archive-qualification  Copy qualification.json, qualification.md, serial.log,
                         and qemu.log to output/qualification/<UTC timestamp>
                         before deleting the disposable acceptance directory.
--dry-run                Print the exact cleanup plan without changing state.
EOF
}

DRY_RUN=0
ARCHIVE_QUALIFICATION=0
while (($#)); do
	case "$1" in
	--dry-run)
		DRY_RUN=1
		shift
		;;
	--archive-qualification)
		ARCHIVE_QUALIFICATION=1
		shift
		;;
	--help | -h)
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
REPO_ROOT="${KYTH_CLEANUP_REPO_ROOT:-$(cd -- "${SCRIPT_DIR}/../.." && pwd)}"
TMP_ROOT="${KYTH_CLEANUP_TMP_ROOT:-/tmp}"
VAR_TMP_ROOT="${KYTH_CLEANUP_VAR_TMP_ROOT:-/var/tmp}"
SKIP_PODMAN="${KYTH_CLEANUP_SKIP_PODMAN:-0}"
ROOTFUL_PODMAN="${REPO_ROOT}/build_files/scripts/rootful-podman.sh"

# These are exact, test-owned locations. Do not turn these into broad globs.
TARGETS=(
	"${REPO_ROOT}/output/vm-acceptance"
	"${REPO_ROOT}/output/live-iso"
	"${REPO_ROOT}/tmp/kyth-rootful-btrfs-storage"
	"${REPO_ROOT}/tmp/kyth-rootful-btrfs-run"
	"${REPO_ROOT}/tmp/kyth-podman-test-root"
	"${REPO_ROOT}/tmp/kyth-podman-test-run"
	"${REPO_ROOT}/tmp/kyth-container-tmp"
	"${VAR_TMP_ROOT}/kyth-vm-disks-${USER}"
	"${VAR_TMP_ROOT}/kyth-vm-share-${USER}"
	"${VAR_TMP_ROOT}/kyth-remote-viewer-${USER}"
)

shopt -s nullglob
for pattern in \
	"${TMP_ROOT}/kyth-vm-acceptance."* \
	"${TMP_ROOT}/kyth-installer-packages."* \
	"${TMP_ROOT}/kyth-titanoboa-ok."* \
	"${TMP_ROOT}/kyth-hub-acceptance-"* \
	"${TMP_ROOT}/kyth-os-build.log" \
	"${VAR_TMP_ROOT}/kyth-live."* \
	"${VAR_TMP_ROOT}/kyth-titanoboa."*; do
	TARGETS+=("${pattern}")
done

contains_active_work() {
	local pid comm command
	while read -r pid comm command; do
		[[ -n "${pid:-}" && "${pid}" != "$$" ]] || continue
		case "${comm}" in
		qemu-system-x86_64|remote-viewer|spicy|podman|buildah|docker)
			case "${command}" in
			*\ build\ *|*\ bud\ *|*\ buildx\ *)
				printf '%s\t%s\n' "${pid}" "${comm} ${command}"
				;;
			esac
			;;
		bash|env|timeout)
			case "${command}" in
			*build-live-iso.sh*|*build_iso.sh*)
			printf '%s\t%s\n' "${pid}" "${command}"
				;;
			esac
			;;
		esac
	done < <(ps -eo pid=,comm=,args=)
}

ACTIVE_WORK="$(contains_active_work)"
if [[ -n "${ACTIVE_WORK}" ]]; then
	echo "Refusing cleanup while acceptance/build processes are active:" >&2
	echo "${ACTIVE_WORK}" >&2
	exit 75
fi

if ((DRY_RUN == 0)) && [[ "${SKIP_PODMAN}" != "1" ]] && [[ -x "${ROOTFUL_PODMAN}" ]]; then
	# A qemux container is another possible VM holder even when the host-side
	# process is not visible. Only inspect; never stop containers implicitly.
	if ! PODMAN_WORK="$("${ROOTFUL_PODMAN}" ps --format '{{.ID}} {{.Image}} {{.Command}}' 2>/dev/null | awk '$2 ~ /qemux\/qemu|kyth-live/ {print}')"; then
		echo "Unable to inspect rootful Podman containers; refusing cleanup" >&2
		exit 69
	fi
	if [[ -n "${PODMAN_WORK}" ]]; then
		echo "Refusing cleanup while a KythOS VM container is active:" >&2
		echo "${PODMAN_WORK}" >&2
		exit 75
	fi
fi

archive_qualification() {
	local source="${REPO_ROOT}/output/vm-acceptance"
	[[ -d "${source}" ]] || return 0
	local archive_root="${REPO_ROOT}/output/qualification"
	local archive
	archive="${archive_root}/$(date -u +%Y%m%dT%H%M%SZ)"
	local file
	local found=0
	for file in qualification.json qualification.md serial.log qemu.log; do
		if [[ -f "${source}/${file}" ]]; then
			found=1
			break
		fi
	done
	((found)) || return 0
	if ((DRY_RUN)); then
		echo "ARCHIVE ${source} -> ${archive}"
		return 0
	fi
	mkdir -p "${archive}"
	for file in qualification.json qualification.md serial.log qemu.log; do
		[[ -f "${source}/${file}" ]] && cp -p -- "${source}/${file}" "${archive}/${file}"
	done
	echo "Archived qualification evidence: ${archive}"
}

if ((ARCHIVE_QUALIFICATION)); then
	archive_qualification
fi

echo "KythOS VM acceptance cleanup plan:"
for target in "${TARGETS[@]}"; do
	[[ -e "${target}" || -L "${target}" ]] && echo "  REMOVE ${target}"
done
if ((DRY_RUN == 0)) && [[ "${SKIP_PODMAN}" != "1" ]] && [[ -x "${ROOTFUL_PODMAN}" ]]; then
	for image in $("${ROOTFUL_PODMAN}" images --format '{{.Repository}}:{{.Tag}}' | awk '$0 ~ /^localhost\/kyth-live:/ {print}'); do
		echo "  REMOVE rootful image ${image}"
	done
fi

if ((DRY_RUN)); then
	exit 0
fi

unmount_below() {
	local root="$1"
	[[ -d "${root}" ]] || return 0
	local mountpoint
	local -a mountpoints=()
	mapfile -t mountpoints < <(
		findmnt -rn -o TARGET 2>/dev/null |
			while read -r mountpoint; do
				case "${mountpoint}" in
				"${root}" | "${root}"/*) printf '%s\n' "${mountpoint}" ;;
				esac
			done | sort -r
	)
	for mountpoint in "${mountpoints[@]}"; do
		[[ -n "${mountpoint}" ]] || continue
		echo "  UNMOUNT ${mountpoint}"
		sudo umount -l -- "${mountpoint}"
	done
	# Never remove a directory that is still a mount target.
	if findmnt -rn -o TARGET 2>/dev/null | awk -v root="${root}" '$0 == root || index($0, root "/") == 1 {found=1} END {exit found ? 0 : 1}'; then
		echo "Mounts remain below ${root}; refusing to delete it" >&2
		return 1
	fi
}

remove_target() {
	local target="$1"
	local parent
	parent="$(dirname -- "${target}")"
	if [[ -w "${parent}" ]]; then
		rm -rf -- "${target}"
	elif [[ -d "${target}" && -w "${target}" ]]; then
		# A workspace bind mount can make the parent appear root-owned while
		# leaving this exact test directory user-writable. Empty it and retain
		# the harmless empty directory when the parent itself cannot be removed.
		find "${target}" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
		if [[ -n "$(find "${target}" -mindepth 1 -print -quit 2>/dev/null)" ]]; then
			echo "Cannot empty acceptance target: ${target}" >&2
			return 1
		fi
	else
		command -v sudo >/dev/null || {
			echo "Cannot remove root-owned acceptance target without sudo: ${target}" >&2
			return 1
		}
		sudo rm -rf -- "${target}"
	fi
}

for target in "${TARGETS[@]}"; do
	[[ -e "${target}" || -L "${target}" ]] || continue
	if [[ -d "${target}" ]]; then
		unmount_below "${target}"
	fi
	echo "  REMOVE ${target}"
	# The target list is fixed above; no user-provided path reaches rm.
	remove_target "${target}"
done

if [[ "${SKIP_PODMAN}" != "1" ]] && [[ -x "${ROOTFUL_PODMAN}" ]]; then
	# Interrupted Buildah/Podman builds leave external `Storage` containers
	# behind. They are not running VMs, but they pin otherwise disposable
	# intermediate images, so remove only the standard working-container names.
	while IFS='|' read -r container status name; do
		[[ "${status}" == "Storage" && ("${name}" == *-working-container || "${name}" == *-working-container-[0-9]*) ]] || continue
		echo "  REMOVE rootful build container ${container} (${name})"
		sudo podman rm --force -- "${container}"
	done < <("${ROOTFUL_PODMAN}" container ls -a --external --format '{{.ID}}|{{.Status}}|{{.Names}}')

	declare -A seen_images=()
	rootful_images=()
	while IFS='|' read -r image repository; do
		[[ -n "${image}" && -z "${seen_images[${image}]:-}" ]] || continue
		if [[ "${repository}" == "localhost/kyth:latest" ||
			"${repository}" == "localhost/kyth-base:stable" ||
			"${repository}" == localhost/kyth-live:* ]]; then
			seen_images["${image}"]=1
			rootful_images+=("${image}")
			continue
		fi
		[[ "${repository}" == "<none>:<none>" ]] || continue
		title="$("${ROOTFUL_PODMAN}" inspect --format '{{index .Config.Labels "org.opencontainers.image.title"}}' "${image}" 2>/dev/null || true)"
		if [[ "${title}" == "kinoite-main" ]]; then
			seen_images["${image}"]=1
			rootful_images+=("${image}")
		fi
	done < <("${ROOTFUL_PODMAN}" images -a --format '{{.ID}}|{{.Repository}}:{{.Tag}}')
	for image in "${rootful_images[@]}"; do
		[[ -n "${image}" ]] || continue
		"${ROOTFUL_PODMAN}" image exists "${image}" || continue
		echo "  REMOVE rootful image ${image}"
		"${ROOTFUL_PODMAN}" image rm -- "${image}"
	done
fi

echo "KythOS VM acceptance cleanup complete"
