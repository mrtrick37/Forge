#!/usr/bin/env bash
# Local PR gate: exact GitHub Validation plus changed-file Codacy analysis.
set -euo pipefail

# Heaviest local gate (validate + quality + Codacy + CodeQL). Must not starve
# the desktop compositor — see lib/desktop-throttle.sh. Falls back
# gracefully on CI where --user is unavailable.
repo_root="$(git rev-parse --show-toplevel)"
# shellcheck source=lib/desktop-throttle.sh disable=SC1091
source "${repo_root}/build_files/scripts/lib/desktop-throttle.sh"
kyth_deprioritize_on_desktop "$@"

cd "${repo_root}"

echo "==> GitHub Validation parity"
./build_files/scripts/validate.sh

echo "==> Snapshot dry-run (release gate)"
# The perf gate itself already ran for real inside validate.sh above
# (build_files/scripts/check-perf-gate.py) — this used to be a second,
# separate call that passed current_ms=None and could only ever print a
# trivial dry-run, never actually check anything.
snapshot_timeline_cmd=()
for candidate in \
	/usr/bin/kyth-snapshot-timeline \
	"${repo_root}/src/kyth-shared-rs/target/release/kyth-snapshot-timeline" \
	"${repo_root}/src/kyth-shared-rs/target/debug/kyth-snapshot-timeline"; do
	if [[ -x "${candidate}" ]]; then
		snapshot_timeline_cmd=("${candidate}")
		break
	fi
done
if ((${#snapshot_timeline_cmd[@]} == 0)); then
	snapshot_timeline_cmd=(cargo run --quiet --manifest-path src/kyth-shared-rs/Cargo.toml --bin kyth-snapshot-timeline --)
fi
if snapshot_json="$("${snapshot_timeline_cmd[@]}" --json --limit 3 2>/dev/null)"; then
	printf 'snapshot dry-run: %s entries\n' "$(jq -r 'length' <<<"${snapshot_json}")"
else
	echo "snapshot dry-run: no timeline"
fi

echo "==> GitHub quality parity"
./build_files/scripts/run-quality.sh

if [[ "${KYTH_SKIP_CODACY_PREFLIGHT:-0}" == "1" ]]; then
	echo "==> Codacy preflight skipped with KYTH_SKIP_CODACY_PREFLIGHT=1"
	exit 0
fi

base_ref="${KYTH_PREFLIGHT_BASE_REF:-origin/main}"
if ! git rev-parse --verify --quiet "${base_ref}^{commit}" >/dev/null; then
	base_ref="main"
fi
if ! git rev-parse --verify --quiet "${base_ref}^{commit}" >/dev/null; then
	echo "ERROR: cannot resolve preflight base; set KYTH_PREFLIGHT_BASE_REF" >&2
	exit 1
fi

merge_base="$(git merge-base HEAD "${base_ref}")"
work_dir="$(mktemp -d)"
trap 'rm -rf "${work_dir}"' EXIT
changed_file="${work_dir}/changed-files.txt"
report="${work_dir}/codacy.sarif"

{
	git diff --name-only --diff-filter=ACMR "${merge_base}"...HEAD
	git diff --name-only --diff-filter=ACMR
	git diff --cached --name-only --diff-filter=ACMR
	git ls-files --others --exclude-standard
} | LC_ALL=C sort -u >"${changed_file}"

if [[ ! -s "${changed_file}" ]]; then
	echo "==> Codacy: no changed files relative to ${base_ref}"
	exit 0
fi

echo "==> Codacy analyzers (changed-file gate relative to ${base_ref})"
./.codacy/cli.sh analyze --format sarif --output "${report}"

python3 build_files/scripts/filter-sarif.py --root "${repo_root}" --changed-files "${changed_file}" --sarif "${report}" --label "Codacy"

echo "==> Codacy preflight passed"

if [[ "${KYTH_SKIP_CODEQL_PREFLIGHT:-0}" == "1" ]]; then
	echo "==> CodeQL preflight skipped with KYTH_SKIP_CODEQL_PREFLIGHT=1"
	exit 0
fi

# Keep this aligned with the bundle reported by .github/workflows/codeql.yml.
codeql_version="2.26.1"
codeql_sha256="02cb5f5d2ae8332ffc65b889eac2c88a4b57f5c66c5ebddc90ebf1c24eefcf67"
codeql_cache_root="${KYTH_CODEQL_CACHE:-${XDG_CACHE_HOME:-/var/tmp/kyth-${UID}}/kyth-codeql}"
codeql_cache="${codeql_cache_root}/codeql-${codeql_version}"
codeql_bin="${codeql_cache}/codeql/codeql"
if [[ ! -x "${codeql_bin}" ]]; then
	echo "==> Installing pinned CodeQL ${codeql_version} (one-time download)"
	mkdir -p "${codeql_cache}"
	archive="${work_dir}/codeql-bundle-linux64.tar.zst"
	curl --fail --location --silent --show-error \
		--output "${archive}" \
		"https://github.com/github/codeql-action/releases/download/codeql-bundle-v${codeql_version}/codeql-bundle-linux64.tar.zst"
	echo "${codeql_sha256}  ${archive}" | sha256sum --check --strict
	tar --extract --zstd --file "${archive}" --directory "${codeql_cache}"
fi

echo "==> CodeQL ${codeql_version} security-extended (Python)"
codeql_db="${work_dir}/codeql-db"
codeql_report="${work_dir}/codeql.sarif"
"${codeql_bin}" database create "${codeql_db}" \
	--language=python \
	--source-root="${repo_root}" \
	--overwrite
"${codeql_bin}" database analyze "${codeql_db}" \
	codeql/python-queries:codeql-suites/python-security-extended.qls \
	codeql/python-queries:AlertSuppression.ql \
	--format=sarif-latest \
	--output="${codeql_report}"

python3 build_files/scripts/filter-sarif.py --root "${repo_root}" --changed-files "${changed_file}" --sarif "${codeql_report}" --label "CodeQL"

echo "==> CI preflight passed"
