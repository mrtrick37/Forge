"""Sysconfig domain fragments (demonolith build layout)."""
from __future__ import annotations

import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPTS = ROOT / "build_files" / "scripts"
FRAG_DIR = SCRIPTS / "sysconfig"
RUNNER = SCRIPTS / "sysconfig-static.sh"
SYSCTL_DATA = ROOT / "build_files" / "data" / "sysctl.d" / "99-kyth.conf"
BRANDING_FRAG_DIR = SCRIPTS / "branding"
FULL_UPDATE = ROOT / "build_files" / "kyth-full-update"
UPGRADE_SUDOERS = FRAG_DIR / "systemd/35-sudoers-passwordless-safe-upgrade-firmware-operati.sh"


class SysconfigFragmentTests(unittest.TestCase):
    def test_runner_exists_and_is_thin(self):
        self.assertTrue(RUNNER.is_file())
        text = RUNNER.read_text(encoding="utf-8")
        self.assertIn("sysconfig", text)
        self.assertLess(len(text.splitlines()), 50)
        # Must not still embed the old monolith herdocs.
        self.assertNotIn("99-kyth.conf", text)

    def test_fragments_present_and_named(self):
        frags = sorted(FRAG_DIR.rglob("*.sh"))
        self.assertGreaterEqual(len(frags), 20)
        for frag in frags:
            self.assertRegex(frag.name, r"^\d{2}-.+\.sh$")
            body = frag.read_text(encoding="utf-8")
            self.assertTrue(body.startswith("#!/bin/bash") or body.lstrip().startswith("#"))
            self.assertIn("set -euo pipefail", body)

    def test_kernel_sysctl_fragment(self):
        # Sysctl is now consolidated via 00-sysctl-compose (build_files/config/sysctl/*.toml).
        # The old fragment copied 99-kyth.conf verbatim; now it retains only module loads.
        path = FRAG_DIR / "kernel" / "01-kernel-sysctl-parameters.sh"
        compose = ROOT / "build_files" / "scripts" / "sysconfig" / "00-sysctl-compose.sh"
        base_toml = ROOT / "build_files" / "config" / "sysctl" / "base.toml"
        self.assertTrue(path.is_file())
        body = path.read_text(encoding="utf-8")
        self.assertIn("00-sysctl-compose", body)
        self.assertNotIn("cp /ctx/data/sysctl.d/99-kyth.conf", body)
        self.assertTrue(compose.is_file())
        self.assertTrue(base_toml.is_file())
        self.assertIn("vm.swappiness", base_toml.read_text(encoding="utf-8"))

    def test_boot_log_regression_guards(self):
        guards = (FRAG_DIR / "desktop" / "09-autostart-log-noise-guards.sh").read_text(
            encoding="utf-8"
        )
        self.assertIn("groupadd --system plugdev", guards)
        self.assertIn("Before=dbus.socket", guards)
        self.assertIn("RemainAfterExit=yes", guards)
        self.assertIn("systemd-udevd.service", guards)
        self.assertIn("/etc/udev/rules.d/99-input-remapper.rules", guards)
        self.assertIn('TEST=="charge_control_start_threshold"', guards)
        self.assertNotIn('TEST{0002}!="/sys%p/charge_', guards)

    def test_dxvk_defaults_do_not_enable_async(self):
        body = (FRAG_DIR / "gaming" / "47-dxvk-async.sh").read_text(encoding="utf-8")
        self.assertIn("DXVK_CONFIG_FILE=/etc/dxvk.conf", body)
        self.assertIn("dxvk.numCompilerThreads = 0", body)
        self.assertNotIn("dxvk.enableAsync", body)
        self.assertNotIn("DXVK_ASYNC", body)
        self.assertNotRegex(body, r"^DXVK_FRAME_RATE=", re.M)

    def test_journald_cap_has_headroom_and_bounded_file_size(self):
        """A tight cap with no per-file bound left normal growth sitting right at
        the ceiling: systemd-journal-flush.service (stock, unavoidable, early in
        every boot) then had to vacuum old journals back under the cap on every
        boot, stalling all logging for ~30s under full boot I/O contention —
        previously misread as a zram/udev-specific hang. Guard against
        regressing back to a cap with no breathing room.
        """
        body = (FRAG_DIR / "systemd" / "28-journald-size-cap.sh").read_text(encoding="utf-8")
        self.assertIn("SystemMaxFileSize=", body)
        match = re.search(r"SystemMaxUse=(\d+)([MG])", body)
        self.assertIsNotNone(match, "SystemMaxUse must be set with a M/G suffix")
        size_mb = int(match.group(1)) * (1024 if match.group(2) == "G" else 1)
        self.assertGreaterEqual(size_mb, 2048 - 1, "cap regressed back toward zero headroom")

    def test_openrgb_is_not_unconditionally_autostarted(self):
        body = (FRAG_DIR / "peripherals" / "39-openrgb-rgb-peripheral-control.sh").read_text(
            encoding="utf-8"
        )
        self.assertIn("rm -f /etc/skel/.config/autostart/openrgb.desktop", body)
        self.assertNotIn("Exec=openrgb", body)

    def test_initramfs_includes_account_databases(self):
        owner = (ROOT / "build_base" / "plymouth" / "kyth-plymouth-configure").read_text(encoding="utf-8")
        final = (SCRIPTS / "plymouth-initramfs.sh").read_text(encoding="utf-8")
        self.assertIn('install_items+=" /etc/passwd /etc/group "', owner)
        self.assertIn("etc/passwd etc/group", final)

    def test_late_plasma_splash_is_kyth_owned(self):
        fragment = (BRANDING_FRAG_DIR / "12-kyth-session-splash.sh").read_text(
            encoding="utf-8"
        )
        qml = (
            ROOT
            / "build_files"
            / "branding"
            / "plasma-splash"
            / "contents"
            / "splash"
            / "Splash.qml"
        ).read_text(encoding="utf-8")
        polish = (ROOT / "src" / "kyth-shared-rs" / "src" / "user_polish_bin.rs").read_text(
            encoding="utf-8"
        )
        guard = (ROOT / "build_files" / "kyth-session-splash-guard").read_text(
            encoding="utf-8"
        )
        self.assertIn("Theme=org.kythos.desktop", fragment)
        self.assertIn("ExecStartPre=/usr/bin/kyth-session-splash-guard", fragment)
        self.assertIn('source: "images/kyth-logo.svg"', qml)
        self.assertNotIn("fedora", qml.lower())
        self.assertTrue(
            "--key Theme org.kythos.desktop" in polish or
            ("Theme" in polish and "org.kythos.desktop" in polish)
        )
        self.assertIn("--key Theme org.kythos.desktop", guard)

    def test_antigravity_host_wrapper_is_not_built(self):
        self.assertFalse(
            (SCRIPTS / "packages" / "20-google-antigravity-ide.sh").exists()
        )

    def test_bootc_sudoers_uses_only_fixed_guard_operations(self):
        body = (
            FRAG_DIR / "systemd/35-sudoers-passwordless-safe-upgrade-firmware-operati.sh"
        ).read_text(encoding="utf-8")
        for operation in (
            "status",
            "status-json",
            "switch-latest",
            "switch-testing",
            "switch-latest-cachy",
            "switch-testing-cachy",
        ):
            self.assertIn(f"/usr/bin/kyth-bootc-guard {operation}", body)
        self.assertNotIn("NOPASSWD: /usr/bin/bootc", body)
        self.assertIn("NOPASSWD: /usr/libexec/kyth-finalize-staged", body)
        self.assertIn("NOPASSWD: /usr/libexec/kyth-finalize-staged reboot", body)
        self.assertFalse((ROOT / "build_files/kyth-bootc-sudo").exists())
        self.assertFalse((ROOT / "build_files/kyth-sched-sudo").exists())

    def test_bootc_guard_finalizes_after_switch(self):
        guard = (ROOT / "build_files" / "kyth-bootc-guard").read_text(encoding="utf-8")
        self.assertIn("/usr/bin/bootc switch", guard)
        self.assertIn("/usr/libexec/kyth-finalize-staged", guard)
        self.assertNotIn("exec /usr/bin/bootc switch", guard)

    def test_bootc_guard_prepares_boot_before_status(self):
        """bootc status opens sysroot-relative `boot` and returns EPERM on the
        Kyth read-only /boot bind until prepare-boot remounts it.
        """
        guard = (ROOT / "build_files" / "kyth-bootc-guard").read_text(encoding="utf-8")
        self.assertIn("kyth-finalize-staged prepare-boot", guard)
        self.assertIn("opendir(boot)", guard)
        self.assertIn("/usr/bin/rpm-ostree status", guard)
        self.assertNotIn("exec /usr/bin/bootc status", guard)

    def test_upgrade_sudoers_never_grants_podman(self):
        fragment = (
            FRAG_DIR / "systemd/35-sudoers-passwordless-safe-upgrade-firmware-operati.sh"
        ).read_text(encoding="utf-8")
        self.assertNotIn("NOPASSWD: /usr/bin/podman", fragment)

    def test_full_updaters_sudo_calls_have_a_matching_nopasswd_rule(self):
        """kyth-full-update runs everything through `sudo -n`, which fails
        silently (non-interactive, no prompt) the moment its argv drifts
        from the exact NOPASSWD line in the sudoers fragment — this is what
        broke `fwupdmgr refresh --force` when `--force` was added to the
        call but not to the sudoers rule. Every `sudo -n /usr/bin/...`
        invocation must have a byte-exact NOPASSWD counterpart.
        """
        update_body = FULL_UPDATE.read_text(encoding="utf-8")
        sudoers_body = UPGRADE_SUDOERS.read_text(encoding="utf-8")
        commands = re.findall(r"sudo -n (/usr/bin/\S.*)$", update_body, flags=re.MULTILINE)
        self.assertTrue(commands, "expected at least one `sudo -n` call in kyth-full-update")
        for command in commands:
            with self.subTest(command=command):
                self.assertIn(f"NOPASSWD: {command}", sudoers_body)

    def test_passwordless_sudo_rules_do_not_use_argument_globs(self):
        for path in (ROOT / "build_files").rglob("*"):
            if not path.is_file():
                continue
            try:
                lines = path.read_text(encoding="utf-8").splitlines()
            except UnicodeDecodeError:
                continue
            for line in lines:
                if "NOPASSWD:" in line:
                    with self.subTest(path=path.relative_to(ROOT), line=line):
                        self.assertNotIn("*", line)

    def test_firstboot_missing_apps_is_a_successful_status(self):
        cargo = (ROOT / "src" / "kyth-shared-rs" / "Cargo.toml").read_text(
            encoding="utf-8"
        )
        binary = (ROOT / "src" / "kyth-shared-rs" / "src" / "firstboot_app_status_bin.rs")
        self.assertIn('name = "kyth-firstboot-app-status"', cargo)
        self.assertTrue(binary.is_file())
        self.assertIn("write_app_status", binary.read_text(encoding="utf-8"))

    def test_coredump_size_is_capped(self):
        """Nothing else bounds systemd-coredump — a crash-looping game/Proton
        process under gaming.slice can otherwise dump unbounded cores until
        /var fills, cascading into unrelated journald/D-Bus/sddm failures
        that look like random instability rather than a full disk.
        """
        path = FRAG_DIR / "systemd" / "29-coredump-size-cap.sh"
        self.assertTrue(path.is_file())
        body = path.read_text(encoding="utf-8")
        self.assertIn("/etc/systemd/coredump.conf.d/99-kyth.conf", body)
        for expected in ("ProcessSizeMax=", "ExternalSizeMax=", "MaxUse=", "KeepFree="):
            self.assertIn(expected, body)


class ConfigHelperTests(unittest.TestCase):
    HELPER = SCRIPTS / "lib" / "config-helpers.sh"

    def test_helper_defines_write_config_and_write_line(self):
        self.assertTrue(self.HELPER.is_file())
        body = self.HELPER.read_text(encoding="utf-8")
        self.assertIn("write_config()", body)
        self.assertIn("write_line()", body)

    def test_migrated_fragments_source_helper_and_use_it(self):
        # Fragments run in isolated bash subshells, so each must source the
        # helper via a path relative to its own location and then call it.
        migrated = {
            "kernel/13-ntsync.sh": ["write_config", "write_line"],
            "storage/18-i-o-schedulers.sh": ["write_config"],
        }
        for rel, funcs in migrated.items():
            body = (FRAG_DIR / rel).read_text(encoding="utf-8")
            self.assertIn(
                "../../lib/config-helpers.sh", body,
                f"{rel} does not source the shared config helper",
            )
            for func in funcs:
                self.assertIn(f"{func} ", body, f"{rel} does not call {func}")
            # The helper owns parent-dir creation now — migrated fragments
            # should no longer hand-roll a bare `cat >` redirect for their drop.
            self.assertNotIn("cat >/", body, f"{rel} still hand-rolls cat >")


if __name__ == "__main__":
    unittest.main()
