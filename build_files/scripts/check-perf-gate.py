#!/usr/bin/env python3
"""Compatibility wrapper for the native Rust performance gate."""

from __future__ import annotations

import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def _command() -> list[str]:
    for candidate in (
        ROOT / "src/kyth-shared-rs/target/debug/kyth-perf-gate-rs",
        ROOT / "src/kyth-shared-rs/target/release/kyth-perf-gate-rs",
    ):
        if candidate.is_file() and candidate.stat().st_mode & 0o111:
            return [str(candidate)]
    installed = Path("/usr/bin/kyth-perf-gate-rs")
    if installed.is_file() and installed.stat().st_mode & 0o111:
        return [str(installed)]
    return [
        "cargo", "run", "--quiet", "--manifest-path",
        str(ROOT / "src/kyth-shared-rs/Cargo.toml"),
        "--bin", "kyth-perf-gate-rs", "--",
    ]


def main() -> int:
    args = ["measure", "--ledger", str(ROOT / "build_files/config/perf-ledger.jsonl")]
    if "--record" in os.sys.argv[1:]:
        args.append("--record")
    return subprocess.run([*_command(), *args], cwd=ROOT, check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main())
