from __future__ import annotations

import os
import pathlib
import re
import subprocess
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "build_files" / "scripts" / "cleanup-vm-acceptance.sh"


class VmAcceptanceCleanupTests(unittest.TestCase):
    def test_script_is_bash_valid_and_has_narrow_safety_contract(self):
        result = subprocess.run(["bash", "-n", str(SCRIPT)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        text = SCRIPT.read_text(encoding="utf-8")
        self.assertIn("Refusing cleanup while acceptance/build processes are active", text)
        self.assertIn("qemu-system-x86_64", text)
        self.assertIn("ps -eo pid=,comm=,args=", text)
        self.assertIn("findmnt -rn -o TARGET", text)
        self.assertIn("localhost\\/kyth-live:", text)
        self.assertIn("--external", text)
        self.assertIn('"${status}" == "Storage"', text)
        self.assertIn("*-working-container-[0-9]*", text)
        self.assertIsNone(re.search(r"(?m)^\s*(?:sudo\s+)?podman\s+system\s+(?:reset|prune)\b", text))

    def test_dry_run_lists_only_test_owned_targets(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            (root / "output" / "vm-acceptance").mkdir(parents=True)
            (root / "output" / "live-iso").mkdir(parents=True)
            (root / "tmp" / "kyth-rootful-btrfs-storage").mkdir(parents=True)
            (root / "tmp" / "unrelated").mkdir(parents=True)
            env = {
                **os.environ,
                "KYTH_CLEANUP_REPO_ROOT": str(root),
                "KYTH_CLEANUP_TMP_ROOT": str(root / "tmp"),
                "KYTH_CLEANUP_VAR_TMP_ROOT": str(root / "var-tmp"),
                "KYTH_CLEANUP_SKIP_PODMAN": "1",
            }
            result = subprocess.run(
                ["bash", str(SCRIPT), "--dry-run"],
                env=env,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(f"REMOVE {root / 'output' / 'vm-acceptance'}", result.stdout)
            self.assertIn(f"REMOVE {root / 'output' / 'live-iso'}", result.stdout)
            self.assertNotIn(str(root / "tmp" / "unrelated"), result.stdout)

    def test_cleanup_removes_known_targets_but_not_unrelated_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            disposable = [
                root / "output" / "vm-acceptance",
                root / "output" / "live-iso",
                root / "tmp" / "kyth-rootful-btrfs-storage",
                root / "tmp" / "kyth-rootful-btrfs-run",
                root / "tmp" / "kyth-podman-test-root",
                root / "tmp" / "kyth-podman-test-run",
                root / "tmp" / "kyth-container-tmp",
            ]
            for path in disposable:
                path.mkdir(parents=True)
                (path / "payload").write_text("test", encoding="utf-8")
            unrelated = root / "tmp" / "bib-debug"
            unrelated.mkdir(parents=True)
            (unrelated / "keep").write_text("keep", encoding="utf-8")
            env = {
                **os.environ,
                "KYTH_CLEANUP_REPO_ROOT": str(root),
                "KYTH_CLEANUP_TMP_ROOT": str(root / "tmp"),
                "KYTH_CLEANUP_VAR_TMP_ROOT": str(root / "var-tmp"),
                "KYTH_CLEANUP_SKIP_PODMAN": "1",
            }
            result = subprocess.run(
                ["bash", str(SCRIPT)],
                env=env,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            for path in disposable:
                self.assertFalse(path.exists(), path)
            self.assertTrue((unrelated / "keep").is_file())


if __name__ == "__main__":
    unittest.main()
