import importlib.util
import json
import subprocess
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CHECKER = ROOT / "build_files/scripts/check-runtime-recipe-inventory.py"
LEDGER = ROOT / "build_files/config/runtime-recipe-migration-inventory.json"


EXPECTED_MISSING_OWNER_NAMES = {
    "_install-flatpak",
    "setup-kali-box",
    "export-kali-apps",
    "setup-waydroid",
    "remove-waydroid",
    "install-boxbuddy",
    "_ai-dev",
    "ai-dev-status",
    "ai-dev-setup",
    "ai-dev-enter",
    "ai-dev-start",
    "ai-dev-stop",
    "ai-dev-remove",
    "startup-apps",
    "install-ms-fonts",
    "setup-printer",
    "firmware-update",
    "setup-kyth-dev-box",
    "install-vscode",
    "install-jetbrains-toolbox",
    "setup-boot-windows-steam",
    "dualboot-status",
    "reclaim-windows",
    "fix-dualboot-clock",
    "install-battlenet",
    "install-epic-launcher",
    "install-ea-app",
    "install-ubisoft-connect",
    "install-steam",
    "install-lutris",
    "install-heroic",
    "install-bottles",
    "install-prismlauncher",
    "install-itch",
    "install-retroarch",
    "install-ludusavi",
    "hardware-inventory",
    "hardware-policy-apply",
    "export-steam-games",
    "install-lact",
    "corectrl",
    "install-coolercontrol",
    "install-piper",
    "install-openrgb",
    "install-solaar",
    "install-racing-wheel-drivers",
    "install-oversteer",
    "install-asus-tools",
    "install-vesktop",
    "install-gpu-screen-recorder",
    "install-goverlay",
    "install-mangojuice",
    "install-obs",
    "enable-obs-capture",
    "install-lsfg-vk",
    "deploy-opticscaler",
    "update-proton-cachyos",
    "install-umu",
    "toggle-fsr4",
    "toggle-nvapi",
    "gaming-stack-status",
    "install-nvidia-driver",
    "install-displaylink",
    "game-performance",
    "game-performance-profile",
    "zink-run",
    "low-latency",
    "enable-bpftune",
    "disable-bpftune",
    "apply-preset",
    "setup-sunshine",
    "setup-vr",
    "setup-tailscale",
    "retry-quarantined-update",
    "rebase",
    "switch-channel",
    "switch-channel-impl",
    "switch-kernel",
}


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
        self.assertEqual(self.document["summary"]["missing_owner"], 78)
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

        self.assertEqual(self.document["summary"]["routed"], 124)
        self.assertEqual(self.document["summary"]["explicit_dispatch"], 31)
        self.assertEqual(self.document["summary"]["native_fallback"], 93)

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
