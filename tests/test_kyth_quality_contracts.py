import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

class QualityContractsTests(unittest.TestCase):
    def test_quality_dependencies_are_exactly_pinned(self):
        requirements = (ROOT / "requirements-quality.txt").read_text().splitlines()
        self.assertTrue(requirements)
        self.assertTrue(all(line.count("==") == 1 for line in requirements))
        justfile = (ROOT / "Justfile").read_text()
        self.assertIn("setup-quality:", justfile)
        self.assertIn(".venv-quality/bin/python", justfile)

    def test_validation_publishes_coverage_even_on_failure(self):
        workflow = (ROOT / ".github/workflows/validation.yml").read_text()
        self.assertIn("./build_files/scripts/run-quality.sh", workflow)
        quality_job = re.split(
            r"^  [a-z][a-z0-9-]*:$",
            workflow.split("  quality:\n", 1)[1],
            maxsplit=1,
            flags=re.MULTILINE,
        )[0]
        self.assertNotIn("run: just ", quality_job)
        self.assertIn("run-quality.sh", quality_job)
        self.assertIn("if: always()", workflow)
        self.assertIn("coverage.xml", workflow)

    def test_validation_job_configures_rust_for_runtime_compile_tests(self):
        workflow = (ROOT / ".github/workflows/validation.yml").read_text()
        validation_job = workflow.split("  validation:\n", 1)[1].split("\n  quality:\n", 1)[0]
        self.assertIn("rustup toolchain install stable", validation_job)
        self.assertIn("rustup default stable", validation_job)
        self.assertIn("cargo --version", validation_job)

    def test_validation_preserves_rustup_home_when_isolating_test_home(self):
        validation = (ROOT / "build_files/scripts/validate.sh").read_text()
        self.assertIn('rustup_home="${RUSTUP_HOME:-${HOME}/.rustup}"', validation)
        self.assertIn('cargo_home="${CARGO_HOME:-${HOME}/.cargo}"', validation)
        self.assertIn('export RUSTUP_HOME="${rustup_home}"', validation)
        self.assertIn('export CARGO_HOME="${cargo_home}"', validation)

    def test_pre_push_runs_the_same_quality_gate_as_ci(self):
        preflight = (ROOT / "build_files/scripts/ci-preflight.sh").read_text()
        self.assertIn("./build_files/scripts/run-quality.sh", preflight)

    def test_snapshot_preflight_uses_native_owner(self):
        preflight = (ROOT / "build_files/scripts/ci-preflight.sh").read_text()
        self.assertIn("kyth-snapshot-timeline", preflight)
        self.assertNotIn("from kyth_shared.snapshot_timeline", preflight)

        launcher = (ROOT / "build_files/kyth-snapshot-timeline").read_text()
        self.assertTrue(launcher.startswith("#!/usr/bin/env bash"))
        self.assertNotIn("kyth_shared.snapshot_timeline", launcher)

    def test_validation_tool_archives_do_not_require_archive_owners(self):
        installer = (ROOT / "build_files/scripts/install-validation-tools.sh").read_text()
        self.assertIn("--no-same-owner", installer)
        self.assertIn('SHELLCHECK_VERSION="${SHELLCHECK_VERSION:-', installer)
        self.assertIn('download_and_verify "shellcheck"', installer)

    def test_critical_modules_have_explicit_thresholds(self):
        gate = (ROOT / "build_files/config/coverage-floors.json").read_text()
        for module in (
            "installer_service.py", "recovery.py", "privileged.py", "updates.py",
            "windows_installer.py", "thirdparty.py", "user_polish.py", "vm_acceptance.py",
        ):
            self.assertIn(module, gate)

    def test_readme_is_not_rewritten_with_volatile_git_metadata(self):
        hook = (ROOT / ".githooks/pre-commit").read_text()
        readme = (ROOT / "README.md").read_text()
        self.assertNotIn("update-readme-snapshot", hook)
        self.assertNotIn("AUTO-README-START", readme)

    def test_optimization_budgets_are_part_of_validation(self):
        validation = (ROOT / "build_files/scripts/validate.sh").read_text()
        report = ROOT / "build_files/scripts/optimization-report.py"
        budgets = ROOT / "build_files/config/optimization-budgets.json"
        self.assertIn("optimization-report.py --check", validation)
        self.assertTrue(report.is_file())
        self.assertTrue(budgets.is_file())

    def test_retired_python_hub_ui_is_absent(self):
        package_root = ROOT / "src/kyth-welcome/kyth_welcome"
        self.assertFalse((ROOT / "src/kyth-welcome/kyth-welcome").exists())
        self.assertFalse((package_root / "app.py").exists())
        self.assertFalse((package_root / "page_registry.py").exists())
        self.assertFalse((package_root / "windows.py").exists())
        metadata = (ROOT / "src/kyth-welcome/pyproject.toml").read_text()
        self.assertNotIn("PySide6", metadata)
        self.assertNotIn("kyth_welcome.app", metadata)

    def test_transitional_package_contains_no_hub_pages(self):
        package_root = ROOT / "src/kyth-welcome/kyth_welcome"
        self.assertEqual(
            {path.name for path in package_root.glob("page_*.py")},
            set(),
        )
        wizard_dir = package_root / "wizard"
        self.assertFalse(
            wizard_dir.is_dir()
            and any(path.name != "__pycache__" for path in wizard_dir.iterdir())
        )
        self.assertTrue((package_root / "services").is_dir())

if __name__ == "__main__":
    unittest.main()
