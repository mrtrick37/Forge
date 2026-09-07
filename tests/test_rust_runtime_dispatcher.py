from __future__ import annotations

import json
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RUNTIME = ROOT / "src/kyth-shared-rs/src/runtime_bin.rs"
INVENTORY = ROOT / "build_files/config/runtime-migration-inventory.json"


class RustRuntimeDispatcherTest(unittest.TestCase):
    def test_compatibility_launchers_have_no_runtime_logic(self):
        inventory = json.loads(INVENTORY.read_text(encoding="utf-8"))
        native_wrappers = [
            item for item in inventory["entries"]
            if item["surface"] == "launcher"
            and item["status"] == "done-native"
            and item["current_implementation"] == "shell"
            and item["name"] in {
                "kyth-apply-update", "kyth-boot-branding-guard", "kyth-boot-verify",
                "kyth-davinci-install", "kyth-device-info", "kyth-distrobox-root-launch",
                "kyth-enroll-mok", "kyth-full-update", "kyth-gamescope",
                "kyth-greenboot-failure", "kyth-greenboot-required", "kyth-greenboot-success",
                "kyth-hw-setup", "kyth-isolate-game", "kyth-kerver", "kyth-local-bin-migrate",
                "kyth-mok-rotate", "kyth-nearby-share", "kyth-nvme-tuning", "kyth-perf-gate",
                "kyth-power-arbiter", "kyth-readahead-hint", "kyth-readahead-run",
                "kyth-retry-hardware-setup", "kyth-scx", "kyth-scx-loader",
                "kyth-session-splash-guard", "kyth-set-sleep-mode", "kyth-shader-preheat",
                "kyth-shader-prune", "kyth-snappy-bench", "kyth-storage-gate",
                "kyth-vpnc-script", "kyth-windows-friendly-defaults", "kyth-windows-import",
            }
        ]
        self.assertGreater(len(native_wrappers), 20)
        for item in native_wrappers:
            text = (ROOT / item["path"]).read_text(encoding="utf-8")
            self.assertIn("exec /usr/bin/kyth-runtime", text, item["path"])
            self.assertNotIn("rm -rf", text, item["path"])
            if item["name"] != "kyth-full-update":
                self.assertNotIn("sudo", text, item["path"])

    def test_runtime_uses_bounded_non_shell_processes(self):
        text = RUNTIME.read_text(encoding="utf-8")
        self.assertIn("run_bounded", text)
        self.assertNotIn("sh -c", text)
        self.assertNotIn("bash -c", text)
        self.assertIn("write_atomic", text)
        self.assertIn("validate_token", text)

    def test_runtime_binary_compiles(self):
        result = subprocess.run(
            ["cargo", "check", "--manifest-path", "src/kyth-shared-rs/Cargo.toml", "--bin", "kyth-runtime"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_packaging_builds_and_installs_runtime_binary(self):
        dockerfile = (ROOT / "Dockerfile").read_text(encoding="utf-8")
        self.assertIn("--bin kyth-runtime", dockerfile)
        self.assertIn("/build/kyth-runtime /usr/bin/kyth-runtime", dockerfile)


if __name__ == "__main__":
    unittest.main()
