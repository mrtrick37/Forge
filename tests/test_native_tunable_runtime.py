"""Process-boundary coverage for the native tunable dispatcher."""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
LEDGER = ROOT / "build_files/config/runtime-recipe-migration-inventory.json"
TUNABLE = ROOT / "src/kyth-shared-rs/target/debug/kyth-tunable-rs"


class NativeTunableRuntimeTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        subprocess.run(
            [
                "cargo",
                "build",
                "--manifest-path",
                "src/kyth-shared-rs/Cargo.toml",
                "--bin",
                "kyth-tunable-rs",
            ],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
        if not TUNABLE.is_file():
            raise AssertionError(f"tunable binary was not built: {TUNABLE}")
        cls.ledger = json.loads(LEDGER.read_text(encoding="utf-8"))

    @staticmethod
    def _environment(root: Path) -> dict[str, str]:
        config = root / "config"
        runtime = root / "runtime"
        home = root / "home"
        config.mkdir(parents=True)
        runtime.mkdir(parents=True)
        home.mkdir(parents=True)

        environment = os.environ.copy()
        environment.update(
            {
                "HOME": str(home),
                "PATH": str(root / "empty-path"),
                "XDG_CONFIG_HOME": str(config),
                "XDG_RUNTIME_DIR": str(runtime),
                "KYTH_TEST_MODE": "1",
            }
        )
        return environment

    def _run(
        self,
        name: str,
        action: str,
        environment: dict[str, str],
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [str(TUNABLE), name, action],
            cwd=ROOT,
            env=environment,
            capture_output=True,
            text=True,
            check=False,
        )

    def test_all_native_tunables_support_isolated_status_and_apply(self) -> None:
        listed = subprocess.run(
            [str(TUNABLE), "--list-native"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
        )
        native_names = {
            line.strip() for line in listed.stdout.splitlines() if line.strip()
        }
        self.assertEqual(len(native_names), 94)

        fallback_routes = {
            entry["name"]
            for entry in self.ledger["entries"]
            if entry["rust_owner"] == "native::kyth-tunable-rs"
        }
        self.assertEqual(len(fallback_routes), 91)
        self.assertTrue(fallback_routes <= native_names)

        with tempfile.TemporaryDirectory() as directory:
            environment = self._environment(Path(directory))
            for name in sorted(native_names):
                with self.subTest(tunable=name):
                    status = self._run(name, "status", environment)
                    self.assertEqual(
                        status.returncode,
                        0,
                        f"status failed: stdout={status.stdout!r} stderr={status.stderr!r}",
                    )
                    apply = None
                    if name not in {"mimalloc-run", "windows-verify"}:
                        apply = self._run(name, "apply", environment)
                        self.assertEqual(
                            apply.returncode,
                            0,
                            f"apply failed: stdout={apply.stdout!r} stderr={apply.stderr!r}",
                        )
                    combined = status.stdout + status.stderr
                    if apply is not None:
                        combined += apply.stdout + apply.stderr
                    self.assertNotIn("native Rust implementation is not ready", combined)
                    self.assertNotIn("panicked at", combined)


if __name__ == "__main__":
    unittest.main()
