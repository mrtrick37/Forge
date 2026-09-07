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

    def test_python_package_queue_uses_reachability_not_path_prefix(self):
        checker = load_checker()
        reachable = checker.python_reachable_modules()
        entries = load_inventory()["entries"]
        by_path = {item["path"]: item for item in entries}

        # The surviving Python console scripts are roots; their transitive
        # imports remain active until a native entry point replaces them.
        for module in ("ai_dev", "boot_health", "hardware_policy", "qualification"):
            path = f"src/kyth_shared/kyth_shared/{module}.py"
            self.assertIn(f"kyth_shared.{module}", reachable)
            self.assertTrue(by_path[path]["runtime_active"], path)
            self.assertEqual(by_path[path]["status"], "queued")

        # This source file is present in the installed compatibility package,
        # but no supported launcher or harness reaches it after the native
        # cutovers. It must not inflate the migration queue.
        accounts = by_path["src/kyth_shared/kyth_shared/accounts.py"]
        self.assertNotIn("kyth_shared.accounts", reachable)
        self.assertFalse(accounts["runtime_active"])
        self.assertEqual(accounts["runtime_authority"], "source-only")
        self.assertEqual(accounts["status"], "explicitly-not-ported")
        self.assertEqual(accounts["installed_implementation"], "python-fixture")

    def test_direct_harness_and_dynamic_catalog_edges_are_preserved(self):
        checker = load_checker()
        reachable = checker.python_reachable_modules()
        entries = load_inventory()["entries"]
        by_name = {item["name"]: item for item in entries if item["surface"] == "python-runtime"}

        # These modules are invoked from build/acceptance harnesses rather than
        # from an installed console script.
        for name in ("memory_tune", "sysctl_compose", "perf_gate", "qualification"):
            self.assertIn(f"kyth_shared.{name}", reachable, name)
            self.assertTrue(by_name[name]["runtime_active"], name)

        # hardware_quirks.catalog imports the managed modules through its
        # explicit importlib table; the reachability graph must retain them.
        for name in (
            "amdgpu_gaming_memory", "amdgpu_psr_disable", "asus_tuf_amd_cachy_stability",
            "bluetooth_usb_autosuspend", "intel_i915_media", "intel_wifi_association_power",
            "mediatek_pcie_wifi_aspm", "nvidia_wayland_suspend",
        ):
            module = f"kyth_shared.hardware_quirks.{name}"
            self.assertIn(module, reachable, module)
            self.assertTrue(by_name[name]["runtime_active"], name)

    def test_surviving_python_console_entry_point_is_not_silently_retired(self):
        checker = load_checker()
        roots = checker._python_console_roots()
        self.assertIn("kyth_shared.ai_dev", roots)
        self.assertNotIn("kyth_shared.guardian", roots)
        item = next(
            item for item in load_inventory()["entries"]
            if item["path"] == "src/kyth_shared/kyth_shared/ai_dev.py"
        )
        self.assertTrue(item["runtime_active"])
        self.assertEqual(item["status"], "queued")

    def test_data_or_config_is_terminal_not_queued(self):
        entries = load_inventory()["entries"]
        data = [item for item in entries if item["runtime_authority"] == "data-or-config"]
        self.assertEqual(len(data), 8)
        for item in data:
            self.assertEqual(item["status"], "not-applicable", item["path"])
            self.assertFalse(item["runtime_active"], item["path"])

    def test_shell_runtime_entries_have_function_level_ownership(self):
        entries = load_inventory()["entries"]
        shell = [
            item for item in entries
            if item["runtime_authority"] == "shell-orchestration"
            and item["runtime_active"]
        ]
        self.assertGreater(len(shell), 0)
        for item in shell:
            self.assertTrue(item["function_inventory"], item["path"])
            for function in item["function_inventory"]:
                self.assertIn(function["ownership"], {"native", "shell", "exception"})
                self.assertTrue(function["owner"], item["path"])

    def test_native_shell_entries_do_not_retain_shell_function_ownership(self):
        entries = load_inventory()["entries"]
        native_shell = [
            item for item in entries
            if item["runtime_authority"] == "shell-orchestration"
            and item["status"] == "done-native"
        ]
        self.assertGreater(len(native_shell), 0)
        for item in native_shell:
            self.assertEqual(
                {function["ownership"] for function in item["function_inventory"]},
                {"native"},
                item["path"],
            )

    def test_build_shell_scripts_are_terminal_non_runtime_entries(self):
        entries = load_inventory()["entries"]
        scripts = [item for item in entries if item["surface"] == "shell-script"]
        self.assertGreater(len(scripts), 0)
        for item in scripts:
            self.assertEqual(item["runtime_authority"], "build-only", item["path"])
            self.assertEqual(item["status"], "not-applicable", item["path"])
            self.assertFalse(item["runtime_active"], item["path"])
            self.assertTrue(item["function_inventory"], item["path"])

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
