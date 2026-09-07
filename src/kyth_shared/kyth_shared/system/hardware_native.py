"""Native hardware-policy status boundary.

The policy engine and its persisted state belong to ``kyth-hardware-policy``.
Python callers that still provide compatibility probes may consume its JSON
status, but must not import or reimplement the policy engine.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from kyth_shared.system.process import run_command

_ROOT = Path(__file__).resolve().parents[4]
_POLICY = _ROOT / "build_files/config/hardware-profiles.toml"


def _native_command() -> list[str]:
    installed = Path("/usr/bin/kyth-hardware-policy")
    if installed.is_file() and installed.stat().st_mode & 0o111:
        return [str(installed)]
    for candidate in (
        _ROOT / "src/kyth-shared-rs/target/release/kyth-hardware-policy",
        _ROOT / "src/kyth-shared-rs/target/debug/kyth-hardware-policy",
    ):
        if candidate.is_file() and candidate.stat().st_mode & 0o111:
            return [str(candidate), "--policy", str(_POLICY)]
    return [
        "cargo",
        "run",
        "--quiet",
        "--manifest-path",
        str(_ROOT / "src/kyth-shared-rs/Cargo.toml"),
        "--bin",
        "kyth-hardware-policy",
        "--",
        "--policy",
        str(_POLICY),
    ]


def status() -> dict[str, Any]:
    """Return the Rust-owned evaluation and applied state."""

    result = run_command([*_native_command(), "status"], timeout=30)
    if result is None or result.returncode != 0:
        detail = result.stderr.strip() if result is not None else "native command unavailable"
        raise RuntimeError(detail or "kyth-hardware-policy status failed")
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError("kyth-hardware-policy returned invalid JSON") from exc
    if not isinstance(payload, dict) or not isinstance(payload.get("evaluation"), dict):
        raise RuntimeError("kyth-hardware-policy returned an invalid status payload")
    return payload


__all__ = ["status"]
