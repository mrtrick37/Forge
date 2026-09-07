import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class PythonPackagingTests(unittest.TestCase):
    def _metadata(self, relative: str) -> dict:
        with (ROOT / relative / "pyproject.toml").open("rb") as stream:
            return tomllib.load(stream)

    def test_runtime_projects_have_standard_metadata(self):
        expected = {
            "build_files/kyth_shared": "kyth-shared",
            "build_files/kyth-installer": "kyth-installer",
            "build_files/kyth-welcome": "kyth-hub-services",
        }

        for relative, project_name in expected.items():
            with self.subTest(project=project_name):
                metadata = self._metadata(relative)
                self.assertEqual(metadata["build-system"]["build-backend"], "setuptools.build_meta")
                self.assertEqual(metadata["project"]["name"], project_name)

    def test_app_packages_publish_console_entry_points(self):
        shared = self._metadata("build_files/kyth_shared")
        installer = self._metadata("build_files/kyth-installer")
        welcome = self._metadata("build_files/kyth-welcome")

        self.assertEqual(
            shared["project"]["scripts"]["kyth-ai-dev"],
            "kyth_shared.ai_dev:main",
        )
        self.assertEqual(
            shared["project"]["scripts"]["kyth-smoke-check"],
            "kyth_shared.smoke_check:main",
        )
        self.assertEqual(
            shared["project"]["scripts"]["kyth-qualify"],
            "kyth_shared.qualification:main",
        )
        self.assertEqual(
            shared["project"]["scripts"]["kyth-boot-health"],
            "kyth_shared.boot_health:main",
        )
        self.assertEqual(
            shared["project"]["scripts"]["kyth-hardware-policy"],
            "kyth_shared.hardware_policy:main",
        )
        self.assertNotIn("kyth-safe-upgrade", shared["project"]["scripts"])
        self.assertEqual(
            shared["project"]["scripts"]["kyth-setup-transfer"],
            "kyth_shared.setup_transfer:main",
        )
        self.assertEqual(
            installer["project"]["scripts"]["kyth-installer"],
            "kyth_installer.app:main",
        )
        self.assertEqual(
            installer["project"]["scripts"]["kyth-partition-install"],
            "kyth_installer.partition_cli:main",
        )
        self.assertNotIn("scripts", welcome["project"])

    def test_image_builds_install_only_the_native_installer_runtime(self):
        dockerfile = (ROOT / "Dockerfile").read_text()
        installer_build = (ROOT / "installer/build.sh").read_text()
        helper_build = (
            ROOT / "build_files/scripts/branding/23-kyth-helper-ctx-installs.sh"
        ).read_text()

        shared_install = dockerfile.index(
            "COPY build_files/kyth_shared /tmp/kyth-shared-package"
        )
        build_time_import = dockerfile.index("bash /ctx/sysconfig-static.sh")
        self.assertLess(shared_install, build_time_import)
        self.assertIn(
            "source=build_files/kyth_shared,target=/ctx/kyth_shared",
            dockerfile,
        )
        self.assertIn("kyth-launch-installer", installer_build)
        self.assertIn("kyth-installerd.service", installer_build)
        self.assertNotIn("python3 -m pip install", installer_build)
        self.assertNotIn("/src/build_files/kyth-installer/", installer_build)
        self.assertNotIn("kyth-installer-package", installer_build)
        self.assertNotIn("python3 -m pip install", helper_build)
        self.assertNotIn("/ctx/kyth-installer", helper_build)
        self.assertNotIn("kyth-installer-package", helper_build)
        self.assertNotIn("chromium", installer_build.lower())
        self.assertNotIn("chromium", helper_build.lower())
        self.assertIn("kyth-hub-desktop-entries", helper_build)
        self.assertNotIn('from kyth_welcome.krunner_desktop import', helper_build)
        self.assertNotIn('"${welcome_package_dir}"', helper_build)
        self.assertNotIn("kyth-partition-install.sh", helper_build)
        self.assertNotIn("/usr/lib/kyth-installer", installer_build)
        self.assertNotIn("/usr/lib/kyth-welcome", helper_build)
        self.assertNotIn("PySide6", (ROOT / "src/kyth-welcome/pyproject.toml").read_text())

    def test_runtime_scripts_do_not_mutate_import_paths(self):
        offenders = []
        runtime_roots = [
            ROOT / "build_files",
            ROOT / "build_files/kyth-installer/kyth_installer",
            ROOT / "build_files/kyth-welcome/kyth_welcome",
        ]
        for runtime_root in runtime_roots:
            for script in runtime_root.rglob("*"):
                if not script.is_file() or any(
                    part.startswith(".") for part in script.relative_to(ROOT).parts
                ):
                    continue
                text = script.read_text(errors="ignore")
                if (
                    "_ensure_kyth_shared_path" in text
                    or "sys.path.insert" in text
                ):
                    offenders.append(str(script.relative_to(ROOT)))

        self.assertEqual(sorted(set(offenders)), [])

    def test_diagnostic_entry_points_are_native_rust_binaries(self):
        dockerfile = (ROOT / "Dockerfile").read_text()
        cargo = (ROOT / "src/kyth-shared-rs/Cargo.toml").read_text()
        entry_points = (
            "kyth-health-check",
            "kyth-resume-check",
            "kyth-nvidia-status",
            "kyth-controller-check",
            "kyth-game-boost",
            "kyth-doctor",
            "kyth-apply-scx-preset",
            "kyth-apply-explorer",
            "kyth-apply-desktop-layout",
            "kyth-apply-display-hdr",
            "kyth-apply-input",
            "kyth-apply-network",
            "kyth-apply-pipewire-latency",
            "kyth-apply-plasma",
            "kyth-apply-quicksettings",
            "kyth-apply-rgb",
            "kyth-apply-role-preset",
            "kyth-apply-scaling",
            "kyth-apply-tailscale",
            "kyth-apply-vrr",
            "kyth-apply-window-snap",
            "kyth-driver-switch",
            "kyth-kali-desktop-fixup",
            "kyth-ntfs-repair",
            "kyth-performance-mode",
            "kyth-refresh-taskbar-pins",
            "kyth-report-issue",
            "kyth-session-snapshot",
            "kyth-setup-devcontainer",
            "kyth-setup-transfer",
            "kyth-vscode-wallet",
            "kyth-web-app-categorize",
            "kyth-storage-sense",
            "kyth-duperemove",
            "kyth-batteryd",
            "kyth-cloud-mount",
            "kyth-save-sync",
            "kyth-backup",
            "kyth-game-launch",
            "kyth-dynamic-lock",
            "kyth-proton-cachyos-update",
            "kyth-rclone-update",
        )
        for entry_point in entry_points:
            with self.subTest(entry_point=entry_point):
                self.assertIn(f'name = "{entry_point}"', cargo)
                self.assertIn(f"/build/{entry_point} /usr/bin/{entry_point}", dockerfile)
                self.assertFalse((ROOT / "build_files" / entry_point).exists())

        # libexec-installed native launcher (systemd unit ExecStart): same
        # contract, different install prefix.
        with self.subTest(entry_point="kyth-refresh-boot-splash-initramfs"):
            self.assertIn('name = "kyth-refresh-boot-splash-initramfs"', cargo)
            self.assertIn(
                "/build/kyth-refresh-boot-splash-initramfs /usr/libexec/kyth-refresh-boot-splash-initramfs",
                dockerfile,
            )
            self.assertFalse((ROOT / "build_files" / "kyth-refresh-boot-splash-initramfs").exists())

        helper_copy = next(
            line
            for line in dockerfile.splitlines()
            if line.startswith("COPY build_files/game-performance ")
        )
        for entry_point in entry_points:
            with self.subTest(host_copy_entry_point=entry_point):
                self.assertNotIn(f"build_files/{entry_point}", helper_copy)

    def test_tunable_dispatcher_uses_native_subset_before_static_sysconfig(self):
        dockerfile = (ROOT / "Dockerfile").read_text()
        cargo = (ROOT / "src/kyth-shared-rs/Cargo.toml").read_text()
        dispatcher = (
            ROOT / "build_files/scripts/sysconfig/tunable/01-tunable-dispatcher.sh"
        ).read_text()

        self.assertIn('name = "kyth-tunable-rs"', cargo)
        native_copy = dockerfile.index(
            "COPY --from=hub-web-builder --chmod=0755 /build/kyth-tunable-rs /usr/bin/kyth-tunable-rs"
        )
        static_layer = dockerfile.index("bash /ctx/sysconfig-static.sh")
        self.assertLess(native_copy, static_layer)
        self.assertEqual(
            dockerfile.count(
                "COPY --from=hub-web-builder --chmod=0755 /build/kyth-tunable-rs /usr/bin/kyth-tunable-rs"
            ),
            1,
        )
        self.assertIn("--list-native", dispatcher)
        self.assertNotIn("python3", dispatcher)
        self.assertIn('ln -sf kyth-tunable-rs "/usr/bin/kyth-${t}"', dispatcher)
        self.assertIn('ln -sf kyth-tunable "/usr/bin/kyth-${t}"', dispatcher)
        self.assertLess(
            dockerfile.index("bash /ctx/scripts/branding.sh"),
            dockerfile.index("bash /ctx/scripts/sysconfig/tunable/01-tunable-dispatcher.sh"),
        )

    def test_shared_modules_use_the_command_runner(self):
        shared_root = ROOT / "build_files/kyth_shared/kyth_shared"
        direct_callers = []
        for module in shared_root.rglob("*.py"):
            if "subprocess.run(" in module.read_text():
                direct_callers.append(str(module.relative_to(shared_root)))

        self.assertEqual(direct_callers, ["accounts.py"])

    def test_welcome_bounded_commands_use_service_adapter(self):
        welcome_root = ROOT / "build_files/kyth-welcome/kyth_welcome"
        direct_callers = []
        for module in welcome_root.rglob("*.py"):
            text = module.read_text()
            if "subprocess.run(" in text or "subprocess.check_output(" in text:
                direct_callers.append(str(module.relative_to(welcome_root)))

        self.assertEqual(direct_callers, [])


if __name__ == "__main__":
    unittest.main()
