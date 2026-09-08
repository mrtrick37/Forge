import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
ACCEPTANCE = ROOT / "build_files/scripts/run-rust-migration-acceptance.sh"
GATES = ROOT / "docs/rust-migration-acceptance-gates.md"
JUSTFILE = ROOT / "Justfile"
ARTIFACT_WORKFLOW = ROOT / ".github/workflows/rust-migration-acceptance.yml"


class RustMigrationAcceptanceContractTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.script = ACCEPTANCE.read_text(encoding="utf-8")
        cls.gates = GATES.read_text(encoding="utf-8")
        cls.justfile = JUSTFILE.read_text(encoding="utf-8")
        cls.artifact_workflow = ARTIFACT_WORKFLOW.read_text(encoding="utf-8")

    def test_wrapper_requires_exact_iso_and_image_and_preserves_artifacts(self):
        for token in (
            "--iso",
            "--image-ref",
            "--update-ref",
            "--artifacts",
            "--timeout-minutes",
            "vm-acceptance.sh",
            "--update-ref",
            "run-metadata.txt",
            "evidence-status.txt",
        ):
            self.assertIn(token, self.script)
        self.assertIn("intentionally leaves its artifact directory", self.script)
        self.assertNotIn("rm -rf", self.script)

    def test_gate_document_separates_source_and_promoted_image_evidence(self):
        self.assertIn("Static owner gate", self.gates)
        self.assertIn("Exact-image gate", self.gates)
        self.assertIn("Observation window", self.gates)
        self.assertIn("disposable disks/images", self.gates)
        self.assertIn("cleanup-vm-acceptance.sh", self.gates)

    def test_just_exposes_reproducible_acceptance_entrypoint(self):
        self.assertIn("rust-migration-acceptance iso image_ref", self.justfile)
        self.assertIn("run-rust-migration-acceptance.sh", self.justfile)

    def test_artifact_workflow_reuses_exact_iso_without_rebuilding(self):
        for token in (
            "artifact_run_id",
            "artifact_name",
            "source_sha",
            "image_ref",
            "actions/download-artifact",
            "run-id:",
            "sha256sum --check --strict",
            "run-rust-migration-acceptance.sh",
            "actions/upload-artifact",
            "IMAGE_REF:",
            "ARTIFACT_RUN_ID:",
            "TIMEOUT_MINUTES:",
            "UPDATE_REF:",
        ):
            self.assertIn(token, self.artifact_workflow)
        self.assertNotIn("podman build", self.artifact_workflow)
        self.assertNotIn("Build ISO with Titanoboa", self.artifact_workflow)


if __name__ == "__main__":
    unittest.main()
