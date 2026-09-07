#!/usr/bin/env python3
"""Measure maintainability budgets and optional built-image/runtime metrics."""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BUDGETS_PATH = ROOT / "build_files/config/optimization-budgets.json"
HUB_ROOT = ROOT / "src/kyth-hub-web/src"
INSTALLER_WEBUI = ROOT / "build_files/kyth-installer/kyth_installer/webui"


def _static_metrics() -> dict[str, int]:
    js_sizes = {path.name: path.stat().st_size for path in INSTALLER_WEBUI.glob("*.js")}
    hub_sources = [
        path for path in HUB_ROOT.rglob("*")
        if path.is_file() and path.suffix in {".ts", ".tsx"}
    ]
    return {
        "installer_js_max_file_bytes": max(js_sizes.values(), default=0),
        "probe_collector_count": _probe_collector_count(),
        "system_hub_inline_styles": sum(
            path.read_text(encoding="utf-8").count("style={{")
            for path in hub_sources
        ),
        "system_hub_source_files": len(hub_sources),
    }


def _probe_collector_count() -> int:
    source = (ROOT / "build_files/kyth_shared/kyth_shared/system/probe.py").read_text(
        encoding="utf-8",
    )
    body = source.split("def default_collectors()", 1)[1].split(
        "def _run_collector", 1,
    )[0]
    return body.count("ProbeCollector(")


def _runtime_metrics() -> dict[str, object]:
    metrics: dict[str, object] = {}
    started = time.perf_counter()
    probe_cmd: list[str] | None = None
    for candidate in (
        Path("/usr/bin/kyth-probe"),
        ROOT / "src/kyth-shared-rs/target/release/kyth-probe",
        ROOT / "src/kyth-shared-rs/target/debug/kyth-probe",
    ):
        if candidate.is_file() and os.access(candidate, os.X_OK):
            probe_cmd = [str(candidate), "--print-only"]
            break
    if probe_cmd is None:
        probe_cmd = [
            "cargo", "run", "--quiet", "--locked",
            "--manifest-path", "src/kyth-shared-rs/Cargo.toml",
            "--bin", "kyth-probe", "--", "--print-only",
        ]
    probed = subprocess.run(probe_cmd, cwd=ROOT, capture_output=True, text=True, check=False)
    metrics["probe_duration_ms"] = round((time.perf_counter() - started) * 1000, 2)
    if probed.returncode:
        metrics["probe_error"] = probed.stderr.strip().splitlines()[-1]
    else:
        document = json.loads(probed.stdout)
        sections = document if isinstance(document, dict) else {}
        metrics.update({
            "probe_result_count": len(sections),
            "probe_status_counts": {
                "available": sum(value is not None for value in sections.values()),
                "unavailable": sum(value is None for value in sections.values()),
                "failed": 0,
            },
        })
    return metrics


def _artifact_metrics(oci_manifest: Path | None, rpm_manifest: Path | None) -> dict[str, int]:
    metrics: dict[str, int] = {}
    if oci_manifest:
        document = json.loads(oci_manifest.read_text(encoding="utf-8"))
        manifests = document.get("manifests")
        if manifests:
            metrics["image_index_bytes"] = sum(int(item.get("size", 0)) for item in manifests)
        else:
            layers = document.get("layers", [])
            metrics["image_compressed_bytes"] = sum(int(item.get("size", 0)) for item in layers)
            metrics["image_layer_count"] = len(layers)
    if rpm_manifest:
        packages = {
            line.strip() for line in rpm_manifest.read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.startswith("#")
        }
        metrics["rpm_package_count"] = len(packages)
    return metrics


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--runtime", action="store_true")
    parser.add_argument("--oci-manifest", type=Path)
    parser.add_argument("--rpm-manifest", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    budgets = json.loads(BUDGETS_PATH.read_text(encoding="utf-8"))
    static = _static_metrics()
    report: dict[str, object] = {
        "schema_version": 1,
        "source_revision": os.environ.get("GITHUB_SHA", "local"),
        "static": static,
        "budgets": budgets,
        "artifacts": _artifact_metrics(args.oci_manifest, args.rpm_manifest),
    }
    if args.runtime:
        report["runtime"] = _runtime_metrics()

    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered, encoding="utf-8")
    else:
        print(rendered, end="")

    failures = [
        f"{name}: {static[name]} exceeds budget {limit}"
        for name, limit in budgets.items()
        if static[name] > limit
    ]
    if args.check and failures:
        for failure in failures:
            print(failure, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
