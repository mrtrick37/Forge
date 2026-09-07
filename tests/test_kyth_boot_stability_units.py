"""Contracts for boot-path timeouts, MOK retry, and /boot mutator caps."""
from __future__ import annotations

import pathlib
import re
import subprocess
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "build_files" / "kyth_shared"))
sys.path.insert(0, str(ROOT / "build_files" / "kyth-installer"))
SELINUX_UNIT = (
    ROOT / "build_files/scripts/sysconfig/systemd"
    / "32-selinux-relabel-var-home-after-each-new-deployment.sh"
)
BOOT_SPLASH = ROOT / "build_files/scripts/branding/28-bootc-kernel-arguments-and-boot-splash.sh"
ENROLL_SCRIPT = ROOT / "build_files/tests/secureboot-enrollment.sh"


class BootStabilityUnitTests(unittest.TestCase):
    def test_selinux_home_relabel_is_capped_and_still_before_greeter(self) -> None:
        """The login-critical relabel must stay bounded and gate the greeter;
        the exhaustive full-tree pass must run separately, in the background,
        and must never be able to delay login. Before 27ca9887 these were one
        oneshot with a single 300s cap around a full `restorecon -RF`, which
        scales with home directory size — on a large home it can run past the
        cap, the completion stamp is never written, and every subsequent boot
        repeats the same doomed relabel instead of it costing once.
        """
        body = SELINUX_UNIT.read_text(encoding="utf-8")
        fast_unit = body.split("RELABELEOF", 1)[1].split("RELABELEOF", 1)[0]
        full_unit = body.split("RELABELFULLEOF", 1)[1].split("RELABELFULLEOF", 1)[0]
        # StartLimitIntervalSec/StartLimitBurst are only recognized in
        # [Unit] — systemd silently logs "Unknown key ... in section
        # [Service], ignoring" and drops both if they land in [Service].
        # Split each unit at its own [Service] header so the assertions
        # below actually pin *which* section a key lives in, instead of
        # matching anywhere in the file.
        # Split on the section *header line*, not a bare substring match —
        # both units' comments quote systemd's own "in section [Service]"
        # log message, which would otherwise split the string early.
        fast_unit_sec, fast_service_sec = fast_unit.split("\n[Service]\n", 1)
        full_unit_sec, full_service_sec = full_unit.split("\n[Service]\n", 1)

        self.assertIn("Before=plasmalogin.service display-manager.service", fast_unit)
        self.assertIn("TimeoutStartSec=60", fast_service_sec)
        self.assertIn("StartLimitIntervalSec=300", fast_unit_sec)
        self.assertIn("StartLimitBurst=5", fast_unit_sec)
        self.assertNotIn("StartLimit", fast_service_sec)
        self.assertNotIn("restorecon -RF -T0 /var/home", body.split("RELABELFULLEOF")[0])

        self.assertNotIn("Before=plasmalogin", full_unit)
        self.assertNotIn("Before=display-manager", full_unit)
        self.assertIn("Conflicts=shutdown.target", full_unit_sec)
        self.assertIn("IOSchedulingClass=idle", full_service_sec)
        self.assertIn("TimeoutStartSec=3600", full_service_sec)
        self.assertIn("StartLimitIntervalSec=3600", full_unit_sec)
        self.assertIn("StartLimitBurst=3", full_unit_sec)
        self.assertNotIn("StartLimit", full_service_sec)
        self.assertIn("kyth-selinux-relabel-home-full", body)

        fast_script = (
            SELINUX_UNIT.parents[1] / "kyth-selinux-relabel-home"
        ).read_text(encoding="utf-8")
        full_script = (
            SELINUX_UNIT.parents[1] / "kyth-selinux-relabel-home-full"
        ).read_text(encoding="utf-8")
        # The login-critical script must never recurse into a user's bulk
        # data — that would reintroduce the same size-dependent stall.
        self.assertNotIn("restorecon -RF", fast_script)
        self.assertNotIn("restorecon -RF -T0 /var/home", fast_script)
        self.assertIn("restorecon -RF -T0 /var/home", full_script)
        self.assertIn("selinux-relabel-home-full.stamp", full_script)
        self.assertIn("selinux-relabel-home.stamp", fast_script)
        # Both scripts run under set -euo pipefail; a failing `ostree admin
        # status` (seen in the field as a silent status=1/FAILURE with no
        # script output at all) must fall through to the /proc/cmdline and
        # stat(1) fallbacks below it, not abort the script outright.
        for script in (fast_script, full_script):
            deployment_block = script.split("deployment_id=\"\"", 1)[1].split(
                "if [ -z \"$deployment_id\" ] && [ -r /proc/cmdline ]", 1
            )[0]
            self.assertIn("awk '/^\\* /{print $2\" \"$3; exit}')\" || true", deployment_block)
        # The top-level restorecon must not be able to hard-fail the whole
        # script the way the per-home loop below it is already guarded.
        self.assertNotIn("\n/sbin/restorecon -F /var/home\n", fast_script)

    def test_boot_mutators_have_timeouts_and_path_trigger_limit(self) -> None:
        body = BOOT_SPLASH.read_text(encoding="utf-8")
        self.assertIn("kyth-boot-splash-kargs.service", body)
        self.assertIn("kyth-boot-branding.service", body)
        self.assertIn("kyth-boot-splash-initramfs.service", body)
        self.assertGreaterEqual(body.count("TimeoutStartSec=60"), 2)
        self.assertIn("TimeoutStartSec=300", body)
        self.assertIn("TriggerLimitIntervalSec=10", body)
        self.assertIn("TriggerLimitBurst=5", body)

    def test_secureboot_enrollment_does_not_stamp_flag_on_import_failure(self) -> None:
        result = subprocess.run(
            ["bash", str(ENROLL_SCRIPT)],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        self.assertIn("secureboot enrollment tests passed", result.stdout)

    def test_sched_and_telem_install_as_user_units(self) -> None:
        body = (ROOT / "build_files/scripts/branding/27-performance-daemons.sh").read_text(
            encoding="utf-8"
        )
        self.assertIn("/usr/lib/systemd/user/kyth-sched.service", body)
        self.assertIn("/usr/lib/systemd/user/kyth-telem.service", body)
        self.assertNotIn("/usr/lib/systemd/system/kyth-sched.service", body)
        self.assertNotIn("/usr/lib/systemd/system/kyth-telem.service", body)

    def test_restart_limited_units_cap_start_burst(self) -> None:
        units = (
            ROOT / "build_files/kyth-batteryd.service",
            ROOT / "build_files/rclone@.service",
            ROOT / "build_files/kyth-telem.service",
        )
        for path in units:
            body = path.read_text(encoding="utf-8")
            with self.subTest(unit=path.name):
                self.assertIn("StartLimitIntervalSec=60", body)
                self.assertIn("StartLimitBurst=3", body)
                self.assertIn("RestartSec=", body)
        zram = (ROOT / "build_files/scripts/branding/51-zram.sh").read_text(encoding="utf-8")
        self.assertIn("StartLimitIntervalSec=60", zram)
        self.assertIn("StartLimitBurst=3", zram)
        # oneshot + RemainAfterExit cannot use Restart=; keep a start-limit
        # so a crash loop still cannot take the boot.
        arbiter = (ROOT / "build_files/kyth-sched-arbiter.service").read_text(
            encoding="utf-8"
        )
        self.assertIn("StartLimitIntervalSec=60", arbiter)
        self.assertIn("StartLimitBurst=3", arbiter)
        self.assertNotRegex(arbiter, r"^Restart=", re.M)
        generated = (
            ROOT / "build_files/scripts/sysconfig/gaming/15-sched-arbiter.sh"
        ).read_text(encoding="utf-8")
        self.assertIn("After=local-fs.target", generated)
        self.assertNotIn("Restart=on-failure", generated)
        self.assertNotRegex(generated, r"^After=multi-user\.target$", re.M)

    def test_zram_setup_does_not_wait_for_udev_device(self) -> None:
        """After switch-root, udevd is down until sysinit; sysinit After=swap.
        Waiting for dev-zram0.device is a 30s timeout every boot.
        """
        zram = (ROOT / "build_files/scripts/branding/51-zram.sh").read_text(encoding="utf-8")
        ntsync = (
            ROOT / "build_files/scripts/sysconfig/kernel/13-ntsync.sh"
        ).read_text(encoding="utf-8")
        self.assertIn("kyth-zram-swap.service", zram)
        self.assertIn("mknod -m 0600 /dev/zram0", zram)
        self.assertIn("After=systemd-modules-load.service", zram)
        self.assertIn("Before=swap.target", zram)
        self.assertNotIn("After=systemd-udevd.service", zram)
        self.assertNotIn("After=dev-zram0.device", zram)
        self.assertNotIn("Requires=dev-zram0.device", zram)
        self.assertIn("system-generators/zram-generator", zram)
        self.assertIn("systemctl mask", zram)
        self.assertIn("dev-zram0.device", zram)
        self.assertIn("dev-zram0.swap", zram)
        self.assertIn("systemd-zram-setup@zram0.service", zram)
        self.assertNotIn("JobTimeoutSec=30", ntsync)
        self.assertNotIn("dev-zram0.device.d", ntsync)

    def test_zram_swap_sources_a_plain_contract_instead_of_parsing_generator_syntax(
        self,
    ) -> None:
        """kyth-zram-swap must not re-derive memory_tune's formula by
        pattern-matching zram-generator.conf's math-expression grammar — a
        format this project owns the writer of but that script doesn't parse.
        A new shape memory_tune emits (that the old awk `case` didn't
        enumerate) would silently fall back to the wrong tier instead of
        erroring. It should source memory_tune's plain key=value sidecar
        file instead.
        """
        zram = (ROOT / "build_files/scripts/branding/51-zram.sh").read_text(encoding="utf-8")
        self.assertIn("/etc/kyth/zram-runtime.env", zram)
        self.assertIn("KYTH_ZRAM_PERCENT", zram)
        self.assertIn("KYTH_ZRAM_CAP_MB", zram)
        self.assertIn("KYTH_ZRAM_ALGO", zram)
        # The old awk one-liners only recognized a fixed set of formula
        # shapes memory_tune happened to emit at the time they were written.
        self.assertNotIn("awk -F=", zram)
        self.assertNotIn("min(ram*0.5,8192)", zram)

    def test_memory_tune_applies_only_its_own_sysctl_file(self) -> None:
        body = (ROOT / "build_files/scripts/sysconfig/kernel/56-memory-tune.sh").read_text(
            encoding="utf-8"
        )
        self.assertIn("sysctl --load=/etc/sysctl.d/99-kyth-memory.conf", body)
        self.assertIn("ExecStartPost=-/usr/bin/sysctl --load=/etc/sysctl.d/99-kyth-memory.conf", body)
        self.assertNotIn("ExecStartPost=/usr/bin/sysctl --system", body)
        self.assertNotIn("sudo sysctl --system", body)
        self.assertIn("After=local-fs.target systemd-sysctl.service", body)
        self.assertNotRegex(body, r"^After=multi-user\.target$", re.M)

    def test_irqbalance_oneshot_does_not_fail_type_simple(self) -> None:
        body = (ROOT / "build_files/scripts/sysconfig/systemd/05-irqbalance-tuning.sh").read_text(
            encoding="utf-8"
        )
        late = (ROOT / "build_files/scripts/sysconfig/kernel/48-irqbalance-tuning.sh").read_text(
            encoding="utf-8"
        )
        self.assertIn("IRQBALANCE_ONESHOT=yes", body)
        self.assertIn("irqbalance.service.d/10-kyth-oneshot.conf", body)
        self.assertIn("Type=oneshot", body)
        self.assertIn("RemainAfterExit=yes", body)
        self.assertIn("--deepestcache=2", body)
        self.assertNotIn("write_config /etc/sysconfig/irqbalance", late)

    def test_dbus_runtime_dir_stays_active_after_mkdir(self) -> None:
        body = (
            ROOT / "build_files/scripts/sysconfig/desktop/09-autostart-log-noise-guards.sh"
        ).read_text(encoding="utf-8")
        dbus_unit = body.split("kyth-dbus-runtime-dir.service", 1)[1]
        self.assertIn("RemainAfterExit=yes", dbus_unit.split("DBUSRUNDIREOF", 1)[0])

    def test_local_bin_migrate_can_write_state_and_homes(self) -> None:
        body = (ROOT / "build_files/kyth-local-bin-migrate.service").read_text(
            encoding="utf-8"
        )
        self.assertIn("RemainAfterExit=yes", body)
        self.assertIn("StateDirectory=kyth/migrations", body)
        self.assertIn("ReadWritePaths=-/usr/local/bin -/var/home -/root", body)
        self.assertNotIn("PrivateUsers=yes", body)

    def test_flathub_setup_skips_offline_and_can_write_flatpak_state(self) -> None:
        body = (ROOT / "build_files/kyth-flathub-setup.service").read_text(
            encoding="utf-8"
        )
        self.assertIn("Wants=network-online.target", body)
        self.assertNotIn("Requires=network-online.target", body)
        self.assertIn("ExecCondition=", body)
        self.assertIn("ReadWritePaths=-/var/lib/flatpak -/var/cache/flatpak", body)
        self.assertNotIn("PrivateUsers=yes", body)
        self.assertIn("exit 0", body.split("ExecStart=", 1)[1])

    def test_default_flatpaks_do_not_fail_when_flathub_is_absent(self) -> None:
        body = (ROOT / "build_files/kyth-default-flatpaks.service").read_text(
            encoding="utf-8"
        )
        self.assertIn("Wants=network-online.target kyth-flathub-setup.service", body)
        self.assertNotIn("Requires=network-online.target", body)
        self.assertIn("ExecCondition=", body)
        self.assertIn("grep -qx flathub", body)
        self.assertIn("will retry next boot", body)
        self.assertNotIn("ExecStartPost=/bin/touch", body)

    def test_qemu_guest_agent_is_vm_only(self) -> None:
        body = (
            ROOT / "build_files/scripts/packages/13-gpu-amd-and-qemu-guest.sh"
        ).read_text(encoding="utf-8")
        self.assertIn("qemu-guest-agent.service.d", body)
        self.assertIn("ConditionVirtualization=vm", body)

    def test_boot_rw_uses_prepare_boot_and_cannot_fail_sysinit(self) -> None:
        body = (
            ROOT / "build_files/scripts/sysconfig/systemd/kyth-boot-rw.service"
        ).read_text(encoding="utf-8")
        self.assertIn("ExecStart=-/usr/libexec/kyth-finalize-staged prepare-boot", body)
        self.assertNotIn("mount -o remount,bind,rw /boot", body)

    def test_splash_and_branding_wait_for_writable_boot(self) -> None:
        body = BOOT_SPLASH.read_text(encoding="utf-8")
        self.assertGreaterEqual(body.count("After=local-fs.target kyth-boot-rw.service"), 2)
        self.assertIn("grubby --update-kernel=ALL --remove-args=", body)
        self.assertIn("|| true", body)
        self.assertNotIn("kyth-firstboot-notice.service", body)
        self.assertNotIn("first-boot-done", body)

    def test_first_boot_plymouth_message_stamps_before_plymouth(self) -> None:
        body = (
            ROOT / "build_files/scripts/sysconfig/systemd/33-first-boot-plymouth-message.sh"
        ).read_text(encoding="utf-8")
        unit = body.split("FIRSTBOOTEOF", 1)[1].split("FIRSTBOOTEOF", 1)[0]
        self.assertIn("touch /var/lib/kyth/.first-boot-complete", unit)
        self.assertIn("ExecStart=-/usr/bin/plymouth", unit)
        self.assertNotIn("ExecCondition=/usr/bin/plymouth --ping", unit)
        self.assertIn("open Kyth Hub", unit)
        self.assertLess(
            unit.find("touch /var/lib/kyth/.first-boot-complete"),
            unit.find("plymouth message"),
        )

    def test_wait_online_offline_exit_is_success(self) -> None:
        body = (ROOT / "build_files/scripts/branding/31-ujust-recipes.sh").read_text(
            encoding="utf-8"
        )
        self.assertIn("NetworkManager-wait-online.service.d", body)
        self.assertIn("SuccessExitStatus=1", body)

    def test_storage_maint_is_timer_only(self) -> None:
        body = (ROOT / "build_files/kyth-storage-maint.service").read_text(
            encoding="utf-8"
        )
        self.assertNotIn("[Install]", body)
        self.assertNotRegex(body, r"^WantedBy=", re.M)

    def test_boot_timing_and_batteryd_avoid_multiuser_cycle(self) -> None:
        timing = (ROOT / "build_files/scripts/branding/51-zram.sh").read_text(
            encoding="utf-8"
        )
        battery = (ROOT / "build_files/kyth-batteryd.service").read_text(encoding="utf-8")
        power = (ROOT / "build_files/kyth-power-arbiter.service").read_text(
            encoding="utf-8"
        )
        self.assertIn("After=local-fs.target", timing)
        self.assertNotRegex(timing, r"^After=multi-user\.target$", re.M)
        self.assertIn("After=local-fs.target", battery)
        self.assertNotIn("After=multi-user.target", battery)
        self.assertIn("After=local-fs.target", power)
        self.assertNotIn("After=multi-user.target", power)

    def test_branding_guard_prefers_bind_rw(self) -> None:
        guard = (ROOT / "build_files/kyth-boot-branding-guard").read_text(encoding="utf-8")
        ply = (ROOT / "src/kyth_shared/kyth_shared/plymouth.py").read_text(
            encoding="utf-8"
        )
        self.assertIn("remount,bind,rw /boot", guard)
        self.assertIn('["mount", "-o", "remount,bind,rw", "/boot"]', ply)
        repair = (
            ROOT / "build_files/scripts/repair-current-plymouth-initramfs.sh"
        ).read_text(encoding="utf-8")
        self.assertIn("remount,bind,rw /boot", repair)

    def test_greenboot_waits_for_selinux_relabel_and_stays_active(self) -> None:
        from kyth_shared.system.boot_runtime import DEFAULT_DEADLINE

        body = (
            ROOT / "build_files/scripts/branding/35-diagnostic-script-installs.sh"
        ).read_text(encoding="utf-8")
        self.assertIn("After=kyth-selinux-relabel-home.service", body)
        self.assertIn("TimeoutStartSec=600", body)
        self.assertIn("RemainAfterExit=yes", body)
        self.assertGreaterEqual(DEFAULT_DEADLINE, 300.0)

    def test_probe_oneshot_stays_active_for_timer(self) -> None:
        body = (ROOT / "build_files/kyth-probe.service").read_text(encoding="utf-8")
        unit = body.split("[Service]", 1)[0]
        self.assertIn("RemainAfterExit=yes", body)
        self.assertIn("StartLimitIntervalSec=120", unit)
        self.assertIn("StartLimitBurst=5", unit)

    def test_power_arbiter_can_retrigger_without_start_limit(self) -> None:
        body = (ROOT / "build_files/kyth-power-arbiter.service").read_text(
            encoding="utf-8"
        )
        self.assertIn("RemainAfterExit=no", body)
        self.assertIn("StartLimitBurst=20", body)
        self.assertNotIn("PrivateUsers=yes", body)

    def test_scx_loader_skips_when_unconfigured(self) -> None:
        unit = (ROOT / "build_files/kyth-scx-loader.service").read_text(encoding="utf-8")
        script = (ROOT / "build_files/kyth-scx-loader").read_text(encoding="utf-8")
        self.assertIn("ConditionPathExists=/etc/scx/scx_loader.conf", unit)
        self.assertIn("leaving sched_ext unset", script)
        self.assertNotIn("exit 1", script.split("missing", 1)[1].split("scheduler", 1)[0])

    def test_splash_initramfs_cannot_fail_the_boot_unit_list(self) -> None:
        body = BOOT_SPLASH.read_text(encoding="utf-8")
        self.assertIn("ExecStart=-/usr/libexec/kyth-refresh-boot-splash-initramfs", body)


class InstallerMokFailClosedTests(unittest.TestCase):
    def test_failed_mok_staging_blocks_install_success(self) -> None:
        from kyth_installer.phases.run import _require_secure_boot_ready

        with self.assertRaisesRegex(RuntimeError, "could not stage MOK"):
            _require_secure_boot_ready("failed")

    def test_successful_mok_states_do_not_block_install(self) -> None:
        from kyth_installer.phases.run import _require_secure_boot_ready

        for state in ("skipped", "enrolled", "pending", "staged", {}):
            with self.subTest(state=state):
                _require_secure_boot_ready(state)


if __name__ == "__main__":
    unittest.main()
