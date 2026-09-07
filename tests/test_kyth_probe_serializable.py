"""Every disk-backed probe section must survive the JSON round trip.

kyth-probe.service died on every run with
``TypeError: Object of type HardwareView is not JSON serializable`` because the
hardware-view collector returned a live dataclass into a section that gets
written to the shared cache file. The disk cache was therefore never populated,
silently defeating the cross-process probe caching the Hub's cold-start budget
depends on.
"""
from __future__ import annotations

import json
import pathlib
import sys
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "build_files" / "kyth_shared"))

from kyth_shared.system import probe  # noqa: E402


class SectionSerializabilityTests(unittest.TestCase):
    def test_every_collector_section_is_declared(self):
        """A section nobody declares a TTL for silently skips the disk cache."""
        declared = set(probe.DISK_TTL)
        emitted = {
            section
            for collector in probe.default_collectors()
            for section in collector.keys
        }
        # display-detect is intentionally memory-only; everything else declared.
        self.assertEqual(set(), emitted - declared - {"display-detect"})

    def test_typed_memo_keys_are_not_disk_backed(self):
        """Typed dataclass keys cannot round-trip through the JSON cache file."""
        for typed, projection in (
            ("hardware-view", "hardware-summary"),
            ("network-identity", "network-summary"),
        ):
            with self.subTest(key=typed):
                self.assertNotIn(typed, probe.DISK_BACKED_KEYS)
                self.assertIn(projection, probe.DISK_BACKED_KEYS)

    def test_hardware_collector_emits_json_safe_values(self):
        from kyth_shared.hardware_policy import Evaluation
        from kyth_shared.system.hardware_view import HardwareView

        # evaluate_system() stores profiles as dicts, not objects with .id.
        evaluation = mock.Mock(spec=Evaluation)
        evaluation.capabilities = ["gpu.amd", "gpu.hybrid"]
        evaluation.profiles = [{"id": "amd-desktop"}]
        view = HardwareView(
            evaluation=evaluation, applied={}, has_nvidia=False, is_hybrid=True
        )

        with mock.patch(
            "kyth_shared.system.hardware_view.get_hardware_view", return_value=view
        ):
            section = probe._collect_hardware_view()

        # The exact failure mode: this raised TypeError before the fix.
        json.dumps(section)
        self.assertEqual(False, section["hardware-summary"]["has_nvidia"])
        self.assertEqual(True, section["hardware-summary"]["is_hybrid"])
        self.assertEqual(["amd-desktop"], section["hardware-summary"]["profiles"])

    def test_hardware_collector_failure_is_json_safe_too(self):
        with mock.patch(
            "kyth_shared.system.hardware_view.get_hardware_view",
            side_effect=RuntimeError("no lspci"),
        ):
            section = probe._collect_hardware_view()

        json.dumps(section)
        self.assertIsNone(section["hardware-summary"])

    def test_display_collector_reads_profile_dicts(self):
        from kyth_shared.hardware_policy import Evaluation

        evaluation = mock.Mock(spec=Evaluation)
        evaluation.capabilities = ["gpu.amd"]
        evaluation.profiles = [{"id": "amd-desktop"}]

        with mock.patch(
            "kyth_shared.system.hardware_native.status",
            return_value={"evaluation": {"capabilities": evaluation.capabilities, "profiles": evaluation.profiles}},
        ):
            section = probe._collect_display()

        json.dumps(section)
        self.assertEqual(["amd-desktop"], section["display-detect"]["profiles"])

    def test_full_snapshot_write_round_trips(self):
        """End-to-end: what collect_snapshot produces must be writable."""
        for collector in probe.default_collectors():
            with self.subTest(collector=collector.name):
                try:
                    values = collector.collect()
                except Exception:
                    continue  # collection failure is a separate concern
                for section, value in values.items():
                    if section not in probe.DISK_BACKED_KEYS:
                        continue
                    json.dumps({section: value})


if __name__ == "__main__":
    unittest.main()
