"""Executable process-boundary tests for the high-risk runtime recipes.

These tests deliberately stop at the native runtime boundary.  They replace
the external system tools with deterministic helpers, record the argv vectors,
and use a temporary HOME.  This verifies refusal, dependency, failure, and
fixed-command behavior without touching the host.  Disposable-device and
exact-image effects remain separate acceptance gates.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "build_files/just/kyth/native.just"
LEDGER = ROOT / "build_files/config/runtime-recipe-migration-inventory.json"
VERIFICATION = ROOT / "build_files/config/runtime-recipe-verification.json"
RUNTIME = ROOT / "src/kyth-shared-rs/target/debug/kyth-runtime"


FAKE_COMMAND = r'''#!/usr/bin/env python3
import json
import os
import sys
from pathlib import Path

log = os.environ.get("KYTH_RUNTIME_TEST_LOG")
if log:
    with Path(log).open("a", encoding="utf-8") as stream:
        json.dump({"command": Path(sys.argv[0]).name, "args": sys.argv[1:]}, stream)
        stream.write("\n")

if Path(sys.argv[0]).name == "efibootmgr":
    print("Boot0007* Windows Boot Manager")
if Path(sys.argv[0]).name == "fwupdmgr":
    if sys.argv[1:] == ["get-updates"] and os.environ.get("KYTH_RUNTIME_FAKE_FWUPD_UPDATES"):
        print("Device ID: fake-device")
    if sys.argv[1:] and sys.argv[1] == "update":
        output = os.environ.get("KYTH_RUNTIME_FAKE_FWUPD_UPDATE_OUTPUT")
        if output:
            print(output)
        if os.environ.get("KYTH_RUNTIME_FAKE_FWUPD_UPDATE_EXIT"):
            sys.exit(int(os.environ["KYTH_RUNTIME_FAKE_FWUPD_UPDATE_EXIT"]))
failed_command = os.environ.get("KYTH_RUNTIME_FAKE_FAIL_COMMAND")
if failed_command and (
    Path(sys.argv[0]).name == failed_command
    or (len(sys.argv) > 1 and sys.argv[1] == failed_command)
):
    sys.exit(1)
sys.exit(int(os.environ.get("KYTH_RUNTIME_FAKE_EXIT", "0")))
'''


class RuntimeRecipeBehaviorTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        subprocess.run(
            [
                "cargo",
                "build",
                "--manifest-path",
                "src/kyth-shared-rs/Cargo.toml",
                "--bin",
                "kyth-runtime",
            ],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
        if not RUNTIME.is_file():
            raise AssertionError(f"runtime binary was not built: {RUNTIME}")

    def _environment(self, temporary: Path, *, fake_exit: int = 0) -> tuple[dict[str, str], Path]:
        fake_bin = temporary / "fake-bin"
        fake_bin.mkdir(parents=True)
        for command in (
            "sudo",
            "systemctl",
            "efibootmgr",
            "fwupdmgr",
            "kcmshell6",
            "systemsettings",
            "rpm-ostree",
            "timedatectl",
            "bootc",
        ):
            path = fake_bin / command
            path.write_text(FAKE_COMMAND, encoding="utf-8")
            path.chmod(0o755)
        log = temporary / "commands.jsonl"
        environment = os.environ.copy()
        environment["HOME"] = str(temporary / "home")
        environment["PATH"] = f"{fake_bin}:{environment.get('PATH', '')}"
        environment["KYTH_RUNTIME_TEST_LOG"] = str(log)
        environment["KYTH_RUNTIME_FAKE_EXIT"] = str(fake_exit)
        return environment, log

    def _run(
        self,
        recipe: str,
        args: list[str],
        environment: dict[str, str],
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [str(RUNTIME), "recipe", recipe, *args],
            cwd=ROOT,
            env=environment,
            capture_output=True,
            text=True,
            check=False,
        )

    @staticmethod
    def _records(log: Path) -> list[dict[str, list[str]]]:
        if not log.exists():
            return []
        return [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()]

    def test_guarded_routes_execute_only_fixed_external_argv(self) -> None:
        cases = {
            "setup-waydroid": ([], "waydroid"),
            "remove-waydroid": (["--confirm"], "rm"),
            "setup-printer": ([], "systemctl"),
            "firmware-update": ([], "fwupdmgr"),
            "setup-boot-windows-steam": ([], "install"),
            "fix-dualboot-clock": ([], "timedatectl"),
            "install-racing-wheel-drivers": ([], "rpm-ostree"),
            "install-asus-tools": ([], "rpm-ostree"),
            "install-nvidia-driver": ([], "rpm-ostree"),
            "install-displaylink": ([], "rpm-ostree"),
            "setup-vr": ([], "rpm-ostree"),
            "retry-quarantined-update": (["sha256:abc"], "kyth-boot-health"),
            "rebase": (["kyth:testing"], "bootc"),
            "switch-channel": (["testing"], "kyth-bootc-guard"),
            "switch-channel-impl": (["testing"], "kyth-bootc-guard"),
            "switch-kernel": (["cachy"], "kyth-bootc-guard"),
        }
        with tempfile.TemporaryDirectory() as directory:
            environment, log = self._environment(Path(directory))
            for recipe, (args, expected_arg) in cases.items():
                with self.subTest(recipe=recipe):
                    if recipe == "remove-waydroid":
                        (Path(environment["HOME"]) / ".waydroid").mkdir(parents=True)
                    result = self._run(recipe, args, environment)
                    self.assertEqual(
                        result.returncode,
                        0,
                        f"{recipe}: stdout={result.stdout!r} stderr={result.stderr!r}",
                    )
                    self.assertTrue(
                        any(
                            expected_arg == record["command"]
                            or any(
                                Path(argument).name == expected_arg
                                for argument in record["args"]
                            )
                            for record in self._records(log)
                        ),
                        f"{recipe} did not issue the expected fixed command",
                    )

    def test_high_risk_routes_preserve_exact_fixed_command_shapes(self) -> None:
        expected = {
            "setup-waydroid": [
                ("sudo", ["rpm-ostree", "install", "--idempotent", "waydroid"]),
                (
                    "systemctl",
                    ["--user", "enable", "--now", "waydroid-container.service"],
                ),
                ("sudo", ["waydroid", "init", "-s", "GAPPS"]),
            ],
            "setup-printer": [
                ("sudo", ["systemctl", "enable", "--now", "cups"]),
                ("kcmshell6", ["kcm_printer_manager"]),
            ],
            "firmware-update": [
                ("fwupdmgr", ["refresh", "--force"]),
                ("fwupdmgr", ["get-updates"]),
            ],
            "fix-dualboot-clock": [
                (
                    "sudo",
                    ["timedatectl", "set-local-rtc", "1", "--adjust-system-clock"],
                )
            ],
            "retry-quarantined-update": [
                (
                    "sudo",
                    ["kyth-boot-health", "clear-quarantine", "--digest", "sha256:abc"],
                )
            ],
            "rebase": [
                ("sudo", ["bootc", "switch", "ghcr.io/kyth-os/kyth:testing"]),
                ("sudo", ["/usr/bin/kyth-finalize-staged"]),
            ],
            "switch-channel": [
                ("sudo", ["/usr/bin/kyth-bootc-guard", "switch-testing"])
            ],
            "switch-channel-impl": [
                ("sudo", ["/usr/bin/kyth-bootc-guard", "switch-testing"])
            ],
        }
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory)
            for recipe, commands in expected.items():
                with self.subTest(recipe=recipe):
                    environment, log = self._environment(temporary / recipe)
                    if recipe == "remove-waydroid":
                        (Path(environment["HOME"]) / ".waydroid").mkdir(parents=True)
                    result = self._run(
                        recipe,
                        ["sha256:abc"] if recipe == "retry-quarantined-update" else (
                            ["testing"] if recipe in {"switch-channel", "switch-channel-impl"} else (
                                ["kyth:testing"] if recipe == "rebase" else []
                            )
                        ),
                        environment,
                    )
                    self.assertEqual(result.returncode, 0, result.stderr)
                    records = self._records(log)
                    for command, args in commands:
                        self.assertIn({"command": command, "args": args}, records)

    def test_package_routes_stage_expected_packages_and_report_reboot(self) -> None:
        expected = {
            "install-racing-wheel-drivers": (
                [
                    "akmod-hid-tmff2",
                    "akmod-new-lg4ff",
                    "akmod-hid-fanatecff",
                    "akmod-t150-driver",
                ],
                True,
            ),
            "install-asus-tools": (["asusctl", "supergfxctl", "rog-control-center"], False),
            "install-nvidia-driver": (["akmod-nvidia"], False),
            "install-displaylink": (["displaylink"], False),
            "setup-vr": (["openxr-loader"], False),
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for recipe, (packages, allow_inactive) in expected.items():
                with self.subTest(recipe=recipe):
                    environment, log = self._environment(root / recipe)
                    result = self._run(recipe, [], environment)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    records = self._records(log)
                    installs = [record for record in records if record["command"] == "sudo"]
                    self.assertEqual(len(installs), 1)
                    expected_args = ["rpm-ostree", "install", "--idempotent"]
                    if allow_inactive:
                        expected_args.append("--allow-inactive")
                    expected_args.extend(packages)
                    self.assertEqual(installs[0]["args"], expected_args)
                    self.assertIn(
                        {"command": "rpm-ostree", "args": ["status", "--json"]},
                        records,
                    )
                    self.assertIn("reboot", result.stdout.lower())

    def test_high_risk_child_failures_never_report_success(self) -> None:
        cases = {
            "ai-dev-remove": [],
            "firmware-update": [],
            "fix-dualboot-clock": [],
            "hardware-policy": [],
            "hardware-policy-apply": [],
            "install-asus-tools": [],
            "install-displaylink": [],
            "install-nvidia-driver": [],
            "install-racing-wheel-drivers": [],
            "rebase": ["kyth:testing"],
            "reclaim-windows": [],
            "remove-waydroid": ["--confirm"],
            "retry-quarantined-update": ["sha256:abc"],
            "setup-boot-windows-steam": [],
            "setup-printer": [],
            "setup-vr": [],
            "setup-waydroid": [],
            "switch-channel": ["testing"],
            "switch-channel-impl": ["testing"],
            "switch-kernel": ["cachy"],
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for recipe, args in cases.items():
                with self.subTest(recipe=recipe):
                    environment, log = self._environment(root / recipe, fake_exit=23)
                    if recipe == "remove-waydroid":
                        (Path(environment["HOME"]) / ".waydroid").mkdir(parents=True)
                    result = self._run(recipe, args, environment)
                    self.assertNotEqual(result.returncode, 0)

    def test_repeated_mutations_are_idempotent_or_recover_cleanly(self) -> None:
        package = "install-nvidia-driver"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            environment, log = self._environment(root / package)
            first = self._run(package, [], environment)
            second = self._run(package, [], environment)
            self.assertEqual(first.returncode, 0, first.stderr)
            self.assertEqual(second.returncode, 0, second.stderr)
            installs = [record for record in self._records(log) if record["command"] == "sudo"]
            self.assertEqual(len(installs), 2)
            self.assertEqual(installs[0]["args"], installs[1]["args"])

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            environment, log = self._environment(root)
            user_data = Path(environment["HOME"]) / ".waydroid"
            user_data.mkdir(parents=True)
            self.assertEqual(self._run("remove-waydroid", ["--confirm"], environment).returncode, 0)
            second = self._run("remove-waydroid", ["--confirm"], environment)
            self.assertEqual(second.returncode, 0, second.stderr)
            self.assertIn("already absent", second.stdout)
            self.assertEqual(len(self._records(log)), 4)

    def test_firmware_output_is_redacted_at_the_runtime_boundary(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            environment, _ = self._environment(Path(directory))
            environment["KYTH_RUNTIME_FAKE_FWUPD_UPDATES"] = "1"
            environment["KYTH_RUNTIME_FAKE_FWUPD_UPDATE_OUTPUT"] = (
                "password=super-secret token=another-secret"
            )
            environment["KYTH_RUNTIME_FAKE_FWUPD_UPDATE_EXIT"] = "23"
            result = self._run("firmware-update", [], environment)
            combined = result.stdout + result.stderr
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn("super-secret", combined)
            self.assertNotIn("another-secret", combined)
            self.assertIn("<redacted>", combined)

    def test_invalid_high_risk_requests_fail_before_external_commands(self) -> None:
        cases = [
            ("remove-waydroid", []),
            ("setup-waydroid", ["one", "two"]),
            ("setup-printer", ["unexpected"]),
            ("firmware-update", ["unexpected"]),
            ("setup-boot-windows-steam", ["unexpected"]),
            ("fix-dualboot-clock", ["unexpected"]),
            ("install-nvidia-driver", ["unexpected"]),
            ("setup-vr", ["unexpected"]),
            ("retry-quarantined-update", ["bad\nvalue"]),
            ("rebase", ["not-an-image"]),
            ("switch-channel", ["unknown"]),
            ("switch-channel-impl", ["unknown"]),
            ("switch-kernel", ["unknown"]),
            ("reclaim-windows", ["unexpected"]),
        ]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for recipe, args in cases:
                with self.subTest(recipe=recipe):
                    environment, log = self._environment(root / recipe)
                    result = self._run(recipe, args, environment)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(self._records(log), [])

    def test_confirmation_validation_and_dependency_failures_are_observable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory)
            environment, log = self._environment(temporary)

            refused = self._run("remove-waydroid", [], environment)
            self.assertEqual(refused.returncode, 2)
            self.assertEqual(self._records(log), [])

            invalid_rebase = self._run("rebase", ["not-an-image"], environment)
            self.assertNotEqual(invalid_rebase.returncode, 0)
            invalid_channel = self._run("switch-channel", ["unknown"], environment)
            self.assertNotEqual(invalid_channel.returncode, 0)
            invalid_kernel = self._run("switch-kernel", ["unknown"], environment)
            self.assertNotEqual(invalid_kernel.returncode, 0)

            unavailable = self._run("hardware-policy-apply", [], environment)
            self.assertNotEqual(unavailable.returncode, 0)

            unavailable_routes = {
                "ai-dev-remove",
                "hardware-policy",
                "hardware-policy-apply",
                "reclaim-windows",
            }
            for recipe in unavailable_routes:
                with self.subTest(unavailable_recipe=recipe):
                    result = self._run(recipe, [], environment)
                    self.assertNotEqual(result.returncode, 0)

            ledger = json.loads(LEDGER.read_text(encoding="utf-8"))
            high_risk = {
                entry["name"]
                for entry in ledger["entries"]
                if entry["risk_tier"] in {"destructive", "privileged-writer"}
            }
            self.assertEqual(
                high_risk,
                {
                    "ai-dev-remove",
                    "firmware-update",
                    "fix-dualboot-clock",
                    "hardware-policy",
                    "hardware-policy-apply",
                    "install-asus-tools",
                    "install-displaylink",
                    "install-nvidia-driver",
                    "install-racing-wheel-drivers",
                    "rebase",
                    "reclaim-windows",
                    "remove-waydroid",
                    "retry-quarantined-update",
                    "setup-boot-windows-steam",
                    "setup-printer",
                    "setup-vr",
                    "setup-waydroid",
                    "switch-channel",
                    "switch-channel-impl",
                    "switch-kernel",
                },
            )

    def test_windows_boot_setup_installs_helper_and_sudoers_as_one_contract(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            environment, log = self._environment(Path(directory))
            result = self._run("setup-boot-windows-steam", [], environment)
            self.assertEqual(result.returncode, 0, result.stderr)
            installs = [
                record
                for record in self._records(log)
                if record["command"] == "sudo"
                and record["args"]
                and record["args"][0] == "install"
            ]
            self.assertEqual(len(installs), 2)
            self.assertTrue(
                any("/usr/local/bin/boot-windows" in record["args"] for record in installs)
            )
            self.assertTrue(
                any("/etc/sudoers.d/kyth-boot-windows" in record["args"] for record in installs)
            )

    def test_rebase_finalizes_only_after_bootc_switch_succeeds(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            environment, log = self._environment(Path(directory))
            result = self._run("rebase", ["kyth:testing"], environment)
            self.assertEqual(result.returncode, 0, result.stderr)
            sudo = [record for record in self._records(log) if record["command"] == "sudo"]
            self.assertEqual(
                [Path(record["args"][0]).name for record in sudo],
                ["bootc", "kyth-finalize-staged"],
            )

    def test_switch_channel_dry_run_uses_default_stable_channel(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            environment, log = self._environment(Path(directory))
            result = self._run("switch-channel", ["--dry-run"], environment)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(":latest", result.stdout)

    def test_waydroid_removal_restores_user_data_when_privileged_delete_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory)
            environment, log = self._environment(temporary)
            user_data = Path(environment["HOME"]) / ".waydroid"
            user_data.mkdir(parents=True)
            (user_data / "marker").write_text("keep", encoding="utf-8")
            environment["KYTH_RUNTIME_FAKE_FAIL_COMMAND"] = "rm"

            result = self._run("remove-waydroid", ["--confirm"], environment)

            self.assertNotEqual(result.returncode, 0)
            self.assertTrue((user_data / "marker").is_file())
            self.assertEqual(
                list(user_data.parent.glob(".kyth-waydroid-removal-*")),
                [],
            )
    def test_external_failure_is_returned_to_the_recipe_caller(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            environment, log = self._environment(Path(directory), fake_exit=23)
            result = self._run("install-nvidia-driver", [], environment)
            self.assertNotEqual(result.returncode, 0)
            self.assertTrue(self._records(log))

    def test_high_risk_registry_is_explicit_and_behaviorally_tested(self) -> None:
        ledger = json.loads(LEDGER.read_text(encoding="utf-8"))
        verification = json.loads(VERIFICATION.read_text(encoding="utf-8"))
        high_risk = {
            entry["name"]
            for entry in ledger["entries"]
            if entry["risk_tier"] in {"destructive", "privileged-writer"}
        }
        self.assertEqual(high_risk, set(verification["recipes"]))
        for name, entry in verification["recipes"].items():
            self.assertEqual(entry["verification_status"], "behavior-tested", name)
            self.assertIn("tests/test_runtime_recipe_behavior.py", entry["behavioral_tests"])
            self.assertFalse(entry["acceptance_evidence"], name)


if __name__ == "__main__":
    unittest.main()
