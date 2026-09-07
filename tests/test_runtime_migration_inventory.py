from __future__ import annotations

import ast
import importlib.util
import json
import subprocess
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CHECKER = ROOT / "build_files/scripts/check-runtime-migration-inventory.py"
INVENTORY = ROOT / "build_files/config/runtime-migration-inventory.json"
REPORT = ROOT / "build_files/config/runtime-migration-report.json"


def load_checker():
    spec = importlib.util.spec_from_file_location("runtime_inventory_checker", CHECKER)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def load_inventory():
    return json.loads(INVENTORY.read_text(encoding="utf-8"))


def tunable_aliases() -> list[str]:
    """Alias keys of _BUILTIN_TUNABLES, parsed statically (no import)."""
    tree = ast.parse((ROOT / "src/kyth_shared/kyth_shared/tunable.py").read_text(encoding="utf-8"))
    for node in ast.walk(tree):
        if isinstance(node, ast.AnnAssign) and getattr(node.target, "id", "") == "_BUILTIN_TUNABLES":
            return [k.value for k in node.value.keys if isinstance(k, ast.Constant)]
    raise AssertionError("_BUILTIN_TUNABLES not found")


class InventoryTest(unittest.TestCase):
    def test_checked_in_inventory_is_source_complete(self):
        checker = load_checker()
        document = load_inventory()
        expected = {item["path"] for item in checker.discover()}
        self.assertFalse(checker.validate(document, expected_paths=expected))
        self.assertGreaterEqual(
            {item["surface"] for item in document["entries"]},
            {"launcher", "systemd-unit", "python-runtime", "installer-runtime", "ujust-recipe", "rust-crate"},
        )

    def test_checker_cli_passes(self):
        result = subprocess.run(
            [sys.executable, str(CHECKER)],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("valid", result.stdout)

    def test_installer_source_is_retired_fixture(self):
        # Post-cutover contract: the Python installer backend is source-only
        # parity material, not an installed runtime authority.
        entries = load_inventory()["entries"]
        installer = next(item for item in entries if item["path"] == "src/kyth-installer/kyth_installer/server.py")
        self.assertEqual(installer["runtime_authority"], "source-only")
        self.assertEqual(installer["runtime_scope"], "test-fixture")
        self.assertFalse(installer["runtime_active"])
        welcome = next(item for item in entries if item["path"] == "src/kyth-welcome/kyth_welcome/core_base.py")
        self.assertEqual(welcome["runtime_authority"], "source-only")
        self.assertEqual(welcome["runtime_scope"], "test-fixture")
        self.assertFalse(welcome["runtime_active"])

    def test_active_runtime_report_is_current(self):
        checker = load_checker()
        document = load_inventory()
        report = json.loads(REPORT.read_text(encoding="utf-8"))
        self.assertEqual(report, checker.report(document))
        # Installer cutover complete: no priority-0 work remains open and no
        # installer authority is active.
        self.assertEqual(report["summary"]["p0_open_entries"], 0)
        self.assertEqual(report["p0_open"], [])
        self.assertFalse(
            [item for item in report["active_python"] if item["runtime_authority"] == "python-installer"]
        )

    def test_frontend_and_python_boundaries_are_clean(self):
        checker = load_checker()
        document = load_inventory()
        self.assertEqual(checker.boundary_errors(document), [])

    def test_inventory_preserves_actual_unit_execstart(self):
        entries = load_inventory()["entries"]
        unit = next(item for item in entries if item["path"] == "build_files/kyth-browser-wallet-defaults.service")
        self.assertEqual(unit["exec_start"], ["/usr/bin/kyth-vscode-wallet"])

    def test_inventory_distinguishes_native_install_from_retained_tunable_fixture(self):
        entries = load_inventory()["entries"]
        tunable = next(item for item in entries if item["path"] == "build_files/kyth-swappiness")
        self.assertEqual(tunable["current_implementation"], "alias")
        self.assertEqual(tunable["installed_implementation"], "rust")
        self.assertEqual(tunable["status"], "done-native")
        self.assertEqual(tunable["owner"], "native::kyth-tunable-rs")

    def test_all_tunable_aliases_are_native_dispatcher_entries(self):
        entries = load_inventory()["entries"]
        tunables = [
            item for item in entries
            if item["path"].startswith("build_files/kyth-")
            and item.get("resolved_target") == "build_files/kyth-tunable"
        ]
        self.assertEqual(len(tunables), 94)
        self.assertEqual({item["status"] for item in tunables}, {"done-native"})
        self.assertEqual({item["installed_implementation"] for item in tunables}, {"rust"})
        self.assertLessEqual(
            {item["owner"] for item in tunables},
            {"native::kyth-tunable", "native::kyth-tunable-rs"},
        )
        self.assertIn("native::kyth-tunable-rs", {item["owner"] for item in tunables})

    def test_uninstalled_legacy_hub_privilege_fixture_is_not_an_active_authority(self):
        entries = load_inventory()["entries"]
        privileged = next(item for item in entries if item["path"] == "src/kyth-welcome/kyth_welcome/services/privileged.py")
        self.assertEqual(privileged["status"], "explicitly-not-ported")
        self.assertEqual(privileged["installed_implementation"], "not-installed")
        self.assertTrue(privileged["owner"].startswith("fixture::"))

    def test_superseded_tunable_modules_are_inactive_fixtures(self):
        checker = load_checker()
        entries = load_inventory()["entries"]
        by_name = {item["name"]: item for item in entries if item["surface"] == "python-runtime"}
        self.assertEqual(len(checker.SUPERSEDED_TUNABLE_MODULES), 92)
        for name in sorted(checker.SUPERSEDED_TUNABLE_MODULES):
            item = by_name[name]
            self.assertEqual(item["runtime_authority"], "python-shared-package", name)
            self.assertFalse(item["runtime_active"], name)
            self.assertEqual(item["migration_priority"], 3, name)
            self.assertEqual(item.get("superseded_by"), checker.TUNABLE_SUPERSEDED_BY, name)

    def test_reachable_modules_are_never_superseded(self):
        checker = load_checker()
        entries = load_inventory()["entries"]
        by_name = {item["name"]: item for item in entries if item["surface"] == "python-runtime"}
        # sched_arbiter is imported by build_files/kyth-game-launch; perf_gate
        # is used by build_files/scripts/check-perf-gate.py; the shell-harness
        # channel (e.g. qualification.py via vm-acceptance.sh) stays live.
        for name in ("sched_arbiter", "perf_gate", "qualification", "memory_tune", "sysctl_compose"):
            item = by_name[name]
            self.assertTrue(item["runtime_active"], name)
            self.assertNotIn("superseded_by", item, name)
        self.assertFalse(set(checker.SUPERSEDED_TUNABLE_MODULES) & set(checker.SHELL_HARNESS_MODULES))

    def test_data_or_config_is_terminal_not_queued(self):
        entries = load_inventory()["entries"]
        data = [item for item in entries if item["runtime_authority"] == "data-or-config"]
        self.assertEqual(len(data), 8)
        for item in data:
            self.assertEqual(item["status"], "not-applicable", item["path"])
            self.assertFalse(item["runtime_active"], item["path"])

    def test_tunable_registry_covers_all_python_aliases(self):
        rust = (ROOT / "src/kyth-shared-rs/src/system/tunable_registry.rs").read_text(encoding="utf-8")
        aliases = tunable_aliases()
        self.assertEqual(len(aliases), 94)
        missing = [name for name in aliases if name not in rust]
        self.assertEqual(missing, [])

    def test_user_polish_python_sources_are_retired_fixtures(self):
        entries = load_inventory()["entries"]
        for name in ("user_polish", "user_polish_flatpak"):
            item = next(item for item in entries if item["name"] == name and item["surface"] == "python-runtime")
            self.assertFalse(item["runtime_active"])
            self.assertEqual(item["superseded_by"], "native::kyth-user-polish")


if __name__ == "__main__":
    unittest.main()
