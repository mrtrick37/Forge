import importlib.util
import json
import subprocess
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CHECKER = ROOT / "build_files/scripts/check-runtime-recipe-inventory.py"
LEDGER = ROOT / "build_files/config/runtime-recipe-migration-inventory.json"


EXPECTED_MISSING_OWNER_NAMES = set()


def load_checker():
    spec = importlib.util.spec_from_file_location("runtime_recipe_inventory_checker", CHECKER)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


class RuntimeRecipeInventoryTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.checker = load_checker()
        cls.document = json.loads(LEDGER.read_text(encoding="utf-8"))

    def test_checked_in_ledger_is_current(self):
        expected = self.checker.generate()
        self.assertEqual(self.document, expected)

        result = subprocess.run(
            [sys.executable, str(CHECKER)],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("valid: 202 recipes", result.stdout)
        self.assertIn("verification: 0 routed-only, 20 behavior-tested", result.stdout)

    def test_every_native_recipe_has_one_unique_ledger_entry(self):
        recipes = self.checker.parse_recipes()
        entries = self.document["entries"]
        self.assertEqual(len(recipes), 202)
        self.assertEqual(self.document["recipe_count"], 202)
        self.assertEqual([recipe["name"] for recipe in recipes], [entry["name"] for entry in entries])
        self.assertEqual(len({entry["name"] for entry in entries}), 202)
        self.assertEqual({entry["manifest"] for entry in entries}, {"build_files/just/kyth/native.just"})

    def test_open_owner_set_matches_assessed_gap(self):
        missing = {
            entry["name"]
            for entry in self.document["entries"]
            if entry["route_kind"] == "missing-owner"
        }
        self.assertEqual(missing, EXPECTED_MISSING_OWNER_NAMES)
        self.assertEqual(self.document["summary"]["missing_owner"], 0)
        for entry in self.document["entries"]:
            if entry["name"] in missing:
                self.assertEqual(entry["assessment"], "open")
                self.assertEqual(entry["status"], "needs-rust-owner")
                self.assertIsNone(entry["rust_owner"])

    def test_route_kinds_agree_with_dispatcher_cargo_and_tunable_registry(self):
        names = {recipe["name"] for recipe in self.checker.parse_recipes()}
        explicit = self.checker.explicit_dispatch_names(names)
        binaries = self.checker.native_binary_names()
        tunables = self.checker.native_tunable_names(names)
        for entry in self.document["entries"]:
            route_kind, owner, target = self.checker.route_for(
                entry["name"], explicit, binaries, tunables
            )
            self.assertEqual(entry["route_kind"], route_kind, entry["name"])
            self.assertEqual(entry["rust_owner"], owner, entry["name"])
            self.assertEqual(entry["rust_target"], target, entry["name"])

        self.assertEqual(self.document["summary"]["routed"], 202)
        self.assertEqual(self.document["summary"]["explicit_dispatch"], 105)
        self.assertEqual(self.document["summary"]["explicit_retirement"], 3)
        self.assertEqual(self.document["summary"]["native_fallback"], 94)

    def test_verification_depth_is_separate_from_route_ownership(self):
        high_risk = {
            entry["name"]
            for entry in self.document["entries"]
            if entry["risk_tier"] in {"destructive", "privileged-writer"}
        }
        self.assertEqual(len(high_risk), 20)
        self.assertEqual(
            self.document["summary"]["verification_status"]["routed-only"],
            0,
        )
        self.assertEqual(
            self.document["summary"]["verification_status"]["behavior-tested"],
            20,
        )
        for entry in self.document["entries"]:
            if entry["name"] not in high_risk:
                continue
            self.assertEqual(entry["status"], "routed")
            self.assertEqual(entry["verification_status"], "behavior-tested")
            self.assertIn("tests/test_runtime_recipe_behavior.py", entry["behavioral_tests"])
            self.assertFalse(entry["acceptance_evidence"])

    def test_vendor_asset_recipes_are_explicitly_retired(self):
        retired = {
            entry["name"]
            for entry in self.document["entries"]
            if entry["status"] == "retired"
        }
        self.assertEqual(
            retired,
            {"install-lsfg-vk", "deploy-opticscaler", "install-umu"},
        )
        for entry in self.document["entries"]:
            if entry["name"] in retired:
                self.assertEqual(entry["route_kind"], "explicit-retirement")
                self.assertEqual(entry["assessment"], "retired")

    def test_legacy_provenance_points_to_existing_files_and_lines(self):
        for entry in self.document["entries"]:
            for source in entry["legacy_sources"]:
                path = ROOT / source["path"]
                self.assertTrue(path.is_file(), entry["name"])
                self.assertGreaterEqual(source["line"], 1)
                self.assertLessEqual(
                    source["line"], len(path.read_text(encoding="utf-8").splitlines())
                )


if __name__ == "__main__":
    unittest.main()
