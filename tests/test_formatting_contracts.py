"""Regression tests for repository formatting contracts."""
from __future__ import annotations

import pathlib
import shutil
import subprocess
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]


def _cargo_manifests() -> list[pathlib.Path]:
    result = subprocess.run(
        ["git", "ls-files", "-z", "*Cargo.toml"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return [
        ROOT / pathlib.Path(raw.decode("utf-8"))
        for raw in result.stdout.split(b"\0")
        if raw
    ]


class FormattingContractTests(unittest.TestCase):
    def test_all_tracked_rust_manifests_are_rustfmt_clean(self):
        """Every Rust project must stay clean under the same formatter gate."""
        cargo = shutil.which("cargo")
        if cargo is None:
            self.fail("cargo is required to verify Rust formatting")

        manifests = _cargo_manifests()
        self.assertGreater(len(manifests), 0, "no tracked Cargo.toml files found")
        failures: list[str] = []
        for manifest in manifests:
            result = subprocess.run(
                [cargo, "fmt", "--manifest-path", str(manifest), "--all", "--", "--check"],
                cwd=ROOT,
                capture_output=True,
                text=True,
                timeout=120,
            )
            if result.returncode != 0:
                detail = (result.stdout + result.stderr).strip()
                failures.append(f"{manifest.relative_to(ROOT)}:\n{detail}")

        self.assertEqual(
            failures,
            [],
            "Rust formatting drift detected; run `just format-rust`:\n" + "\n".join(failures),
        )


if __name__ == "__main__":
    unittest.main()
