import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RUNTIME = ROOT / "src/kyth-shared-rs/src/runtime_bin.rs"
LEDGER = ROOT / "build_files/config/runtime-recipe-migration-inventory.json"


class RuntimeRecipeParityTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.runtime = RUNTIME.read_text(encoding="utf-8")
        cls.ledger = json.loads(LEDGER.read_text(encoding="utf-8"))

    def test_ai_dev_recipe_actions_reach_native_binary(self):
        for recipe, action in {
            "ai-dev-status": "status",
            "ai-dev-setup": "setup",
            "ai-dev-enter": "enter",
            "ai-dev-start": "start",
            "ai-dev-stop": "stop",
            "ai-dev-remove": "remove",
        }.items():
            self.assertIn(f'"{recipe}"', self.runtime)
            self.assertIn(f'vec!["{action}".into()]', self.runtime)
        self.assertIn('"_ai-dev" => ("ai-dev", forwarded.to_vec())', self.runtime)
        self.assertIn('"ai-dev" => run("/usr/bin/kyth-ai-dev", args)', self.runtime)

    def test_flatpak_recipe_catalog_preserves_legacy_app_ids(self):
        applications = {
            "install-boxbuddy": "io.github.dvlv.boxbuddyrs",
            "install-steam": "com.valvesoftware.Steam",
            "install-lutris": "net.lutris.Lutris",
            "install-heroic": "com.heroicgameslauncher.hgl",
            "install-bottles": "com.usebottles.bottles",
            "install-prismlauncher": "org.prismlauncher.PrismLauncher",
            "install-itch": "io.itch.itch",
            "install-retroarch": "org.libretro.RetroArch",
            "install-ludusavi": "com.github.mtkennerly.ludusavi",
            "install-lact": "io.github.ilya_zlobintsev.LACT",
            "install-piper": "org.freedesktop.Piper",
            "install-openrgb": "org.openrgb.OpenRGB",
            "install-solaar": "io.github.pwr_solaar.solaar",
            "install-oversteer": "org.berarma.Oversteer",
            "install-vesktop": "dev.vencord.Vesktop",
            "install-gpu-screen-recorder": "com.dec05eba.gpu_screen_recorder",
            "install-goverlay": "io.github.benjamimgois.goverlay",
            "install-mangojuice": "io.github.radiolamp.mangojuice",
            "install-obs": "com.obsproject.Studio",
        }
        for recipe, app_id in applications.items():
            self.assertIn(f'"{recipe}"', self.runtime)
            self.assertIn(f'"{app_id}".into()', self.runtime)
        self.assertIn('"install-flatpak" => flatpak_install(args)', self.runtime)
        self.assertIn('"remote-add".into()', self.runtime)
        self.assertIn('"install".into()', self.runtime)

    def test_existing_native_owner_routes_preserve_operation_arguments(self):
        contracts = {
            '"hardware-inventory" => ("hardware-policy", vec!["inventory".into()])':
                '"hardware-policy" => run("/usr/bin/kyth-hardware-policy", args)',
            '"hardware-policy-apply" => (':
                '"hardware-policy" => run("/usr/bin/kyth-hardware-policy", args)',
            '"export-steam-games" => ("steam-game-export", forwarded.to_vec())':
                '"steam-game-export" => run("/usr/bin/kyth-steam-game-export", args)',
            '"setup-tailscale" => ("apply-tailscale", forwarded.to_vec())':
                '"apply-tailscale" => run("/usr/bin/kyth-apply-tailscale", args)',
            '"update-proton-cachyos" => ("proton-cachyos-update", forwarded.to_vec())':
                '"proton-cachyos-update" => run("/usr/bin/kyth-proton-cachyos-update", args)',
        }
        for recipe_contract, owner_contract in contracts.items():
            self.assertIn(recipe_contract, self.runtime)
            self.assertIn(owner_contract, self.runtime)

    def test_environment_writers_are_atomic_and_user_scoped(self):
        self.assertIn("fn toggle_environment_file", self.runtime)
        self.assertIn('home().join(".config/environment.d")', self.runtime)
        self.assertIn("write_atomic(&path, content)", self.runtime)
        self.assertIn('"99-kyth-fsr4.conf"', self.runtime)
        self.assertIn('"99-kyth-nvapi.conf"', self.runtime)
        self.assertIn("obs-vkcapture.conf", self.runtime)

    def test_open_entries_remain_explicit_until_their_owner_is_implemented(self):
        open_entries = [
            entry for entry in self.ledger["entries"] if entry["status"] == "needs-rust-owner"
        ]
        self.assertEqual(open_entries, [])

    def test_every_routed_recipe_has_dispatch_boundary_coverage(self):
        for entry in self.ledger["entries"]:
            if entry["status"] != "routed":
                continue
            self.assertIn(
                "tests/test_runtime_recipe_dispatch.py",
                entry["route_contract_tests"],
                entry["name"],
            )

    def test_completed_recipe_families_have_explicit_dispatch_routes(self):
        completed = {
            "setup-kali-box", "export-kali-apps", "setup-waydroid", "remove-waydroid",
            "startup-apps", "install-ms-fonts", "setup-printer", "firmware-update",
            "setup-kyth-dev-box", "install-vscode", "install-jetbrains-toolbox",
            "setup-boot-windows-steam", "dualboot-status", "reclaim-windows",
            "fix-dualboot-clock", "install-battlenet", "install-epic-launcher",
            "install-ea-app", "install-ubisoft-connect", "corectrl",
            "install-racing-wheel-drivers", "install-asus-tools", "install-lsfg-vk",
            "deploy-opticscaler", "install-umu", "gaming-stack-status",
            "install-nvidia-driver", "install-displaylink", "game-performance",
            "game-performance-profile", "zink-run", "low-latency", "enable-bpftune",
            "disable-bpftune", "setup-vr", "retry-quarantined-update", "rebase",
            "switch-channel", "switch-channel-impl", "switch-kernel",
        }
        for recipe in completed:
            self.assertIn(f'"{recipe}" =>', self.runtime, recipe)

    def test_destructive_and_update_routes_have_refusal_or_validation_guards(self):
        self.assertIn('args != ["--confirm"]', self.runtime)
        self.assertIn('validate_token(input, "image reference")', self.runtime)
        self.assertIn('validate_token(&args[0], "digest")', self.runtime)
        self.assertIn('"--dry-run"', self.runtime)

    def test_high_risk_recipe_routes_have_explicit_parity_coverage(self):
        high_risk = {
            entry["name"]
            for entry in self.ledger["entries"]
            if entry["risk_tier"] in {"destructive", "privileged-writer"}
        }
        self.assertEqual(len(high_risk), 20)
        for entry in self.ledger["entries"]:
            if entry["name"] not in high_risk:
                continue
            self.assertEqual(entry["status"], "routed", entry["name"])
            self.assertIn(
                entry["route_kind"],
                {"explicit-dispatch", "native-fallback"},
                entry["name"],
            )
            self.assertEqual(
                entry["parity_tests"],
                ["tests/test_runtime_recipe_parity.py"],
                entry["name"],
            )
            self.assertIn(f'"{entry["name"]}" =>', self.runtime, entry["name"])

    def test_high_risk_routes_report_behavioral_test_coverage(self):
        for entry in self.ledger["entries"]:
            if entry["risk_tier"] not in {"destructive", "privileged-writer"}:
                continue
            self.assertEqual(entry["verification_status"], "behavior-tested", entry["name"])
            self.assertIn(
                "tests/test_runtime_recipe_behavior.py",
                entry["behavioral_tests"],
                entry["name"],
            )
            self.assertEqual(entry["acceptance_evidence"], [], entry["name"])


if __name__ == "__main__":
    unittest.main()
