#!/usr/bin/env python3
"""Generate and validate Kyth's runtime migration inventory.

The inventory is intentionally source-derived.  It records aliases and nested
unit files separately because the installed executable is not always the
source file that appears in build_files.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
INVENTORY = ROOT / "build_files/config/runtime-migration-inventory.json"
REPORT = ROOT / "build_files/config/runtime-migration-report.json"
SCHEMA_VERSION = 2
REPORT_SCHEMA_VERSION = 1
STATUSES = {"done-native", "queued", "explicitly-not-ported"}
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
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-tunable-rs", "kyth-game-boost"}
NATIVE_BINARIES = NATIVE_BINARIES | {"kyth-apply-scx-preset"}
NATIVE_BINARIES = NATIVE_BINARIES | {
    "kyth-installer-shell", "kyth-installer-native", "kyth-installer-exec", "kyth-installerd",
}
PACKAGED_NATIVE_LAUNCHERS = NATIVE_BINARIES | {"kyth-launch-installer"}
NOT_PORTED = {"kyth-default-flatpaks", "kyth-flathub-setup", "kyth-local-bin-migrate", "rclone@", "scx_loader"}
NOT_PORTED_PATHS = {"src/kyth-welcome/kyth_welcome/services/privileged.py"}
READ_ONLY_NAMES = {
    "kyth-doctor", "kyth-health-check", "kyth-smoke-check", "kyth-resume-check",
    "kyth-nvidia-status", "kyth-controller-check", "kyth-creator-check",
    "kyth-exe-compat", "kyth-snapshot-timeline", "kyth-print-check",
    "kyth-windows-verify", "kyth-vm-acceptance-guest",
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


def risk_for(name: str, kind: str, path: Path) -> str:
    if "/scripts/" in f"/{rel(path)}" or path.parts[:2] == ("build_files", "scripts"):
        return "build-time"
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
        else:
            authority, scope, active, priority = "python-shared-package", "standalone", True, 2
    elif surface == "rust-crate":
        authority, scope, active, priority = "rust-library", "build", False, 3
    elif surface == "ujust-recipe":
        authority, scope, active, priority = "shell-orchestration", "user-session", True, 2
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
        if name == "kyth-installer":
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
            authority, priority = "data-or-config", 3

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


def entry(path: Path, *, surface: str, implementation: str | None = None, name: str | None = None) -> dict:
    item_name = name or name_for(path)
    kind = implementation or launcher_kind(path)
    is_tunable_alias = surface == "launcher" and path.is_symlink() and path.resolve() == ROOT / "build_files/kyth-tunable"
    if item_name in NOT_PORTED or rel(path) in NOT_PORTED_PATHS:
        status = "explicitly-not-ported"
        reason = "documented third-party or declarative build/runtime exception"
    elif (
        implementation == "rust"
        or (surface == "systemd-unit" and item_name in NATIVE_BINARIES)
        or (surface == "launcher" and item_name in PACKAGED_NATIVE_LAUNCHERS)
        or is_tunable_alias
    ):
        status = "done-native"
        reason = "native Rust crate or installed unit is already declared/packaged"
    else:
        status = "queued"
        reason = None
    owner = (
        f"fixture::{rel(path)}" if status in {"queued", "explicitly-not-ported"}
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
            "not-installed" if status == "explicitly-not-ported" else kind
        ),
        "status": status,
        "risk_tier": risk_for(item_name, kind, path),
        "priority": 0 if status != "queued" else 1,
        "owner": owner,
        "parity_tests": ["tests/"],
        "cutover": f"replace installed {item_name} entry point after parity gates",
        "rollback": f"restore previous installed {item_name} entry point",
        "retirement": "retain source fixture until exact-image acceptance and rollback qualification",
        **({"reason": reason} if reason else {}),
    }
    metadata = runtime_metadata(path, surface=surface, name=item_name, kind=kind, status=status)
    if metadata["runtime_authority"] == "source-only":
        result.update({
            "status": "explicitly-not-ported",
            "installed_implementation": "not-installed",
            "owner": f"fixture::{rel(path)}",
            "reason": "retired Python/Qt Hub source retained only for compatibility fixtures; not installed in the supported image",
        })
    result.update(metadata)
    return result


def discover() -> list[dict]:
    items: list[dict] = []
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
            items.append(unit)
    for root, surface in ((ROOT / "src/kyth_shared", "python-runtime"), (ROOT / "src/kyth-welcome", "python-runtime"), (ROOT / "src/kyth-installer", "installer-runtime")):
        for path in sorted(root.rglob("*.py")):
            items.append(entry(path, surface=surface, implementation="python", name=path.stem))
    for path in sorted((ROOT / "build_files/just/kyth").glob("*.just")):
        items.append(entry(path, surface="ujust-recipe", implementation="recipe", name=path.stem))
    for manifest in (ROOT / "src/kyth-shared-rs/Cargo.toml", ROOT / "src/kyth-hub-web/src-tauri/Cargo.toml", ROOT / "src/kyth-installer-web/src-tauri/Cargo.toml"):
        if manifest.exists():
            items.append(entry(manifest, surface="rust-crate", implementation="rust", name=manifest.parent.name))
    return sorted(items, key=lambda item: item["path"])


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
        if item.get("status") == "explicitly-not-ported" and not item.get("reason"):
            errors.append(f"entry {path} needs a reason")
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
        return {
            "path": item["path"],
            "name": item.get("name"),
            "runtime_authority": item["runtime_authority"],
            "runtime_scope": item["runtime_scope"],
            "status": item["status"],
            "migration_priority": item["migration_priority"],
            "owner": item["owner"],
        }

    return {
        "schema_version": REPORT_SCHEMA_VERSION,
        "inventory_schema_version": document.get("schema_version"),
        "generated_from": "build_files/config/runtime-migration-inventory.json",
        "summary": {
            "entries": len(entries),
            "active_entries": len(active),
            "active_python_entries": len(active_python),
            "p0_open_entries": len(p0_open),
            "active_by_authority": counts("runtime_authority", active),
            "active_by_scope": counts("runtime_scope", active),
            "status_by_active_entry": counts("status", active),
        },
        "p0_open": [compact(item) for item in sorted(p0_open, key=lambda row: row["path"])],
        "active_python": [compact(item) for item in sorted(active_python, key=lambda row: row["path"])],
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
