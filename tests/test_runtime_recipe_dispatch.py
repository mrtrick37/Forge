"""Source-level dispatch coverage for every supported native recipe.

The high-risk recipes have deeper process-boundary tests in
``test_runtime_recipe_behavior.py``.  This suite covers the remaining route
surface without touching the host: the runtime is built from the checkout,
its external command PATH is intentionally empty, and all writable state is
redirected into a temporary XDG/HOME tree.  A route may legitimately reject
missing arguments or dependencies, but it must resolve to a Rust-owned
boundary rather than falling through to an unowned recipe.
"""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
LEDGER = ROOT / "build_files/config/runtime-recipe-migration-inventory.json"
RUNTIME = ROOT / "src/kyth-shared-rs/target/debug/kyth-runtime"


class RuntimeRecipeDispatchTest(unittest.TestCase):
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

    def test_every_explicit_recipe_resolves_without_host_execution(self) -> None:
        explicit = [
            entry
            for entry in self.ledger["entries"]
            if entry["status"] == "routed" and entry["route_kind"] == "explicit-dispatch"
        ]
        self.assertEqual(len(explicit), 105)

        with tempfile.TemporaryDirectory() as directory:
            environment = self._environment(Path(directory))
            for entry in explicit:
                name = entry["name"]
                with self.subTest(recipe=name):
                    result = subprocess.run(
                        [str(RUNTIME), "recipe", name],
                        cwd=ROOT,
                        env=environment,
                        capture_output=True,
                        text=True,
                        check=False,
                    )
                    self.assertGreaterEqual(result.returncode, 0)
                    combined = result.stdout + result.stderr
                    self.assertNotIn("has no Rust owner", combined)
                    self.assertNotIn("panicked at", combined)


if __name__ == "__main__":
    unittest.main()
