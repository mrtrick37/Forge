#!/usr/bin/env python3
"""Generate and validate the per-recipe Rust migration ledger.

The supported ``ujust`` recipe surface is intentionally kept in
``build_files/just/kyth/native.just`` while individual operations move into
Rust.  The existing runtime migration inventory records that file as one
entry, so it cannot show which recipes have a Rust owner.  This checker makes
that boundary explicit and source-derived:

* recipe names and manifest line numbers come from ``native.just``;
* explicit routes come from ``runtime_bin.rs``;
* fallback routes come from binary targets in ``Cargo.toml``;
* legacy provenance comes from the other ``*.just`` files; and
* risk/priority are derived from a small, reviewable classification table.

Run without arguments to validate the checked-in ledger.  Run with
``--generate`` when the recipe manifest, dispatcher, or Cargo targets change.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "build_files/just/kyth/native.just"
RUNTIME = ROOT / "src/kyth-shared-rs/src/runtime_bin.rs"
CARGO = ROOT / "src/kyth-shared-rs/Cargo.toml"
TUNABLE_REGISTRY = ROOT / "src/kyth-shared-rs/src/system/tunable_registry.rs"
OUTPUT = ROOT / "build_files/config/runtime-recipe-migration-inventory.json"

SCHEMA_VERSION = 1
# Recipe declarations may use ``*args`` or named/default parameters.  Exclude
# ``set shell :=`` and other assignments, which also begin with an identifier
# and contain a colon but are not recipes.
RECIPE_RE = re.compile(
    r"^(?P<name>[A-Za-z_][A-Za-z0-9_-]*)(?:\s+[^:\n]*)*:\s*(?![=])"
)
ROUTE_NAME_RE = re.compile(r'"([A-Za-z_][A-Za-z0-9_-]*)"')
CARGO_BINARY_RE = re.compile(r'^\s*name\s*=\s*"(kyth-[^"]+)"\s*$', re.MULTILINE)
TUNABLE_NAME_RE = re.compile(r'\("([a-z0-9][a-z0-9-]*)"')

# These sets are deliberately small and conservative.  They classify the
# verification depth needed for a future Rust implementation; they do not
# claim that a route is complete merely because it is present.
READ_ONLY_NAMES = {
    "ai-dev-status",
    "dualboot-status",
    "gaming-stack-status",
    "hardware-inventory",
    "kerver",
    "device-info",
    "smoke-check",
    "post-update-check",
    "resume-check",
    "snappy-bench",
    "perf-gate",
    "telemetry-opt",
    "controller-check",
    "gaming-stack-status",
    "windows-verify",
}

DESTRUCTIVE_NAMES = {
    "reclaim-windows",
    "fix-dualboot-clock",
    "remove-waydroid",
    "ai-dev-remove",
    "hardware-policy-apply",
    "rebase",
    "switch-channel",
    "switch-channel-impl",
    "switch-kernel",
    "retry-quarantined-update",
}

PRIVILEGED_MARKERS = (
    "firmware",
    "printer",
    "dualboot",
    "reclaim-windows",
    "hardware-policy",
    "driver",
    "displaylink",
    "asus-tools",
    "setup-vr",
    "setup-waydroid",
    "remove-waydroid",
    "setup-boot-windows-steam",
    "switch-kernel",
    "switch-channel",
    "rebase",
)

# Optional vendor release assets are outside the supported image contract.
# Keep their native dispatcher names for deterministic refusal, but record the
# recipes as explicitly retired rather than as incomplete Rust implementations.
RETIRED_RECIPE_NAMES = frozenset(
    {"install-lsfg-vk", "deploy-opticscaler", "install-umu"}
)


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def parse_recipes() -> list[dict[str, Any]]:
    """Return recipe declarations in manifest order with line numbers."""

    lines = _read(MANIFEST).splitlines()
    recipes: list[dict[str, Any]] = []
    for line_number, line in enumerate(lines, start=1):
        match = RECIPE_RE.match(line)
        if not match:
            continue

        command = ""
        for body_line in lines[line_number:]:
            stripped = body_line.strip()
            if not stripped or stripped.startswith("["):
                continue
            command = stripped
            break
        recipes.append(
            {
                "name": match.group("name"),
                "manifest_line": line_number,
                "manifest_command": command,
            }
        )
    return recipes


def explicit_dispatch_names(recipe_names: set[str]) -> set[str]:
    """Find recipe names handled by the Rust ``recipe`` dispatcher.

    The search is limited to the dispatcher function.  It intentionally
    intersects with the manifest names so helper operations and unrelated
    string literals cannot become recipe owners accidentally.
    """

    text = _read(RUNTIME)
    start = text.index("fn recipe(")
    end = text.index("fn delegate(", start)
    return set(ROUTE_NAME_RE.findall(text[start:end])) & recipe_names


def native_binary_names() -> set[str]:
    """Return packaged Rust binary names declared in the shared Cargo file."""

    return set(CARGO_BINARY_RE.findall(_read(CARGO)))


def native_tunable_names(recipe_names: set[str]) -> set[str]:
    """Return recipe names installed as symlinks to ``kyth-tunable-rs``.

    ``01-tunable-dispatcher.sh`` creates ``/usr/bin/kyth-<name>`` symlinks
    from the Rust registry.  These are real native fallback owners even
    though they are not individual Cargo targets, so the registry is part of
    the source-derived route map.
    """

    return set(TUNABLE_NAME_RE.findall(_read(TUNABLE_REGISTRY))) & recipe_names


def legacy_provenance() -> dict[str, list[dict[str, Any]]]:
    """Index recipe declarations in the pre-native ``*.just`` files."""

    result: dict[str, list[dict[str, Any]]] = {}
    for path in sorted((ROOT / "build_files/just/kyth").rglob("*.just")):
        if path == MANIFEST:
            continue
        relative = path.relative_to(ROOT).as_posix()
        for line_number, line in enumerate(_read(path).splitlines(), start=1):
            match = RECIPE_RE.match(line)
            if not match:
                continue
            result.setdefault(match.group("name"), []).append(
                {"path": relative, "line": line_number}
            )
    return result


def route_for(
    name: str,
    explicit: set[str],
    binaries: set[str],
    tunables: set[str],
) -> tuple[str, str | None, str | None]:
    """Return route kind, Rust owner, and target for one recipe."""

    if name in RETIRED_RECIPE_NAMES:
        return "explicit-retirement", "native::kyth-runtime", "kyth-runtime::retired"

    if name in explicit:
        return "explicit-dispatch", "native::kyth-runtime", "kyth-runtime::recipe"

    if name in tunables:
        return "native-fallback", "native::kyth-tunable-rs", f"kyth-tunable-rs::{name}"

    fallback = f"kyth-{name}"
    if fallback in binaries:
        return "native-fallback", f"native::{fallback}", fallback

    return "missing-owner", None, None


def classify_risk(name: str) -> tuple[str, str]:
    """Return a conservative risk tier and the rule that selected it."""

    if name in DESTRUCTIVE_NAMES or name.startswith("retry-quarantined"):
        return "destructive", "explicit destructive/update action"
    if name in READ_ONLY_NAMES or name.endswith("-status") or name.endswith("-inventory"):
        return "read-only", "status, inventory, or diagnostic action"
    if any(marker in name for marker in PRIVILEGED_MARKERS):
        return "privileged-writer", "system, hardware, boot, or driver mutation"
    return "user-session-writer", "user-session, package, or application mutation"


def current_route(command: str, name: str) -> str:
    """Normalize the command shown in ``native.just`` for the ledger."""

    command = command.replace("{{ args }}", "<args>")
    command = re.sub(r"\s+", " ", command).strip()
    if command:
        return command
    return f"/usr/bin/kyth-runtime recipe {name}"


def route_display(command: str, name: str) -> str:
    """Return a concise semantic route while preserving source command separately."""

    if "kyth-runtime recipe" in command:
        return f"kyth-runtime recipe {name}"
    return f"kyth-runtime {name}"


def generate() -> dict[str, Any]:
    recipes = parse_recipes()
    names = {recipe["name"] for recipe in recipes}
    explicit = explicit_dispatch_names(names)
    binaries = native_binary_names()
    tunables = native_tunable_names(names)
    provenance = legacy_provenance()

    entries: list[dict[str, Any]] = []
    for recipe in recipes:
        name = recipe["name"]
        route_kind, owner, target = route_for(name, explicit, binaries, tunables)
        risk_tier, risk_basis = classify_risk(name)
        legacy_sources = provenance.get(name, [])
        covered = route_kind != "missing-owner"
        retired = route_kind == "explicit-retirement"
        entries.append(
            {
                "name": name,
                "manifest": "build_files/just/kyth/native.just",
                "manifest_line": recipe["manifest_line"],
                "manifest_command": current_route(recipe["manifest_command"], name),
                "legacy_sources": legacy_sources,
                "source_provenance": "legacy-recipe" if legacy_sources else "native-manifest-only",
                "current_route": route_display(recipe["manifest_command"], name),
                "route_kind": route_kind,
                "rust_owner": owner,
                "rust_target": target,
                "assessment": "retired" if retired else "covered" if covered else "open",
                "status": "retired" if retired else "routed" if covered else "needs-rust-owner",
                "risk_tier": risk_tier,
                "risk_basis": risk_basis,
                "migration_priority": 3 if covered else 1,
                "parity_tests": (
                    ["tests/test_runtime_recipe_parity.py"]
                    if risk_tier in {"destructive", "privileged-writer"}
                    else []
                ),
                "retirement": (
                "recipe explicitly retired: optional vendor asset workflow is not part of the supported image contract"
                if retired
                else "retain recipe until the Rust owner has parity coverage and image validation"
                if covered
                    else "retain recipe until a Rust owner is assigned or removal is explicitly reviewed"
                ),
            }
        )

    missing = [entry for entry in entries if entry["route_kind"] == "missing-owner"]
    return {
        "schema_version": SCHEMA_VERSION,
        "generated_from": [
            "build_files/just/kyth/native.just",
            "src/kyth-shared-rs/src/runtime_bin.rs",
            "src/kyth-shared-rs/Cargo.toml",
            "src/kyth-shared-rs/src/system/tunable_registry.rs",
            "build_files/just/kyth/**/*.just",
        ],
        "purpose": "Per-recipe Rust migration ledger for the supported native ujust manifest.",
        "recipe_count": len(entries),
        "summary": {
            "routed": len(entries) - len(missing),
        "explicit_dispatch": sum(entry["route_kind"] == "explicit-dispatch" for entry in entries),
        "explicit_retirement": sum(entry["route_kind"] == "explicit-retirement" for entry in entries),
            "native_fallback": sum(entry["route_kind"] == "native-fallback" for entry in entries),
            "missing_owner": len(missing),
            "open_assessments": len(missing),
        },
        "entries": entries,
    }


def validate(document: dict[str, Any], expected: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if document != expected:
        errors.append("checked-in ledger differs from source-derived output; run with --generate")
    if document.get("schema_version") != SCHEMA_VERSION:
        errors.append(f"schema_version must be {SCHEMA_VERSION}")
    entries = document.get("entries")
    if not isinstance(entries, list):
        return errors + ["entries must be a list"]
    names = [entry.get("name") for entry in entries]
    if len(names) != len(set(names)):
        errors.append("recipe names must be unique")
    if document.get("recipe_count") != len(entries):
        errors.append("recipe_count does not match entries")
    for entry in entries:
        for field in ("name", "manifest", "manifest_line", "route_kind", "status", "risk_tier"):
            if field not in entry:
                errors.append(f"{entry.get('name', '<unknown>')}: missing {field}")
        if entry.get("route_kind") == "missing-owner" and entry.get("rust_owner") is not None:
            errors.append(f"{entry['name']}: missing-owner entry cannot have rust_owner")
        if entry.get("route_kind") != "missing-owner" and not entry.get("rust_owner"):
            errors.append(f"{entry['name']}: routed entry must have rust_owner")
    return errors


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--generate", action="store_true", help="write the source-derived ledger")
    args = parser.parse_args(argv)

    expected = generate()
    if args.generate:
        OUTPUT.write_text(json.dumps(expected, indent=2) + "\n", encoding="utf-8")
        print(f"generated {OUTPUT.relative_to(ROOT)} ({expected['recipe_count']} recipes)")
        return 0

    if not OUTPUT.is_file():
        print(f"missing {OUTPUT.relative_to(ROOT)}; run with --generate", file=sys.stderr)
        return 1
    document = json.loads(_read(OUTPUT))
    errors = validate(document, expected)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(
        "valid: "
        f"{document['recipe_count']} recipes, "
        f"{document['summary']['routed']} routed, "
        f"{document['summary']['missing_owner']} missing Rust owners"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
