"""Tests for kyth_shared.perf_gate and the check-perf-gate.py CI wiring.

perf_gate.check_perf_gate() had zero coverage before this — every real
caller passed current_ms=None, which trivially passes without comparing
anything, so the actual comparison branch never ran in practice either.
"""
from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "build_files" / "kyth_shared"))

from kyth_shared.perf_gate import (  # noqa: E402
    check_perf_gate,
    load_perf_gate,
    save_perf_gate,
)

SPEC = importlib.util.spec_from_file_location(
    "check_perf_gate_script", ROOT / "build_files/scripts/check-perf-gate.py"
)
assert SPEC and SPEC.loader
check_perf_gate_script = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = check_perf_gate_script
SPEC.loader.exec_module(check_perf_gate_script)


class PerfGateCoreTests(unittest.TestCase):
    def test_missing_config_defaults_to_enabled_5_percent(self):
        with tempfile.TemporaryDirectory() as tmp:
            cfg = load_perf_gate(Path(tmp) / "does-not-exist.toml")
        self.assertEqual(cfg, {"threshold": 10, "enabled": True})

    def test_save_then_load_round_trips(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "perf-gate.toml"
            save_perf_gate({"threshold": 10, "enabled": False}, path)
            self.assertEqual(load_perf_gate(path), {"threshold": 10, "enabled": False})

    def test_threshold_is_clamped_to_1_20(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "perf-gate.toml"
            save_perf_gate({"threshold": 999, "enabled": True}, path)
            self.assertEqual(load_perf_gate(path)["threshold"], 20)
            save_perf_gate({"threshold": -5, "enabled": True}, path)
            self.assertEqual(load_perf_gate(path)["threshold"], 1)

    def test_no_ledger_history_passes_without_comparing(self):
        with tempfile.TemporaryDirectory() as tmp:
            ledger = Path(tmp) / "perf-ledger.jsonl"  # never created
            result = check_perf_gate(current_ms=100.0, ledger=ledger)
        self.assertTrue(result["pass"])
        self.assertIsNone(result["last"])

    def test_current_ms_none_passes_without_comparing_even_with_history(self):
        with tempfile.TemporaryDirectory() as tmp:
            ledger = Path(tmp) / "perf-ledger.jsonl"
            ledger.write_text('{"p95": 10.0}\n', encoding="utf-8")
            result = check_perf_gate(current_ms=None, ledger=ledger)
        self.assertTrue(result["pass"])
        self.assertNotIn("delta", result)

    def test_regression_beyond_threshold_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            ledger = Path(tmp) / "perf-ledger.jsonl"
            ledger.write_text('{"p95": 100.0}\n', encoding="utf-8")
            result = check_perf_gate(current_ms=115.0, ledger=ledger)  # +15% > 10% threshold
        self.assertFalse(result["pass"])
        self.assertEqual(result["delta"], 15.0)
        self.assertEqual(result["threshold"], 10)

    def test_improvement_and_small_drift_pass(self):
        with tempfile.TemporaryDirectory() as tmp:
            ledger = Path(tmp) / "perf-ledger.jsonl"
            ledger.write_text('{"p95": 100.0}\n', encoding="utf-8")
            self.assertTrue(check_perf_gate(current_ms=90.0, ledger=ledger)["pass"])
            self.assertTrue(check_perf_gate(current_ms=103.0, ledger=ledger)["pass"])

    def test_reads_the_most_recent_of_the_last_ten_entries(self):
        with tempfile.TemporaryDirectory() as tmp:
            ledger = Path(tmp) / "perf-ledger.jsonl"
            ledger.write_text(
                "\n".join(f'{{"p95": {v}.0}}' for v in range(1, 13)) + "\n",
                encoding="utf-8",
            )
            result = check_perf_gate(current_ms=12.0, ledger=ledger)
        self.assertEqual(result["last"], 12.0)  # the most recent line, not the first

    def test_disabled_gate_always_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            cfg_path = Path(tmp) / "perf-gate.toml"
            save_perf_gate({"threshold": 5, "enabled": False}, cfg_path)
            ledger = Path(tmp) / "perf-ledger.jsonl"
            ledger.write_text('{"p95": 1.0}\n', encoding="utf-8")
            result = check_perf_gate(current_ms=1000.0, ledger=ledger, path=cfg_path)
        self.assertEqual(result, {"enabled": False, "pass": True})

    def test_malformed_ledger_lines_are_skipped_not_fatal(self):
        with tempfile.TemporaryDirectory() as tmp:
            ledger = Path(tmp) / "perf-ledger.jsonl"
            ledger.write_text("not json\n{\"p95\": 50.0}\n{broken\n", encoding="utf-8")
            result = check_perf_gate(current_ms=51.0, ledger=ledger)
        self.assertEqual(result["last"], 50.0)


class CheckPerfGateScriptTests(unittest.TestCase):
    """The compatibility script delegates measurement and persistence to Rust."""

    def test_wrapper_delegates_to_native_binary(self):
        completed = mock.Mock(returncode=0)
        with mock.patch.object(check_perf_gate_script, "_command", return_value=["kyth-perf-gate-rs"]), \
             mock.patch.object(check_perf_gate_script.subprocess, "run", return_value=completed) as run:
            exit_code = check_perf_gate_script.main()
        self.assertEqual(exit_code, 0)
        self.assertEqual(run.call_args.args[0][0], "kyth-perf-gate-rs")
        self.assertEqual(run.call_args.args[0][1], "measure")


if __name__ == "__main__":
    unittest.main()
