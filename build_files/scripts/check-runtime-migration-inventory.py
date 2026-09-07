#!/usr/bin/env python3
"""Generate and validate Kyth's runtime migration inventory.

The inventory is intentionally source-derived.  It records aliases and nested
unit files separately because the installed executable is not always the
source file that appears in build_files.

A path-prefix rule alone cannot distinguish live Python from rollback-only
Python: every ``.py`` file under ``src/kyth_shared/`` is not automatically
an active migration task.  Modules whose entire runtime surface has a proven
native replacement carry ``superseded_by`` and are inactive; only modules
reachable — statically (including transitively) from an active
launcher/unit, via a documented dynamic-dispatch table, or via direct
shell-harness invocation — count toward ``active_python``.
"""

from __future__ import annotations

import argparse
import ast
import json
import re
import sys
from importlib.util import resolve_name
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
INVENTORY = ROOT / "build_files/config/runtime-migration-inventory.json"
REPORT = ROOT / "build_files/config/runtime-migration-report.json"
SCHEMA_VERSION = 4
REPORT_SCHEMA_VERSION = 2
STATUSES = {"done-native", "queued", "explicitly-not-ported", "not-applicable"}
RISK = {"read-only", "user-session-writer", "privileged-writer", "daemon", "destructive", "build-time"}
UNIT_SUFFIXES = {".service", ".timer", ".path"}
RUNTIME_AUTHORITIES = {
    "rust-binary", "rust-dispatcher", "rust-service", "rust-transport-python-backend",
    "rust-library", "python-installer", "python-runtime", "python-shared-package",
    "shell-orchestration", "source-only", "data-or-config", "build-only",
}
RUNTIME_SCOPES = {
    "installer", "system-hub", "system-service", "user-session", "standalone",
    "build", "test-fixture", "configuration",
}
FRONTEND_ROOTS = (
    ROOT / "src/kyth-hub-web/src",
    ROOT / "src/kyth-installer-web/src",
)
FORBIDDEN_FRONTEND_PATTERNS = (
    (re.compile(r"\b(?:PySide6|PyQt6|PyQt5|subprocess|child_process)\b"), "direct Python/process API"),
    (re.compile(r"@tauri-apps/plugin-(?:shell|fs|process)"), "unscoped Tauri shell/filesystem/process plugin"),
    (re.compile(r"window\.__TAURI__"), "direct Tauri global access"),
    (re.compile(
        r"invoke(?:\s*<[^>]+>)?\s*\(\s*['\"](?:run|exec|spawn|shell|execute)(?:[_-](?:command|process|argv)|['\"])",
    ), "generic Tauri command bridge"),
)

NATIVE_BINARIES = {
    "kyth-probe", "kyth-guardian", "kyth-update-watcher", "kyth-network-share",
    "kyth-telem", "kyth-privileged", "kyth-post-update-check",
    "kyth-firstboot-app-status", "kyth-steam-game-export", "kyth-hub-desktop-entries",
    "kyth-safe-upgrade", "kyth-bootc-guard", "kyth-finalize-staged", "kyth-btrfs-maint",
    "kyth-ai-perfd", "kyth-perf-gate-rs",
}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-doctor", "kyth-health-check", "kyth-smoke-check", "kyth-resume-check", "kyth-nvidia-status", "kyth-controller-check", "kyth-creator-check", "kyth-exe-compat", "kyth-snapshot-timeline", "kyth-print-check", "kyth-windows-verify", "kyth-tunable", "kyth-configure-session", "kyth-set-resolution", "kyth-set-kickoff-icon", "kyth-greeter-compositor", "kyth-config-apply"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-tunable-rs", "kyth-game-boost", "kyth-vm-acceptance-guest"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-scx-loader"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-runtime"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-ai-dev"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-apply-scx-preset"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-apply-desktop-layout"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-apply-display-hdr"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-apply-input"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-apply-network"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-apply-pipewire-latency"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-apply-plasma"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-apply-quicksettings"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-apply-rgb"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-apply-role-preset"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-apply-scaling"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-apply-tailscale"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-apply-vrr"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-apply-window-snap"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-driver-switch"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-kali-desktop-fixup"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-ntfs-repair"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-performance-mode"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-refresh-boot-splash-initramfs"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-refresh-taskbar-pins"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-report-issue"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-session-snapshot"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-setup-devcontainer"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-setup-transfer"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-vscode-wallet"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-web-app-categorize"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-storage-sense"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-duperemove"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-batteryd"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-cloud-mount"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-save-sync"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-backup"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-game-launch"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-dynamic-lock"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-proton-cachyos-update"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-rclone-update"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-sched", "kyth-user-polish", "kyth-exe-handler"}
NATIVE_BINARIES = NATIVE_BINARIES | {
    "kyth-apply-update", "kyth-boot-branding-guard", "kyth-boot-verify",
    "kyth-davinci-install", "kyth-device-info", "kyth-distrobox-root-launch",
    "kyth-enroll-mok", "kyth-full-update", "kyth-gamescope",
    "kyth-greenboot-failure", "kyth-greenboot-required", "kyth-greenboot-success",
    "kyth-hw-setup", "kyth-isolate-game", "kyth-kerver", "kyth-local-bin-migrate",
    "kyth-mok-rotate", "kyth-nearby-share", "kyth-nvme-tuning", "kyth-perf-gate",
    "kyth-power-arbiter", "kyth-readahead-hint", "kyth-readahead-run",
    "kyth-retry-hardware-setup", "kyth-scx", "kyth-scx-loader",
    "kyth-session-splash-guard", "kyth-set-sleep-mode", "kyth-shader-preheat",
    "kyth-shader-prune", "kyth-snappy-bench", "kyth-storage-gate",
    "kyth-vpnc-script", "kyth-windows-friendly-defaults", "kyth-windows-import",
    "kyth-default-flatpaks", "kyth-flathub-setup", "kyth-local-bin-migrate",
}
NATIVE_BINARIES = NATIVE_BINARIES | {
    "kyth-installer-shell", "kyth-installer-native", "kyth-installer-exec", "kyth-installerd",
}
PACKAGED_NATIVE_LAUNCHERS = NATIVE_BINARIES | {"kyth-launch-installer"}
NOT_PORTED = {"rclone@"}
NOT_PORTED_PATHS = {"src/kyth-welcome/kyth_welcome/services/privileged.py"}
REVIEWED_EXTERNAL_INTERFACES = {
    "rclone@": "external::rclone",
}
READ_ONLY_NAMES = {
    "kyth-doctor", "kyth-health-check", "kyth-smoke-check", "kyth-resume-check",
    "kyth-nvidia-status", "kyth-controller-check", "kyth-creator-check",
    "kyth-exe-compat", "kyth-snapshot-timeline", "kyth-print-check",
    "kyth-windows-verify",
}
WRITER_NAMES = {
    "kyth-apply-desktop-layout", "kyth-apply-role-preset", "kyth-configure-session",
    "kyth-greeter-compositor", "kyth-performance-mode", "kyth-set-kickoff-icon",
    "kyth-set-resolution", "kyth-config-apply", "kyth-exe-handler", "kyth-report-issue",
    "kyth-session-snapshot", "kyth-setup-transfer", "kyth-setup-devcontainer",
    "kyth-ntfs-repair", "kyth-kali-desktop-fixup", "kyth-refresh-boot-splash-initramfs",
    "kyth-refresh-taskbar-pins", "kyth-vscode-wallet", "kyth-web-app-categorize",
}
DAEMON_NAMES = {
    "kyth-batteryd", "kyth-backup", "kyth-save-sync", "kyth-cloud-mount", "kyth-duperemove",
    "kyth-storage-sense", "kyth-dynamic-lock", "kyth-game-boost", "kyth-game-launch",
    "kyth-sched", "kyth-sched-arbiter", "kyth-proton-cachyos-update", "kyth-rclone-update",
    "kyth-user-polish", "kyth-installerd",
}
SHELL_HELPER_LAUNCHERS = {
    "kyth-perf-report-common.sh", "kyth-report-common.sh",
}
SOURCE_ALIAS_LAUNCHERS = {"kyth-hub-web", "kyth-welcome"}
NATIVE_RECIPE_FILES = {"native.just"}
# Phase 0 reachability audit (2026-09-07): 92 kyth_shared modules whose entire
# runtime surface is superseded by the native tunable dispatcher
# (kyth-tunable-rs / tunable_registry.rs — all 94 registry aliases verified
# present, 0 missing). Excluded after a transitive static-import closure over
# the 39 queued launchers plus a shell-harness scan: sched_arbiter (imported
# by build_files/kyth-game-launch) and perf_gate (used by
# build_files/scripts/check-perf-gate.py) remain reachable and stay active.
# gaming_master/perf_audit dispatch targets overlap this same set; the two
# dispatcher files themselves are unreachable and superseded with it.
TUNABLE_SUPERSEDED_BY = "native::kyth-tunable-rs"
NATIVE_REPLACED_MODULES = {
    "ai_dev": "native::kyth-ai-dev",
    "user_polish": "native::kyth-user-polish",
    "user_polish_flatpak": "native::kyth-user-polish",
    "apps": "native::kyth-exe-handler",
    "exe_handler": "native::kyth-exe-handler",
    "qt_threads": "native::kyth-exe-handler",
    "windows_installer": "native::kyth-exe-handler",
}
SUPERSEDED_TUNABLE_MODULES = frozenset({
    "aio_max", "ananicy_preset", "boot_loader", "bore_tune", "btrfs_autotune",
    "btrfs_perf", "busy_poll", "busy_read", "compaction_tune", "dirty_expire",
    "dirty_ratio", "distrobox_cache", "epp_ac", "fcitx_latency", "file_max",
    "flatpak_prefetch", "flatpak_trim", "fscache_tune", "gaming_cfs",
    "gaming_master", "gpu_power", "hdr_per_game", "hdr_store", "inotify_watches",
    "io_tune", "irq_tune", "journal_tune", "kargs_preset", "kwin_latency",
    "max_map_count", "mimalloc_preset", "min_free_kbytes", "net_backlog",
    "net_latency", "netdev_budget", "numa_balancing", "numa_tune", "oom_gaming",
    "overcommit_memory", "overlay_tune", "page_cluster", "pcie_aspm",
    "perf_audit", "perf_cpu", "pipewire_gaming", "podman_btrfs", "psi_gaming",
    "psi_poll", "readahead_preset", "rmem_default", "rmem_max", "sccache_preset",
    "sched_autogroup", "sched_child", "sched_latency", "sched_nr_migrate",
    "selinux_gaming", "shader_cache_size", "shader_tmpfs", "somaxconn",
    "steam_deadzone", "swappiness", "system_audit", "tcp_ecn", "tcp_fastopen",
    "tcp_fin_timeout", "tcp_keepalive", "tcp_mtu_probing", "tcp_no_metrics_save",
    "tcp_notsent", "tcp_orphan_retries", "tcp_retries1", "tcp_retries2",
    "tcp_sack", "tcp_slow_start", "tcp_timestamps", "tcp_window_scaling",
    "telemetry_opt", "thp_collapse", "thp_tune", "trim_preset", "tunable",
    "uksmd_preset", "vfs_cache_pressure", "vm_stat", "vm_watermark",
    "windows_verify", "wine_sync", "wmem_default", "wmem_max", "work_cache",
    "zswap_preset",
})
# Direct shell-harness invocation channel: kyth_shared modules run straight
# from shell (build_files/scripts/*.sh, check-*.py) rather than any launcher.
# These must never be marked superseded, even if a dispatch table names them.
SHELL_HARNESS_MODULES = frozenset({
    "qualification", "memory_tune", "sysctl_compose", "network_preset",
    "snapshot_timeline", "gaming_resolve", "hardware_policy", "perf_gate",
    "sched_arbiter",
})

# Python console scripts are installed before the native image layer copies
# over any same-named Rust binaries.  Derive the surviving Python roots from
# the package metadata rather than maintaining a second hand-written list.
# This keeps a newly added console script visible until its installed entry
# point is explicitly replaced by a native binary.
PYTHON_PACKAGE = ROOT / "src/kyth_shared"
PYTHON_MODULE_ROOT = PYTHON_PACKAGE / "kyth_shared"
PYTHON_PACKAGE_METADATA = PYTHON_PACKAGE / "pyproject.toml"


def python_module_name(path: Path) -> str | None:
    """Return the import name for a shared-package source file."""
    try:
        relative = path.resolve().relative_to(PYTHON_MODULE_ROOT.resolve())
    except ValueError:
        return None
    if relative.suffix != ".py":
        return None
    parts = list(relative.with_suffix("").parts)
    if parts[-1] == "__init__":
        parts.pop()
    return ".".join(("kyth_shared", *parts))


def python_module_paths() -> dict[str, Path]:
    """Index importable shared-package modules without importing user code."""
    result: dict[str, Path] = {}
    for path in sorted(PYTHON_PACKAGE.rglob("*.py")):
        name = python_module_name(path)
        if name is not None:
            result[name] = path
    return result


def _relative_import_name(current: str, path: Path, level: int, module: str | None) -> str | None:
    if level == 0:
        return module
    package = current if path.name == "__init__.py" else current.rpartition(".")[0]
    if not package:
        return None
    try:
        return resolve_name("." * level + (module or ""), package)
    except (ImportError, ValueError):
        return None


def _imports_from_module(path: Path, current: str, known: dict[str, Path]) -> set[str]:
    """Collect statically resolvable imports from one package module."""
    try:
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    except (OSError, SyntaxError):
        return set()
    imports: set[str] = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                candidate = alias.name
                # An import of a package may resolve to its __init__.py; trim
                # unavailable trailing names until the package index matches.
                while candidate and candidate not in known:
                    candidate = candidate.rpartition(".")[0]
                if candidate in known:
                    imports.add(candidate)
        elif isinstance(node, ast.ImportFrom):
            base = _relative_import_name(current, path, node.level, node.module)
            if base and base in known:
                imports.add(base)
            for alias in node.names:
                if alias.name == "*":
                    continue
                candidate = f"{base}.{alias.name}" if base else alias.name
                if candidate in known:
                    imports.add(candidate)
    # The catalog deliberately imports one module per managed hardware quirk
    # through importlib.  Its tuple is the documented dynamic-dispatch table;
    # include all of the table's source modules when the catalog is reachable.
    if current in {"kyth_shared.hardware_quirks", "kyth_shared.hardware_quirks.catalog"}:
        imports.update(
            name for name in known
            if name.startswith("kyth_shared.hardware_quirks.")
            and name.rpartition(".")[-1] not in {"__init__", "catalog"}
        )
    return imports


def _python_console_roots() -> set[str]:
    """Find Python entry-point modules that remain installed after cutover."""
    try:
        import tomllib

        with PYTHON_PACKAGE_METADATA.open("rb") as stream:
            metadata = tomllib.load(stream)
    except (OSError, tomllib.TOMLDecodeError):
        return set()
    scripts = metadata.get("project", {}).get("scripts", {})
    roots: set[str] = set()
    for command, target in scripts.items():
        if command in PACKAGED_NATIVE_LAUNCHERS:
            continue
        module = str(target).partition(":")[0]
        if module:
            roots.add(module)
    return roots


def _python_harness_roots() -> set[str]:
    """Find direct shared-package imports in active build/test harnesses.

    These are intentionally separate from console-script roots.  VM
    acceptance, validation, image assembly, and performance gates can invoke
    a module directly even when no installed launcher points at it.
    """
    roots: set[str] = set()
    search_roots = (ROOT / "build_files/scripts",)
    for search_root in search_roots:
        if not search_root.exists():
            continue
        paths = search_root.rglob("*") if search_root.is_dir() else ()
        for path in paths:
            if not path.is_file() or path.suffix not in {".py", ".sh", ".bash", ""}:
                continue
            try:
                text = path.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            for match in re.finditer(r"(?:python3?|python)\s+(?:[^\n]*?\s)?-m\s+kyth_shared(?:\.([A-Za-z0-9_.]+))?", text):
                if match.group(1):
                    roots.add(f"kyth_shared.{match.group(1)}")
            for match in re.finditer(r"(?:from|import)\s+kyth_shared\.([A-Za-z0-9_.]+)", text):
                roots.add(f"kyth_shared.{match.group(1)}")
    # These are explicit, direct harness channels even when formatting or a
    # shell continuation prevents the simple patterns above from matching.
    roots.update(f"kyth_shared.{name}" for name in SHELL_HARNESS_MODULES)
    return roots


def python_reachable_modules() -> set[str]:
    """Return the transitive runtime/build reachability closure.

    The result is deliberately source-derived and conservative: an import
    that cannot be resolved is ignored, but documented dynamic dispatch tables
    and direct shell harnesses are added explicitly.  Unreachable package
    files are retained in the tree as compatibility fixtures and removed from
    the migration queue.
    """
    known = python_module_paths()
    pending = [
        module for module in (*_python_console_roots(), *_python_harness_roots())
        if module in known
    ]
    reachable: set[str] = set()
    while pending:
        current = pending.pop()
        if current in reachable:
            continue
        reachable.add(current)
        for imported in _imports_from_module(known[current], current, known):
            if imported not in reachable:
                pending.append(imported)
    return reachable


def rel(path: Path) -> str:
    return path.relative_to(ROOT).as_posix()


def launcher_kind(path: Path) -> str:
    if path.is_symlink():
        return "alias"
    try:
        first = path.read_text(encoding="utf-8", errors="replace").splitlines()[0]
    except (OSError, IndexError):
        return "data"
    if "python" in first:
        return "python"
    if "bash" in first or "/bin/sh" in first:
        return "shell"
    return "data"


def name_for(path: Path) -> str:
    return path.name.removesuffix(".service").removesuffix(".timer").removesuffix(".path")


def shell_function_names(path: Path, surface: str, exec_start: list[str] | None = None) -> list[str]:
    """Return stable function/recipe/command identifiers for shell ownership.

    The migration ledger is file-based for compatibility with existing release
    tooling, but a file can contain several independent runtime operations.
    These identifiers let parity tests and later Rust cutovers name the actual
    behavior being replaced instead of treating a thin wrapper as authority.
    """
    if surface == "systemd-unit":
        commands = exec_start or []
        return [f"ExecStart:{command}" for command in commands] or [f"Unit:{path.name}"]

    text = path.read_text(encoding="utf-8", errors="replace") if path.is_file() else ""
    if surface == "ujust-recipe":
        names = re.findall(r"^\s*([A-Za-z_][A-Za-z0-9_-]*)\s*(?:\([^)]*\))?\s*:", text, re.MULTILINE)
        return list(dict.fromkeys(names)) or ["<recipe-file>"]

    names = re.findall(
        r"^\s*(?:function\s+)?([A-Za-z_][A-Za-z0-9_-]*)\s*(?:\(\s*\))?\s*\{",
        text,
        re.MULTILINE,
    )
    if names:
        return list(dict.fromkeys(names))
    if surface in {"launcher", "shell-script"}:
        return ["__main__"]
    return []


def function_inventory(
    path: Path,
    *,
    surface: str,
    status: str,
    owner: str,
    exec_start: list[str] | None = None,
) -> list[dict[str, str]]:
    """Describe the ownership status of each shell-level operation."""
    if surface not in {"launcher", "shell-script", "ujust-recipe", "systemd-unit"}:
        return []
    if status == "done-native":
        ownership = "native"
    elif status == "not-applicable":
        ownership = "build-only"
    elif status == "explicitly-not-ported":
        ownership = "external" if owner.startswith("external::") else "exception"
    else:
        ownership = "shell"
    return [
        {"name": name, "owner": owner, "ownership": ownership}
        for name in shell_function_names(path, surface, exec_start)
    ]


def risk_for(name: str, kind: str, path: Path) -> str:
    if "/scripts/" in f"/{rel(path)}" or path.parts[:2] == ("build_files", "scripts"):
        return "build-time"
    if name in {"kyth-vm-acceptance-guest", "kyth-scx-loader"}:
        return "destructive"
    if name in READ_ONLY_NAMES or name in NATIVE_BINARIES and name not in {"kyth-network-share"}:
        return "read-only"
    if name in WRITER_NAMES:
        return "user-session-writer"
    if name in DAEMON_NAMES:
        return "destructive" if name in {"kyth-installerd"} else "daemon"
    if "privileged" in name or name in {"kyth-ntfs-repair", "kyth-refresh-boot-splash-initramfs"}:
        return "privileged-writer"
    if kind in {"python", "shell", "alias"}:
        return "user-session-writer"
    return "build-time"


def runtime_metadata(
    path: Path,
    *,
    surface: str,
    name: str,
    kind: str,
    status: str,
    exec_start: list[str] | None = None,
) -> dict[str, object]:
    """Classify the installed authority separately from the source file.

    A Python source file can be a retired fixture (the old Hub), a Python
    package still installed for compatibility, or a source counterpart whose
    installed entry point is already native Rust.  The original inventory had
    no way to distinguish those cases, which made the migration queue look
    much larger than the actual runtime surface.
    """
    authority = "build-only"
    scope = "build"
    active = False
    priority = 3

    if surface == "installer-runtime":
        authority, scope, active, priority = "source-only", "test-fixture", False, 3
    elif surface == "python-runtime":
        if rel(path).startswith("src/kyth-welcome/"):
            authority, scope, active, priority = "source-only", "test-fixture", False, 3
        elif name in NATIVE_REPLACED_MODULES:
            authority, scope, active, priority = "python-shared-package", "test-fixture", False, 3
        else:
            authority, scope, active, priority = "python-shared-package", "standalone", True, 2
    elif surface == "rust-crate":
        authority, scope, active, priority = "rust-library", "build", False, 3
    elif surface == "ujust-recipe":
        authority, scope, active, priority = "shell-orchestration", "user-session", True, 2
    elif surface == "shell-script":
        authority, scope, active, priority = "build-only", "build", False, 3
    elif surface == "systemd-unit":
        commands = " ".join(exec_start or [])
        if "kyth-installerd" in commands:
            authority, scope, active, priority = "rust-service", "installer", True, 0
        elif any(binary in commands for binary in NATIVE_BINARIES):
            authority, scope, active, priority = "rust-service", "system-service", True, 1
        elif "python" in commands:
            authority, scope, active, priority = "python-runtime", "system-service", True, 1
        else:
            authority, scope, active, priority = "shell-orchestration", "system-service", True, 2
    elif surface == "launcher":
        scope = "user-session"
        active = True
        if name in SOURCE_ALIAS_LAUNCHERS:
            authority, scope, active, priority = "source-only", "test-fixture", False, 3
        elif name in SHELL_HELPER_LAUNCHERS:
            authority, scope, active, priority = "build-only", "build", False, 3
        elif name == "kyth-installer":
            authority, scope, active, priority = "source-only", "test-fixture", False, 3
        elif name == "kyth-launch-installer":
            authority, priority = "shell-orchestration", 0
        elif name in NATIVE_BINARIES:
            authority, priority = "rust-dispatcher", 1
        elif kind == "python":
            authority, priority = "python-runtime", 2
        elif kind in {"shell", "alias"}:
            authority, priority = "shell-orchestration", 2
        else:
            # Data/config files are not code: terminal state, never queued work.
            authority, scope, active, priority = "data-or-config", "configuration", False, 3

    if status == "explicitly-not-ported" and authority != "source-only":
        # Third-party/declarative exceptions are not active migration targets.
        active = False
        priority = 3

    return {
        "runtime_authority": authority,
        "runtime_scope": scope,
        "runtime_active": active,
        "migration_priority": priority,
    }


def entry(
    path: Path,
    *,
    surface: str,
    implementation: str | None = None,
    name: str | None = None,
    reachable_python_modules: set[str] | None = None,
) -> dict:
    item_name = name or name_for(path)
    kind = implementation or launcher_kind(path)
    is_tunable_alias = surface == "launcher" and path.is_symlink() and path.resolve() == ROOT / "build_files/kyth-tunable"
    if surface == "shell-script":
        status = "not-applicable"
        reason = "build/test shell script, not installed runtime authority"
    elif surface == "launcher" and item_name in SHELL_HELPER_LAUNCHERS:
        status = "not-applicable"
        reason = "sourceable diagnostic helper; runtime authority is the Rust report dispatcher"
    elif surface == "launcher" and kind == "data":
        status = "not-applicable"
        reason = "data or config file, not migratable code"
    elif surface == "ujust-recipe" and path.name in NATIVE_RECIPE_FILES:
        status = "done-native"
        reason = "installed recipe manifest delegates every recipe to kyth-runtime"
    elif item_name in NOT_PORTED or rel(path) in NOT_PORTED_PATHS:
        status = "explicitly-not-ported"
        reason = "documented third-party or declarative build/runtime exception"
    elif (
        implementation == "rust"
        or (
            surface == "systemd-unit"
            and any(binary in path.read_text(encoding="utf-8", errors="replace") for binary in NATIVE_BINARIES)
        )
        or (surface == "launcher" and item_name in PACKAGED_NATIVE_LAUNCHERS)
        or is_tunable_alias
    ):
        status = "done-native"
        reason = "native Rust crate or installed unit is already declared/packaged"
    else:
        status = "queued"
        reason = None
    owner = (
        f"fixture::{rel(path)}" if status in {"queued", "explicitly-not-ported", "not-applicable"}
        else "native::kyth-runtime" if surface == "ujust-recipe" and path.name in NATIVE_RECIPE_FILES
        else "native::kyth-tunable-rs" if is_tunable_alias
        else f"native::{item_name}"
    )
    result = {
        "path": rel(path),
        "surface": surface,
        "name": item_name,
        "resolved_target": rel(path.resolve()) if path.exists() else None,
        "current_implementation": kind,
        "installed_implementation": (
            "native-launcher" if item_name == "kyth-launch-installer" and status == "done-native" else
            "rust" if status == "done-native" else
            "not-installed" if status in {"explicitly-not-ported", "not-applicable"} else kind
        ),
        "status": status,
        "risk_tier": risk_for(item_name, kind, path),
        "priority": 0 if status != "queued" else 1,
        "owner": owner,
        "parity_tests": ["tests/"],
        "cutover": f"replace installed {item_name} entry point after parity gates",
        "rollback": f"restore previous installed {item_name} entry point",
        "retirement": "retain source fixture until exact-image acceptance and rollback qualification",
        "function_inventory": [],
        **({"reason": reason} if reason else {}),
    }
    metadata = runtime_metadata(path, surface=surface, name=item_name, kind=kind, status=status)
    result.update(metadata)
    if surface == "python-runtime" and item_name in NATIVE_REPLACED_MODULES:
        result.update({
            "runtime_active": False,
            "migration_priority": 3,
            "installed_implementation": "python-fixture",
            "runtime_scope": "test-fixture",
            "status": "explicitly-not-ported",
            "superseded_by": NATIVE_REPLACED_MODULES[item_name],
            "owner": NATIVE_REPLACED_MODULES[item_name],
            "reason": "retained shared-package compatibility module is superseded by a packaged native Rust owner",
        })
    if (
        surface == "python-runtime"
        and rel(path) == f"src/kyth_shared/kyth_shared/{item_name}.py"
        and item_name in SUPERSEDED_TUNABLE_MODULES
    ):
        # Proven native replacement: rollback fixture only, not active work.
        # Reachability guard — shell-harness modules must never land here.
        assert item_name not in SHELL_HARNESS_MODULES, f"reachable module marked superseded: {item_name}"
        result.update({
            "runtime_active": False,
            "migration_priority": 3,
            "superseded_by": TUNABLE_SUPERSEDED_BY,
        })
    module_name = python_module_name(path) if surface == "python-runtime" else None
    if (
        module_name
        and reachable_python_modules is not None
        and module_name not in reachable_python_modules
        and not result.get("superseded_by")
    ):
        # The package is installed for compatibility and rollback tooling, but
        # this module is not reachable from an installed Python entry point or
        # an active shell/build harness.  Keep the source in the tree without
        # presenting it as an open Rust migration target.
        result.update({
            "status": "explicitly-not-ported",
            "installed_implementation": "python-fixture",
            "runtime_authority": "source-only",
            "runtime_scope": "test-fixture",
            "runtime_active": False,
            "migration_priority": 3,
            "owner": f"fixture::{rel(path)}",
            "reason": "retained shared-package compatibility module is unreachable from active entry points and harnesses",
        })
    if metadata["runtime_authority"] == "source-only":
        result.update({
            "status": "explicitly-not-ported",
            "installed_implementation": "not-installed",
            "owner": f"fixture::{rel(path)}",
            "reason": "retired Python/Qt Hub source retained only for compatibility fixtures; not installed in the supported image",
        })
    if item_name in REVIEWED_EXTERNAL_INTERFACES and result["status"] == "explicitly-not-ported":
        result.update({
            "owner": REVIEWED_EXTERNAL_INTERFACES[item_name],
            "reason": "reviewed external interface: upstream rclone owns the mount lifecycle; Kyth owns the unit contract",
        })
    result["function_inventory"] = function_inventory(
        path,
        surface=surface,
        status=result["status"],
        owner=result["owner"],
    )
    return result


def discover() -> list[dict]:
    items: list[dict] = []
    reachable_python_modules = python_reachable_modules()
    for path in sorted(ROOT.glob("build_files/kyth-*")):
        if path.suffix not in UNIT_SUFFIXES:
            items.append(entry(path, surface="launcher"))
    for path in sorted((ROOT / "build_files").rglob("*")):
        if path.is_file() and path.suffix in UNIT_SUFFIXES:
            unit = entry(path, surface="systemd-unit")
            text = "\n".join(
                line for line in path.read_text(encoding="utf-8", errors="replace").splitlines()
                if not line.lstrip().startswith("//")
            )
            execs = re.findall(r"^Exec(?:Start|Condition|Stop)=([^\n]+)", text, re.MULTILINE)
            unit["exec_start"] = execs
            uses_native = any(
                re.search(rf"(?<![A-Za-z0-9_-]){re.escape(binary)}(?=\s|$)", " ".join(execs))
                for binary in NATIVE_BINARIES
                if binary != "kyth-vm-acceptance-guest"
            )
            timer_for_native = (
                not execs
                and path.name.endswith((".timer", ".path"))
                and any(
                    candidate.get("path", "").endswith(f"{path.stem}.service")
                    and candidate.get("status") == "done-native"
                    for candidate in items
                )
            )
            if uses_native or timer_for_native:
                native_owner = next(
                    (
                        binary
                        for binary in sorted(NATIVE_BINARIES)
                        if binary != "kyth-vm-acceptance-guest"
                        and re.search(
                            r"(?<![A-Za-z0-9_-])" + re.escape(binary) + r"(?=\s|$)",
                            " ".join(execs),
                        )
                    ),
                    unit["name"],
                )
                unit.update({
                    "status": "done-native",
                    "reason": "unit delegates to an installed Rust owner",
                    "owner": f"native::{native_owner}",
                    "installed_implementation": "rust",
                })
            if any("kyth-privileged" in command or "kyth-installerd" in command for command in execs):
                unit["risk_tier"] = "privileged-writer" if "installerd" not in path.name else "destructive"
            unit.update(runtime_metadata(
                path,
                surface="systemd-unit",
                name=unit["name"],
                kind=unit["current_implementation"],
                status=unit["status"],
                exec_start=execs,
            ))
            unit["function_inventory"] = function_inventory(
                path,
                surface="systemd-unit",
                status=unit["status"],
                owner=unit["owner"],
                exec_start=execs,
            )
            if unit["status"] == "done-native":
                for function, command in zip(unit["function_inventory"], execs):
                    function["owner"] = next(
                        (
                            f"native::{binary}"
                            for binary in sorted(NATIVE_BINARIES)
                            if binary != "kyth-vm-acceptance-guest"
                            and re.search(rf"(?<![A-Za-z0-9_-]){re.escape(binary)}(?=\s|$)", command)
                        ),
                        unit["owner"],
                    )
            items.append(unit)
    for root, surface in ((ROOT / "src/kyth_shared", "python-runtime"), (ROOT / "src/kyth-welcome", "python-runtime"), (ROOT / "src/kyth-installer", "installer-runtime")):
        for path in sorted(root.rglob("*.py")):
            items.append(entry(
                path,
                surface=surface,
                implementation="python",
                name=path.stem,
                reachable_python_modules=reachable_python_modules,
            ))
    active_just_paths = active_just_imports()
    for path in sorted((ROOT / "build_files/just/kyth").rglob("*.just")):
        item = entry(path, surface="ujust-recipe", implementation="recipe", name=path.stem)
        if path not in active_just_paths and path.name not in NATIVE_RECIPE_FILES:
            item.update({
                "status": "not-applicable",
                "installed_implementation": "not-installed",
                "owner": f"fixture::{rel(path)}",
                "reason": "retained recipe parity fixture is not imported by the installed Rust recipe manifest",
                "runtime_authority": "build-only",
                "runtime_scope": "build",
                "runtime_active": False,
                "migration_priority": 3,
                "function_inventory": function_inventory(
                    path,
                    surface="ujust-recipe",
                    status="not-applicable",
                    owner=f"fixture::{rel(path)}",
                ),
            })
        items.append(item)
    for path in sorted((ROOT / "build_files/scripts").rglob("*")):
        if path.is_file() and (path.suffix in {".sh", ".bash"} or launcher_kind(path) == "shell"):
            items.append(entry(path, surface="shell-script", implementation="shell"))
    for manifest in (ROOT / "src/kyth-shared-rs/Cargo.toml", ROOT / "src/kyth-hub-web/src-tauri/Cargo.toml", ROOT / "src/kyth-installer-web/src-tauri/Cargo.toml"):
        if manifest.exists():
            items.append(entry(manifest, surface="rust-crate", implementation="rust", name=manifest.parent.name))
    return sorted(items, key=lambda item: item["path"])


def active_just_imports() -> set[Path]:
    """Resolve the justfile import graph used by the installed image."""
    root = ROOT / "build_files/just/kyth.just"
    active: set[Path] = set()
    pending = [root]
    while pending:
        current = pending.pop()
        if current in active or not current.exists():
            continue
        active.add(current)
        text = current.read_text(encoding="utf-8", errors="replace")
        for raw in re.findall(r"import\??\s+['\"]([^'\"]+)['\"]", text):
            target = (current.parent / raw).resolve()
            if target.suffix == ".just":
                pending.append(target)
    return active


def validate(document: dict, *, expected_paths: set[str] | None = None) -> list[str]:
    errors: list[str] = []
    if document.get("schema_version") != SCHEMA_VERSION:
        errors.append("unsupported schema_version")
    entries = document.get("entries")
    if not isinstance(entries, list) or not entries:
        return ["entries must be a non-empty list"]
    seen: set[str] = set()
    required = {
        "path", "surface", "resolved_target", "current_implementation", "installed_implementation",
        "status", "risk_tier", "priority", "owner", "parity_tests", "cutover", "rollback",
        "retirement", "runtime_authority", "runtime_scope", "runtime_active", "migration_priority",
        "function_inventory",
    }
    for index, item in enumerate(entries):
        if not isinstance(item, dict):
            errors.append(f"entry {index} is not an object")
            continue
        missing = required - item.keys()
        if missing:
            errors.append(f"entry {index} missing {', '.join(sorted(missing))}")
        path = item.get("path")
        if not isinstance(path, str) or not path or path in seen:
            errors.append(f"entry {index} has missing or duplicate path: {path!r}")
        else:
            seen.add(path)
            source = ROOT / path
            if not source.exists() and not source.is_symlink():
                errors.append(f"entry {path} does not exist")
            if source.is_symlink() and not source.exists():
                errors.append(f"entry {path} is a broken symlink")
        if item.get("status") not in STATUSES:
            errors.append(f"entry {path} has invalid status")
        if item.get("risk_tier") not in RISK:
            errors.append(f"entry {path} has invalid risk_tier")
        if item.get("runtime_authority") not in RUNTIME_AUTHORITIES:
            errors.append(f"entry {path} has invalid runtime_authority")
        if item.get("runtime_scope") not in RUNTIME_SCOPES:
            errors.append(f"entry {path} has invalid runtime_scope")
        if not isinstance(item.get("runtime_active"), bool):
            errors.append(f"entry {path} has invalid runtime_active")
        if not isinstance(item.get("migration_priority"), int) or not 0 <= item["migration_priority"] <= 3:
            errors.append(f"entry {path} has invalid migration_priority")
        if not isinstance(item.get("priority"), int) or item["priority"] < 0:
            errors.append(f"entry {path} has invalid priority")
        for field in ("owner", "cutover", "rollback", "retirement"):
            if not isinstance(item.get(field), str) or not item[field].strip():
                errors.append(f"entry {path} has empty {field}")
        if not isinstance(item.get("parity_tests"), list) or not item["parity_tests"]:
            errors.append(f"entry {path} has no parity_tests")
        if not isinstance(item.get("function_inventory"), list):
            errors.append(f"entry {path} has invalid function_inventory")
        else:
            for function in item["function_inventory"]:
                if not isinstance(function, dict) or not {
                    "name", "owner", "ownership"
                } <= function.keys():
                    errors.append(f"entry {path} has malformed function_inventory")
            if item.get("runtime_authority") == "shell-orchestration" and item.get("status") == "done-native":
                if any(function.get("ownership") != "native" for function in item["function_inventory"]):
                    errors.append(f"native shell entry {path} retains shell-owned functions")
            if item.get("runtime_authority") == "shell-orchestration" and item.get("runtime_active"):
                if not item["function_inventory"]:
                    errors.append(f"active shell entry {path} has no function inventory")
        if item.get("status") == "explicitly-not-ported" and not item.get("reason"):
            errors.append(f"entry {path} needs a reason")
        if item.get("status") == "not-applicable":
            if item.get("runtime_active"):
                errors.append(f"entry {path} is not-applicable but active")
            if item.get("runtime_authority") not in {"data-or-config", "build-only"}:
                errors.append(f"entry {path} is not-applicable but not data-or-config/build-only")
        superseded_by = item.get("superseded_by")
        if superseded_by is not None and (not isinstance(superseded_by, str) or not superseded_by.strip()):
            errors.append(f"entry {path} has empty superseded_by")
        if item.get("runtime_authority") == "python-shared-package" and not item.get("runtime_active"):
            if not superseded_by:
                errors.append(f"entry {path} is inactive without a native owner")
    if expected_paths is not None:
        missing = expected_paths - seen
        extra = seen - expected_paths
        errors.extend(f"missing discovered path {path}" for path in sorted(missing))
        errors.extend(f"stale inventory path {path}" for path in sorted(extra))
    return errors


def report(document: dict) -> dict:
    entries = document["entries"]
    active = [item for item in entries if item.get("runtime_active")]
    active_python = [
        item for item in active
        if str(item.get("runtime_authority", "")).startswith("python")
        or item.get("runtime_authority") == "rust-transport-python-backend"
    ]
    p0_open = [
        item for item in active
        if item.get("migration_priority") == 0 and item.get("status") != "done-native"
    ]

    def counts(field: str, rows: list[dict]) -> dict[str, int]:
        result: dict[str, int] = {}
        for item in rows:
            key = str(item.get(field, "unknown"))
            result[key] = result.get(key, 0) + 1
        return dict(sorted(result.items()))

    def compact(item: dict) -> dict:
        base = {
            "path": item["path"],
            "name": item.get("name"),
            "runtime_authority": item["runtime_authority"],
            "runtime_scope": item["runtime_scope"],
            "status": item["status"],
            "migration_priority": item["migration_priority"],
            "owner": item["owner"],
        }
        if item.get("superseded_by"):
            base["superseded_by"] = item["superseded_by"]
        return base

    return {
        "schema_version": REPORT_SCHEMA_VERSION,
        "inventory_schema_version": document.get("schema_version"),
        "generated_from": "build_files/config/runtime-migration-inventory.json",
        "summary": {
            "entries": len(entries),
            "active_entries": len(active),
            "active_python_entries": len(active_python),
            "p0_open_entries": len(p0_open),
            "superseded_entries": sum(1 for item in entries if item.get("superseded_by")),
            "active_by_authority": counts("runtime_authority", active),
            "active_by_scope": counts("runtime_scope", active),
            "status_by_active_entry": counts("status", active),
        },
        "p0_open": [compact(item) for item in sorted(p0_open, key=lambda row: row["path"])],
        "active_python": [compact(item) for item in sorted(active_python, key=lambda row: row["path"])],
        "superseded": [
            compact(item) for item in sorted(
                (item for item in entries if item.get("superseded_by")),
                key=lambda row: row["path"],
            )
        ],
        "source_only": [
            compact(item) for item in sorted(
                (item for item in entries if item.get("runtime_authority") == "source-only"),
                key=lambda row: row["path"],
            )
        ],
    }


def boundary_errors(document: dict) -> list[str]:
    """Reject new untyped frontend bridges and unclassified Python paths."""
    errors: list[str] = []
    for root in FRONTEND_ROOTS:
        if not root.exists():
            continue
        for path in sorted(root.rglob("*")):
            if path.suffix not in {".ts", ".tsx", ".js", ".jsx"}:
                continue
            text = "\n".join(
                line
                for line in path.read_text(encoding="utf-8", errors="replace").splitlines()
                if not line.lstrip().startswith("//")
            )
            for pattern, label in FORBIDDEN_FRONTEND_PATTERNS:
                if pattern.search(text):
                    errors.append(f"frontend boundary violation in {rel(path)}: {label}")

    for item in document.get("entries", []):
        authority = item.get("runtime_authority")
        if not item.get("runtime_active") or not str(authority).startswith("python"):
            continue
        if item.get("surface") not in {"launcher", "systemd-unit", "python-runtime", "installer-runtime"}:
            errors.append(f"active Python path has unsupported surface: {item.get('path')}")
        if not str(item.get("owner", "")).startswith("fixture::"):
            errors.append(f"active Python path needs an explicit fixture owner: {item.get('path')}")
        if not item.get("retirement"):
            errors.append(f"active Python path needs a retirement condition: {item.get('path')}")
    return errors


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--generate", action="store_true", help="regenerate the inventory from the checkout")
    parser.add_argument("--report", action="store_true", help="regenerate the active-runtime report")
    parser.add_argument("--inventory", type=Path, default=INVENTORY)
    args = parser.parse_args(argv)
    path = args.inventory if args.inventory.is_absolute() else ROOT / args.inventory
    if args.generate:
        document = {"schema_version": SCHEMA_VERSION, "generated_from": "checkout discovery", "entries": discover()}
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        print(f"runtime inventory: cannot read {path}: {exc}", file=sys.stderr)
        return 1
    errors = validate(document, expected_paths={item["path"] for item in discover()})
    errors.extend(boundary_errors(document))
    if errors:
        for error in errors:
            print(f"runtime inventory: {error}", file=sys.stderr)
        return 1
    generated_report = report(document)
    if args.generate or args.report:
        REPORT.parent.mkdir(parents=True, exist_ok=True)
        REPORT.write_text(json.dumps(generated_report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    elif REPORT.exists():
        try:
            checked_report = json.loads(REPORT.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            print(f"runtime migration report: cannot read {REPORT}: {exc}", file=sys.stderr)
            return 1
        if checked_report != generated_report:
            print(
                f"runtime migration report is stale: regenerate with {sys.argv[0]} --report",
                file=sys.stderr,
            )
            return 1
    print(f"runtime inventory: valid ({len(document['entries'])} entries)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
