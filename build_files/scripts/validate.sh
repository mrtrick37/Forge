#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
# shellcheck source=lib/desktop-throttle.sh disable=SC1091
source "${repo_root}/build_files/scripts/lib/desktop-throttle.sh"
kyth_deprioritize_on_desktop "$@"

cd "${repo_root}"

if [[ -x /usr/bin/kyth-hardware-policy ]]; then
	hardware_policy_cmd=(/usr/bin/kyth-hardware-policy)
elif [[ -x src/kyth-shared-rs/target/release/kyth-hardware-policy ]]; then
	hardware_policy_cmd=(src/kyth-shared-rs/target/release/kyth-hardware-policy)
elif [[ -x src/kyth-shared-rs/target/debug/kyth-hardware-policy ]]; then
	hardware_policy_cmd=(src/kyth-shared-rs/target/debug/kyth-hardware-policy)
else
	hardware_policy_cmd=(cargo run --quiet --manifest-path src/kyth-shared-rs/Cargo.toml --bin kyth-hardware-policy --)
fi

tool_bin="$(./build_files/scripts/install-validation-tools.sh | tail -n 1)"
export PATH="${tool_bin}:${PATH}"

echo "==> GitHub Actions workflows"
actionlint -color -shellcheck=""
zizmor --persona auditor --min-severity medium --no-online-audits .github/workflows

echo "==> Container build files"
hadolint --failure-threshold error \
	Dockerfile \
	build_base/Dockerfile \
	build_base/Containerfile.docker-overlay \
	installer/Containerfile

echo "==> Shell scripts"
shell_files=()
while IFS= read -r -d '' file; do
	[[ -f "${file}" ]] || continue
	mime_type="$(file --brief --mime-type "${file}")"
	if [[ "${mime_type}" == "text/x-shellscript" ]]; then
		shell_files+=("${file}")
	fi
done < <(git ls-files -z)
if ((${#shell_files[@]} == 0)); then
	echo "No shell scripts found" >&2
	exit 1
fi
shellcheck --severity=warning "${shell_files[@]}"
for file in "${shell_files[@]}"; do
	bash -n "${file}"
done

echo "==> Python syntax"
python3 build_files/scripts/validate-python-syntax.py

echo "==> Optimization budgets"
python3 build_files/scripts/optimization-report.py --check

echo "==> Gaming hash gate"
bash build_files/scripts/hash-gaming-versions.sh

echo "==> Perf gate (10% ledger, probe collection duration)"
if [[ "${KYTH_PERF_GATE_ADVISORY:-0}" == "1" ]]; then
	if ! PYTHONPATH=build_files/kyth_shared python3 build_files/scripts/check-perf-gate.py; then
		echo "warning: perf gate regression is advisory in this local validation context" >&2
	fi
else
	PYTHONPATH=build_files/kyth_shared python3 build_files/scripts/check-perf-gate.py
fi

echo "==> Sysconfig hash gate (must stay unset locally, pinned in CI)"
if grep -qE '^ARG SYSCONFIG_HASH=unset' Dockerfile && grep -qE '^ARG RPM_SET_HASH=unset' Dockerfile && grep -qE '^ARG GAMING_VERSIONS_HASH=unset' Dockerfile; then echo "hash ARGs unset locally — ok"; else echo "hash ARGs must be unset locally (pinned only in CI)" >&2; exit 1; fi

echo "==> JavaScript syntax"
js_files=()
while IFS= read -r -d '' file; do
	js_files+=("${file}")
done < <(git ls-files -z '*.js')
for file in "${js_files[@]}"; do
	node --check "${file}"
done
echo "Checked ${#js_files[@]} JavaScript files"

echo "==> Committed-secret patterns"
python3 build_files/scripts/check-committed-secrets.py

echo "==> Runtime migration inventory and frontend boundaries"
python3 build_files/scripts/check-runtime-migration-inventory.py

# --fast skips the heavy 600s unittest discover on a live desktop.
# Live-desktop auto-skip: full suite is CI-gated (validation.yml). Force
# locally with --full or KYTH_FORCE_FULL_VALIDATION=1. `pre-push` defaults
# to --fast so a plain `git push` never runs the suite on Plasma.
validate_fast=0
validate_force_full=0
for _arg in "$@"; do
    case "${_arg}" in
        --fast) validate_fast=1 ;;
        --full) validate_force_full=1 ;;
    esac
done
if [[ ${validate_force_full} -eq 0 && -z "${KYTH_FORCE_FULL_VALIDATION:-}" && -z "${CI:-}" && -z "${GITHUB_ACTIONS:-}" ]]; then
    if [[ -n "${WAYLAND_DISPLAY:-}" || -n "${DISPLAY:-}" || "${XDG_CURRENT_DESKTOP:-}" == *KDE* || "${XDG_SESSION_TYPE:-}" == "wayland" ]]; then
        if [[ ${validate_fast} -eq 0 ]]; then
            echo "[validate] Live desktop detected — defaulting to --fast (skipping heavy unittest discover)."
            echo "[validate] Full suite is CI-gated; force locally with: KYTH_FORCE_FULL_VALIDATION=1 ./build_files/scripts/validate.sh --full"
            validate_fast=1
        fi
    fi
fi

echo "==> Python unit tests"
test_home="$(mktemp -d)"
trap 'rm -rf -- "${test_home}"' EXIT
# Keep the runner's installed Rust toolchain visible after HOME is isolated
# for the Python suite. On GitHub-hosted runners `cargo` is a rustup shim; if
# RUSTUP_HOME follows the temporary HOME, Rust tests fail with "no default
# toolchain" even though the workflow configured stable successfully.
rustup_home="${RUSTUP_HOME:-${HOME}/.rustup}"
cargo_home="${CARGO_HOME:-${HOME}/.cargo}"
export HOME="${test_home}/home"
export RUSTUP_HOME="${rustup_home}"
export CARGO_HOME="${cargo_home}"
export XDG_CACHE_HOME="${test_home}/cache"
export XDG_CONFIG_HOME="${test_home}/config"
export XDG_DATA_HOME="${test_home}/data"
export XDG_STATE_HOME="${test_home}/state"
mkdir -p "${HOME}" "${XDG_CACHE_HOME}" "${XDG_CONFIG_HOME}" "${XDG_DATA_HOME}" "${XDG_STATE_HOME}"
if [[ ${validate_fast} -eq 1 ]]; then
	echo "==> Python unit tests SKIPPED (--fast / live-desktop guard) — CI validation.yml gates the full suite"
else
	# Guard with timeout so CI doesn't hang on slow network/hardware probes; --foreground
	# lets the suite read from TTY and avoids timeout's process-group SIGTERM
	# killing the caller's session. 600s matches CI's 10m job timeout.
	PYTHONPATH=build_files/kyth_shared:build_files/kyth-welcome:build_files/kyth-installer timeout --foreground 600 python3 -m unittest discover -s tests -b
fi

echo "==> Structured configuration"
while IFS= read -r -d '' file; do
	[[ -f "${file}" ]] || continue
	jq empty "${file}"
done < <(git ls-files -z '*.json')
python3 build_files/scripts/validate-toml-syntax.py
"${hardware_policy_cmd[@]}" \
	--policy build_files/config/hardware-profiles.toml validate --fail-expired
hardware_matrix="${test_home}/hardware-support-matrix.md"
"${hardware_policy_cmd[@]}" \
	--policy build_files/config/hardware-profiles.toml matrix --output "${hardware_matrix}"
if ! cmp --silent "${hardware_matrix}" docs/hardware-support-matrix.md; then
	echo "Hardware support matrix is stale — docs/hardware-support-matrix.md" >&2
	echo "diff vs generated (build_files/config/hardware-profiles.toml):" >&2
	diff -u docs/hardware-support-matrix.md "${hardware_matrix}" >&2 || true
	echo "Fix: kyth-hardware-policy --policy build_files/config/hardware-profiles.toml matrix --output docs/hardware-support-matrix.md" >&2
	exit 1
fi

echo "==> systemd units"
output="$(systemd-analyze verify build_files/*.service build_files/*.timer 2>&1 || true)"
printf '%s\n' "${output}"
unexpected="$(printf '%s\n' "${output}" |
	grep -Ev \
		-e '^[^:]+: Command .+ is not executable: No such file or directory$' \
		-e '^Failed to turn off SO_PASSRIGHTS on user lookup socket, ignoring: Operation not permitted$' \
		-e '^Failed to enable SO_PASSCRED on handoff timestamp socket(, ignoring)?: Operation not permitted$' \
		-e '^ERROR: ld\.so: object .* cannot be preloaded .* ignored\.$' \
		-e '^Configuration file .* is marked world-writable\. Please remove world writability permission bits\. Proceeding anyway\.$' ||
	true)"
if [[ -n "${unexpected}" ]]; then
	printf 'Unexpected systemd verification errors:\n%s\n' "${unexpected}" >&2
	exit 1
fi
# Security audits — warn, but also enforce bash -c variable interpolation gate
# Fail only on shell-variable interpolation ($var / ${var}), not static $(cmd) subshells
if grep -rn --include="*.py" 'bash.*-c.*\$[A-Za-z_]' src/kyth_shared src/kyth-welcome src/kyth-installer 2>/dev/null | grep -v "static" | grep -v "test_" | grep -q .; then
	echo "Bash -c variable interpolation (\$var/\${var}) found — use validated python helper instead" >&2
	grep -rn --include="*.py" 'bash.*-c.*\$[A-Za-z_]' src/kyth_shared src/kyth-welcome src/kyth-installer 2>/dev/null | grep -v "static" | head -n 5 >&2
	exit 1
fi
# Non-blocking security audit — warn, don't fail (thresholds are advisory while
# the demonolith is being split). Surfaces hardening regressions early.
if command -v systemd-analyze >/dev/null 2>&1; then
	output_sec="$(systemd-analyze security build_files/kyth-ai-perfd.service build_files/kyth-guardian.service build_files/kyth-sched.service build_files/kyth-sched-arbiter.service build_files/kyth-batteryd.service build_files/kyth-probe.service build_files/kyth-probe-user.service build_files/kyth-update-watcher.service 2>&1 || true)"
	printf '%s\n' "${output_sec}" | grep -E "^(build_files|Overall exposure)" || true
fi
# Supply-chain audit — non-blocking, surfaces cargo/pip advisories
if command -v cargo >/dev/null 2>&1 && [ -f Cargo.lock ]; then
	cargo audit 2>&1 | head -n 30 || true
fi
if command -v pip-audit >/dev/null 2>&1; then
	pip-audit 2>&1 | head -n 30 || true
fi

echo "==> Just recipes"
just --list >/dev/null
while IFS= read -r -d '' file; do
	[[ -f "${file}" ]] || continue
	just --justfile "${file}" --list >/dev/null
done < <(git ls-files -z '*.just')

echo "==> Validation passed"
