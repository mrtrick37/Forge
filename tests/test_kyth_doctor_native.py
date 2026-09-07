from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]


class DoctorNativeTests(unittest.TestCase):
    def test_doctor_is_declared_as_a_shared_rust_binary(self):
        cargo = (ROOT / "src/kyth-shared-rs/Cargo.toml").read_text(encoding="utf-8")
        self.assertIn('name = "kyth-doctor"', cargo)
        self.assertIn('path = "src/doctor_bin.rs"', cargo)


    def test_doctor_is_built_and_copied_into_the_image(self):
        dockerfile = (ROOT / "Dockerfile").read_text(encoding="utf-8")
        self.assertIn("--bin kyth-doctor", dockerfile)
        self.assertIn("cp /build/kyth-shared-rs/target/release/kyth-doctor /build/kyth-doctor", dockerfile)
        self.assertIn("COPY --from=hub-web-builder --chmod=0755 /build/kyth-doctor /usr/bin/kyth-doctor", dockerfile)


    def test_python_doctor_launcher_is_not_installed_over_the_native_binary(self):
        script = (ROOT / "build_files/scripts/branding/36-misc-utility-installs.sh").read_text(encoding="utf-8")
        self.assertNotIn("install -m 0755 /ctx/kyth-doctor /usr/bin/kyth-doctor", script)
        self.assertIn("kyth-doctor is the native Rust binary", script)
