"""Static contract tests for unattended live/install/update/rollback gating."""
from __future__ import annotations

import pathlib
import json
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[1]
GUEST = ROOT / "build_files" / "kyth-vm-acceptance-guest"
HOST = ROOT / "build_files" / "scripts" / "vm-acceptance.sh"
UNIT = ROOT / "build_files" / "kyth-vm-acceptance.service"
sys.path.insert(0, str(ROOT / "build_files" / "kyth_shared"))

from kyth_shared import vm_acceptance


def _completed(returncode: int = 0, stdout: str = "") -> subprocess.CompletedProcess:
    return subprocess.CompletedProcess(["x"], returncode, stdout=stdout, stderr="")


class VmAcceptanceTests(unittest.TestCase):
    def test_launchers_parse(self):
        subprocess.run(["bash", "-n", str(GUEST)], check=True)
        self.assertIn("/usr/bin/kyth-vm-acceptance-guest", GUEST.read_text(encoding="utf-8"))
        subprocess.run(["bash", "-n", str(HOST)], check=True)

    def test_guest_is_firmware_gated_and_covers_lifecycle(self):
        text = pathlib.Path(vm_acceptance.__file__).read_text(encoding="utf-8")
        self.assertIn("/sys/firmware/qemu_fw_cfg/by_name/opt/com.kyth", text)
        self.assertIn("ExecCondition", UNIT.read_text(encoding="utf-8"))
        for phase in (
            "LIVE_READY", "INSTALL_COMPLETE", "INSTALLED_READY",
            "UPDATE_STAGED", "UPDATE_BOOTED", "ROLLBACK_STAGED",
            "ROLLBACK_BOOTED", "COMPLETE", "FAILED", "HUB_BINARY_OK",
            "HUB_DEEP_LINKS_OK", "HUB_SECOND_LAUNCH_OK",
            "HUB_DASHBOARD_DEGRADED_OK", "HUB_UPDATES_OK",
            "HUB_PRIVILEGED_FAILURE_OK",
        ):
            self.assertIn(phase, text)
        self.assertIn("oci:/usr/share/kyth/image:latest", text)
        self.assertIn("virtio-KYTH_ACCEPT", text)

    def test_live_build_bundles_the_installer_image(self):
        build = (ROOT / "installer" / "build.sh").read_text(encoding="utf-8")
        containerfile = (ROOT / "installer" / "Containerfile").read_text(encoding="utf-8")

        self.assertIn('"oci:/usr/share/kyth/image:latest"', build)
        self.assertIn("skopeo copy --retry-times 3", build)
        self.assertIn('source_imgref="${INSTALL_SOURCE_IMAGE}"', build)
        self.assertIn("containers-storage:*|oci:*|dir:*|ostree:*)", build)
        self.assertIn("KYTH_SOURCE_IMAGE=oci:/usr/share/kyth/image:latest", build)
        self.assertIn("INSTALL_SOURCE_IMAGE", containerfile)

    def test_update_reference_policy(self):
        self.assertTrue(vm_acceptance.valid_update_ref("ghcr.io/example/kyth:testing"))
        self.assertTrue(vm_acceptance.valid_update_ref(""))
        self.assertFalse(vm_acceptance.valid_update_ref("image; poweroff"))

    def test_bootc_status_json_parsing(self):
        completed = subprocess.CompletedProcess(
            ["bootc"], 0,
            stdout='{"status":{"booted":{"image":{"imageDigest":"sha256:abc"}}}}',
            stderr="",
        )
        with mock.patch("kyth_shared.vm_acceptance.run_text", return_value=completed):
            self.assertEqual(vm_acceptance.booted_digest(), "sha256:abc")

    def test_host_uses_a_dedicated_disk_and_collects_evidence(self):
        text = HOST.read_text(encoding="utf-8")
        self.assertIn("qemu-img create", text)
        self.assertIn("serial=KYTH_ACCEPT", text)
        self.assertIn("live-desktop.ppm", text)
        self.assertIn("installed-login.ppm", text)
        self.assertIn("KYTH_ACCEPTANCE:FAILED", text)
        self.assertIn("qualification.json", text)
        self.assertIn("qualification.md", text)

    def test_harness_uses_iso_once_then_defaults_to_installed_disk(self):
        text = HOST.read_text(encoding="utf-8")
        self.assertIn("ide-cd,bus=ahci.0,drive=liveiso,bootindex=2", text)
        self.assertIn("virtio-blk-pci,drive=systemdisk,serial=KYTH_ACCEPT,bootindex=1", text)
        self.assertIn("-boot once=d,menu=off", text)

    def test_installed_hub_acceptance_matrix_is_required(self):
        text = pathlib.Path(vm_acceptance.__file__).read_text(encoding="utf-8")
        for marker in (
            "kyth-hub-shell", "hubRoutes.json", "HUB_DEEP_LINKS_OK",
            "HUB_SECOND_LAUNCH_OK", "HUB_DASHBOARD_DEGRADED_OK",
            "HUB_UPDATES_OK", "HUB_PRIVILEGED_FAILURE_OK", "KYTH_HUB_ACCEPTANCE_FILE",
        ):
            self.assertIn(marker, text)

    def test_installed_hub_acceptance_bootstraps_a_disposable_graphical_user(self):
        text = pathlib.Path(vm_acceptance.__file__).read_text(encoding="utf-8")
        self.assertIn("kyth-acceptance", text)
        self.assertIn("useradd", text)
        self.assertIn("90-kyth-vm-acceptance.conf", text)
        self.assertIn("systemctl", text)

    def test_hub_second_launch_does_not_read_first_launch_evidence(self):
        text = pathlib.Path(vm_acceptance.__file__).read_text(encoding="utf-8")
        first = text.index('if _wait_hub_event(evidence, "deep-link") is None:')
        second = text.index('second = _hub_start', first)
        self.assertIn("evidence.unlink(missing_ok=True)", text[first:second])


class ReadFwCfgTests(unittest.TestCase):
    def test_missing_file_returns_empty_string(self):
        with tempfile.TemporaryDirectory() as tmp:
            missing = pathlib.Path(tmp) / "missing"
            self.assertEqual(vm_acceptance._read_fw_cfg(missing), "")

    def test_strips_nul_bytes_and_whitespace(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "raw"
            path.write_bytes(b"1\x00\x00\n")
            self.assertEqual(vm_acceptance._read_fw_cfg(path), "1")

    def test_enabled_and_read_update_ref_use_fw_cfg_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            enable_file = pathlib.Path(tmp) / "enable"
            update_file = pathlib.Path(tmp) / "update"
            enable_file.write_bytes(b"1")
            update_file.write_bytes(b"ghcr.io/example/kyth:testing")
            with mock.patch("kyth_shared.vm_acceptance.ENABLE_FILE", enable_file), \
                 mock.patch("kyth_shared.vm_acceptance.UPDATE_FILE", update_file):
                self.assertTrue(vm_acceptance.enabled())
                self.assertEqual(vm_acceptance.read_update_ref(), "ghcr.io/example/kyth:testing")

    def test_enabled_is_false_when_file_absent(self):
        with tempfile.TemporaryDirectory() as tmp:
            with mock.patch("kyth_shared.vm_acceptance.ENABLE_FILE", pathlib.Path(tmp) / "absent"):
                self.assertFalse(vm_acceptance.enabled())


class HubAcceptanceHelpersTests(unittest.TestCase):
    def test_active_graphical_session_builds_minimal_environment(self):
        account = mock.Mock(pw_uid=1000, pw_dir="/home/tester")
        with tempfile.TemporaryDirectory() as tmp:
            runtime = pathlib.Path(tmp) / "run-user"
            runtime.mkdir()
            with mock.patch(
                "kyth_shared.vm_acceptance.run_text",
                side_effect=[_completed(0, "c1\n"), _completed(0, "tester\n")],
            ), mock.patch("kyth_shared.vm_acceptance.pwd.getpwnam", return_value=account), mock.patch(
                "kyth_shared.vm_acceptance.Path", side_effect=lambda value: runtime if str(value).startswith("/run/user/") else pathlib.Path(tmp) / "x11"
            ):
                result = vm_acceptance._active_graphical_session()
        self.assertEqual(result[0], "tester")
        self.assertEqual(result[1]["XDG_SESSION_TYPE"], "")

    def test_install_from_live_iso_runs_complete_install_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            log_file = pathlib.Path(tmp) / "acceptance.log"
            target = mock.Mock()
            target.is_block_device.return_value = True
            target.resolve.return_value = target
            with (
                mock.patch("kyth_shared.vm_acceptance.wait_for_desktop", return_value=True),
                mock.patch("kyth_shared.vm_acceptance.run_smoke_check"),
                mock.patch("kyth_shared.vm_acceptance.TARGET_BY_ID", target),
                mock.patch("kyth_shared.vm_acceptance.Path.is_dir", return_value=True),
                mock.patch("kyth_shared.vm_acceptance.LOG_FILE", log_file),
                mock.patch("kyth_shared.vm_acceptance._installer_target_ref", return_value="ghcr.io/example/kyth:testing"),
                mock.patch("kyth_shared.vm_acceptance.read_update_ref", return_value=""),
                mock.patch("kyth_shared.vm_acceptance.run", return_value=_completed(0)),
                mock.patch("kyth_shared.vm_acceptance.power") as power,
                mock.patch("kyth_shared.vm_acceptance.emit") as emit,
            ):
                vm_acceptance.install_from_live_iso()
        power.assert_called_once_with("reboot")
        self.assertTrue(any(call.args[0] == "INSTALL_COMPLETE" for call in emit.call_args_list))

    def test_hub_pages_expands_destinations_and_sections_from_manifest(self):
        manifest = {
            "destinations": [{
                "key": "Move In",
                "route": "/move-in",
                "sections": [{"key": "Network Shares"}],
            }],
        }
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "hubRoutes.json"
            path.write_text(json.dumps(manifest), encoding="utf-8")
            with mock.patch("kyth_shared.vm_acceptance.HUB_ROUTE_MANIFEST", path):
                self.assertEqual(
                    vm_acceptance._hub_pages(),
                    (("Welcome", "/"), ("Move In", "/move-in"), ("Network Shares", "/move-in?section=Network%20Shares")),
                )

    def test_hub_event_requires_json_object_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            evidence = pathlib.Path(tmp) / "hub.log"
            evidence.write_text(
                "KYTH_HUB_ACCEPTANCE:deep-link:{\"page\":\"Updates\",\"route\":\"/updates\"}\n",
                encoding="utf-8",
            )
            self.assertEqual(
                vm_acceptance._hub_event(evidence, "deep-link"),
                {"page": "Updates", "route": "/updates"},
            )
            evidence.write_text("KYTH_HUB_ACCEPTANCE:deep-link:[1]\n", encoding="utf-8")
            self.assertIsNone(vm_acceptance._hub_event(evidence, "deep-link"))

    def test_hub_event_and_wait_handle_missing_invalid_and_delayed_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            evidence = pathlib.Path(tmp) / "missing.log"
            self.assertIsNone(vm_acceptance._hub_event(evidence, "deep-link"))
            evidence.write_text("KYTH_HUB_ACCEPTANCE:deep-link:not-json\n", encoding="utf-8")
            self.assertIsNone(vm_acceptance._hub_event(evidence, "deep-link"))
            evidence.write_text("KYTH_HUB_ACCEPTANCE:deep-link:{\"page\":\"Welcome\"}\n", encoding="utf-8")
            with mock.patch("kyth_shared.vm_acceptance.time.monotonic", side_effect=[0, 0, 1]), \
                 mock.patch("kyth_shared.vm_acceptance.time.sleep"):
                self.assertEqual(vm_acceptance._wait_hub_event(evidence, "deep-link", timeout=0.5), {"page": "Welcome"})

    def test_hub_launch_and_stop_helpers_cover_process_lifecycle(self):
        with tempfile.TemporaryDirectory() as tmp:
            evidence = pathlib.Path(tmp) / "hub.log"
            process = mock.Mock()
            with mock.patch("kyth_shared.vm_acceptance.subprocess.Popen", return_value=process) as popen:
                started = vm_acceptance._hub_start("tester", {"HOME": "/tmp"}, "Updates", evidence, degraded=True)
            self.assertIs(started, process)
            self.assertIn("KYTH_HUB_ACCEPTANCE_DEGRADED=1", popen.call_args.args[0])
            process.poll.return_value = 0
            vm_acceptance._hub_stop(process)
            process.poll.return_value = None
            with mock.patch("kyth_shared.vm_acceptance.os.killpg", side_effect=ProcessLookupError):
                vm_acceptance._hub_stop(process)

    def test_hub_launch_check_handles_start_failure_and_success(self):
        with tempfile.TemporaryDirectory() as tmp:
            evidence = pathlib.Path(tmp) / "hub.log"
            process = mock.Mock()
            event = {"page": "Updates", "route": "/updates", "source": "initial"}
            with mock.patch("kyth_shared.vm_acceptance._hub_start", return_value=process), \
                 mock.patch("kyth_shared.vm_acceptance._wait_hub_event", return_value=event), \
                 mock.patch("kyth_shared.vm_acceptance._hub_stop"):
                self.assertTrue(vm_acceptance._hub_launch_check("tester", {}, "Updates", "/updates", evidence))
            with mock.patch("kyth_shared.vm_acceptance._hub_start", side_effect=OSError("missing")):
                self.assertFalse(vm_acceptance._hub_launch_check("tester", {}, "Updates", "/updates", evidence))

    def test_hub_acceptance_emits_all_qualification_phases(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary = pathlib.Path(tmp) / "kyth-hub-shell"
            binary.write_bytes(b"hub")
            binary.chmod(0o755)
            evidence = pathlib.Path(f"/tmp/kyth-hub-acceptance-{999991}.log")
            evidence.unlink(missing_ok=True)
            starts = [mock.Mock() for _ in range(4)]
            for process in starts:
                process.poll.return_value = 0
            events = [
                {"page": "Welcome", "source": "initial"},
                {"page": "Updates", "source": "single-instance"},
                {"state": "degraded", "label": "Status unavailable"},
                {"state": "degraded"},
                {"state": "expected"},
            ]
            with mock.patch("kyth_shared.vm_acceptance.HUB_BINARY", binary), \
                 mock.patch("kyth_shared.vm_acceptance._active_graphical_session", return_value=("tester", {"HOME": "/tmp", "DISPLAY": ":0"})), \
                 mock.patch("kyth_shared.vm_acceptance._hub_pages", return_value=(("Welcome", "/"),)), \
                 mock.patch("kyth_shared.vm_acceptance._hub_launch_check", return_value=True), \
                 mock.patch("kyth_shared.vm_acceptance._hub_start", side_effect=starts), \
                 mock.patch("kyth_shared.vm_acceptance._wait_hub_event", side_effect=events), \
                 mock.patch("kyth_shared.vm_acceptance._hub_stop"), \
                 mock.patch("kyth_shared.vm_acceptance.emit") as emit:
                vm_acceptance.run_hub_acceptance()
            self.assertEqual(
                [entry.args[0] for entry in emit.call_args_list],
                [
                    "HUB_BINARY_OK", "HUB_DEEP_LINKS_OK", "HUB_SECOND_LAUNCH_OK",
                    "HUB_DASHBOARD_DEGRADED_OK", "HUB_UPDATES_OK", "HUB_PRIVILEGED_FAILURE_OK",
                ],
            )
            evidence.unlink(missing_ok=True)


class EmitAndPowerTests(unittest.TestCase):
    def test_emit_writes_log_and_serial_and_tolerates_write_failures(self):
        with tempfile.TemporaryDirectory() as tmp:
            log_file = pathlib.Path(tmp) / "log"
            serial = pathlib.Path(tmp) / "serial"
            with mock.patch("kyth_shared.vm_acceptance.LOG_FILE", log_file), \
                 mock.patch("kyth_shared.vm_acceptance.SERIAL_DEVICE", serial):
                vm_acceptance.emit("PHASE", "detail\nwith newline")
            self.assertIn("KYTH_ACCEPTANCE:PHASE:detail with newline", log_file.read_text())
            self.assertIn("KYTH_ACCEPTANCE:PHASE:detail with newline", serial.read_text())

    def test_emit_tolerates_unwritable_log_and_serial(self):
        # Directories can't be opened for append/write_text — both writes
        # should raise OSError internally and be swallowed, not propagate.
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = pathlib.Path(tmp)
            with mock.patch("kyth_shared.vm_acceptance.LOG_FILE", tmp_path), \
                 mock.patch("kyth_shared.vm_acceptance.SERIAL_DEVICE", tmp_path):
                vm_acceptance.emit("PHASE", "detail")  # must not raise

    def test_power_runs_systemctl_no_block(self):
        with mock.patch("kyth_shared.vm_acceptance.run") as mock_run:
            vm_acceptance.power("reboot")
            mock_run.assert_called_once_with(["systemctl", "reboot", "--no-block"])

    def test_fail_emits_powers_off_and_exits_one(self):
        with mock.patch("kyth_shared.vm_acceptance.emit") as mock_emit, \
             mock.patch("kyth_shared.vm_acceptance.power") as mock_power:
            with self.assertRaises(SystemExit) as cm:
                vm_acceptance.fail("boom")
            self.assertEqual(cm.exception.code, 1)
            mock_emit.assert_called_once_with("FAILED", "boom")
            mock_power.assert_called_once_with("poweroff")


class WaitForDesktopTests(unittest.TestCase):
    def test_live_mode_checks_plasmashell_and_succeeds_immediately(self):
        with mock.patch("kyth_shared.vm_acceptance.run_text", return_value=_completed(0)) as mock_run_text, \
             mock.patch("kyth_shared.vm_acceptance.time.sleep") as mock_sleep:
            self.assertTrue(vm_acceptance.wait_for_desktop("live", attempts=3, delay=0))
            mock_run_text.assert_called_once_with(["pgrep", "-x", "plasmashell"], timeout=5)
            mock_sleep.assert_not_called()

    def test_installed_mode_checks_display_manager(self):
        with mock.patch("kyth_shared.vm_acceptance.run_text", return_value=_completed(0)) as mock_run_text:
            self.assertTrue(vm_acceptance.wait_for_desktop("installed", attempts=1, delay=0))
            mock_run_text.assert_called_once_with(
                ["systemctl", "is-active", "--quiet", "display-manager.service"], timeout=5
            )

    def test_times_out_when_never_ready(self):
        with mock.patch("kyth_shared.vm_acceptance.run_text", return_value=_completed(1)), \
             mock.patch("kyth_shared.vm_acceptance.time.sleep"):
            self.assertFalse(vm_acceptance.wait_for_desktop("live", attempts=3, delay=0))

    def test_treats_a_missing_result_as_not_ready(self):
        with mock.patch("kyth_shared.vm_acceptance.run_text", return_value=None), \
             mock.patch("kyth_shared.vm_acceptance.time.sleep"):
            self.assertFalse(vm_acceptance.wait_for_desktop("live", attempts=1, delay=0))


class BootedDigestAndDeploymentCountTests(unittest.TestCase):
    def test_booted_digest_falls_back_to_nested_image_digest(self):
        payload = '{"status":{"booted":{"image":{"image":{"imageDigest":"sha256:nested"}}}}}'
        with mock.patch("kyth_shared.vm_acceptance.run_text", return_value=_completed(0, payload)):
            self.assertEqual(vm_acceptance.booted_digest(), "sha256:nested")

    def test_booted_digest_returns_empty_on_nonzero_exit(self):
        with mock.patch("kyth_shared.vm_acceptance.run_text", return_value=_completed(1, "")):
            self.assertEqual(vm_acceptance.booted_digest(), "")

    def test_booted_digest_returns_empty_when_command_unavailable(self):
        with mock.patch("kyth_shared.vm_acceptance.run_text", return_value=None):
            self.assertEqual(vm_acceptance.booted_digest(), "")

    def test_booted_digest_returns_empty_on_malformed_json(self):
        with mock.patch("kyth_shared.vm_acceptance.run_text", return_value=_completed(0, "not json")):
            self.assertEqual(vm_acceptance.booted_digest(), "")

    def test_deployment_count_from_dict_payload(self):
        payload = '{"deployments":[{"a":1},{"b":2}]}'
        with mock.patch("kyth_shared.vm_acceptance.run_text", return_value=_completed(0, payload)):
            self.assertEqual(vm_acceptance.deployment_count(), 2)

    def test_deployment_count_from_list_payload(self):
        payload = '[{"a":1}]'
        with mock.patch("kyth_shared.vm_acceptance.run_text", return_value=_completed(0, payload)):
            self.assertEqual(vm_acceptance.deployment_count(), 1)

    def test_deployment_count_returns_zero_on_failure(self):
        with mock.patch("kyth_shared.vm_acceptance.run_text", return_value=_completed(1, "")):
            self.assertEqual(vm_acceptance.deployment_count(), 0)

    def test_deployment_count_returns_zero_on_malformed_json(self):
        with mock.patch("kyth_shared.vm_acceptance.run_text", return_value=_completed(0, "nope")):
            self.assertEqual(vm_acceptance.deployment_count(), 0)


class InstallerTargetRefAndStateTests(unittest.TestCase):
    def test_installer_target_ref_reads_env_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            env_file = pathlib.Path(tmp) / "kyth-installer.env"
            env_file.write_text("OTHER=1\nKYTH_TARGET_IMAGE='ghcr.io/example/kyth:custom'\n")
            with mock.patch("kyth_shared.vm_acceptance.INSTALLER_ENV_FILE", env_file):
                self.assertEqual(vm_acceptance._installer_target_ref(), "ghcr.io/example/kyth:custom")

    def test_installer_target_ref_defaults_when_file_absent(self):
        with tempfile.TemporaryDirectory() as tmp:
            with mock.patch("kyth_shared.vm_acceptance.INSTALLER_ENV_FILE", pathlib.Path(tmp) / "absent"):
                self.assertEqual(
                    vm_acceptance._installer_target_ref(), "ghcr.io/kyth-os/kyth:testing"
                )

    def test_installer_target_ref_defaults_when_key_missing(self):
        with tempfile.TemporaryDirectory() as tmp:
            env_file = pathlib.Path(tmp) / "kyth-installer.env"
            env_file.write_text("OTHER=1\n")
            with mock.patch("kyth_shared.vm_acceptance.INSTALLER_ENV_FILE", env_file):
                self.assertEqual(
                    vm_acceptance._installer_target_ref(), "ghcr.io/kyth-os/kyth:testing"
                )

    def test_state_value_defaults_to_fresh(self):
        with tempfile.TemporaryDirectory() as tmp:
            with mock.patch("kyth_shared.vm_acceptance.STATE_FILE", pathlib.Path(tmp) / "absent"):
                self.assertEqual(vm_acceptance._state_value(), "fresh")

    def test_state_value_reads_existing_state(self):
        with tempfile.TemporaryDirectory() as tmp:
            state_file = pathlib.Path(tmp) / "state"
            state_file.write_text("update-staged\n")
            with mock.patch("kyth_shared.vm_acceptance.STATE_FILE", state_file):
                self.assertEqual(vm_acceptance._state_value(), "update-staged")

    def test_initial_digest_returns_stored_value(self):
        with tempfile.TemporaryDirectory() as tmp:
            state_dir = pathlib.Path(tmp)
            (state_dir / "initial-digest").write_text("sha256:initial\n")
            with mock.patch("kyth_shared.vm_acceptance.STATE_DIR", state_dir):
                self.assertEqual(vm_acceptance._initial_digest(), "sha256:initial")

    def test_initial_digest_fails_loudly_when_missing(self):
        with tempfile.TemporaryDirectory() as tmp:
            with mock.patch("kyth_shared.vm_acceptance.STATE_DIR", pathlib.Path(tmp)), \
                 mock.patch("kyth_shared.vm_acceptance.power"):
                with self.assertRaises(SystemExit):
                    vm_acceptance._initial_digest()


class RunSmokeCheckTests(unittest.TestCase):
    def test_ok_result_emits_smoke_ok(self):
        with tempfile.TemporaryDirectory() as tmp:
            log_file = pathlib.Path(tmp) / "log"
            serial = pathlib.Path(tmp) / "serial"
            with mock.patch("kyth_shared.vm_acceptance.LOG_FILE", log_file), \
                 mock.patch("kyth_shared.vm_acceptance.SERIAL_DEVICE", serial), \
                 mock.patch("kyth_shared.vm_acceptance.run", return_value=_completed(0)):
                vm_acceptance.run_smoke_check("LIVE")
            self.assertIn("LIVE_SMOKE_OK", log_file.read_text())

    def test_failed_invariants_calls_fail(self):
        with tempfile.TemporaryDirectory() as tmp:
            log_file = pathlib.Path(tmp) / "log"
            serial = pathlib.Path(tmp) / "serial"
            with mock.patch("kyth_shared.vm_acceptance.LOG_FILE", log_file), \
                 mock.patch("kyth_shared.vm_acceptance.SERIAL_DEVICE", serial), \
                 mock.patch("kyth_shared.vm_acceptance.run", return_value=_completed(2)), \
                 mock.patch("kyth_shared.vm_acceptance.power"):
                with self.assertRaises(SystemExit):
                    vm_acceptance.run_smoke_check("LIVE")

    def test_command_that_cannot_run_calls_fail(self):
        with tempfile.TemporaryDirectory() as tmp:
            log_file = pathlib.Path(tmp) / "log"
            serial = pathlib.Path(tmp) / "serial"
            with mock.patch("kyth_shared.vm_acceptance.LOG_FILE", log_file), \
                 mock.patch("kyth_shared.vm_acceptance.SERIAL_DEVICE", serial), \
                 mock.patch("kyth_shared.vm_acceptance.run", side_effect=OSError("no such binary")), \
                 mock.patch("kyth_shared.vm_acceptance.power"):
                with self.assertRaises(SystemExit):
                    vm_acceptance.run_smoke_check("LIVE")

    def test_smoke_check_tolerates_serial_copy_failure(self):
        with tempfile.TemporaryDirectory() as tmp:
            log_file = pathlib.Path(tmp) / "log"
            with mock.patch("kyth_shared.vm_acceptance.LOG_FILE", log_file), \
                 mock.patch("kyth_shared.vm_acceptance.SERIAL_DEVICE", pathlib.Path(tmp)), \
                 mock.patch("kyth_shared.vm_acceptance.run", return_value=_completed(1)), \
                 mock.patch("kyth_shared.vm_acceptance.emit"):
                vm_acceptance.run_smoke_check("LIVE")


class MainEntrypointTests(unittest.TestCase):
    def test_native_read_report_is_packaged_separately_from_run_executor(self):
        cargo = (ROOT / "src/kyth-shared-rs/Cargo.toml").read_text(encoding="utf-8")
        docker = (ROOT / "Dockerfile").read_text(encoding="utf-8")
        branding = (ROOT / "build_files/scripts/branding/35-diagnostic-script-installs.sh").read_text(encoding="utf-8")
        self.assertIn('name = "kyth-vm-acceptance-guest"', cargo)
        rust = (ROOT / "src/kyth-shared-rs/src/system/vm_acceptance.rs").read_text(encoding="utf-8")
        for phase in ("LIVE_READY", "INSTALL_COMPLETE", "INSTALLED_READY", "UPDATE_STAGED", "UPDATE_BOOTED", "ROLLBACK_STAGED", "ROLLBACK_BOOTED", "COMPLETE", "FAILED"):
            self.assertIn(phase, rust)
        self.assertIn("/build/kyth-vm-acceptance-guest /usr/bin/kyth-vm-acceptance-guest", docker)
        self.assertIn("/ctx/kyth-vm-acceptance-guest /usr/libexec/kyth-vm-acceptance-guest", branding)
        self.assertNotIn("ExecCondition=", UNIT.read_text(encoding="utf-8"))
        unit = UNIT.read_text(encoding="utf-8")
        self.assertIn("ExecStart=/usr/libexec/kyth-vm-acceptance-guest run", unit)
        self.assertIn("WantedBy=multi-user.target", unit)
        self.assertIn("MemoryMax=4G", unit)
        installer = (ROOT / "installer/build.sh").read_text(encoding="utf-8")
        self.assertIn("livesys-late.service.d/kyth-vm-acceptance.conf", installer)
        self.assertIn("Wants=kyth-vm-acceptance.service", installer)

    def test_enabled_command_reflects_fw_cfg_state(self):
        with mock.patch("kyth_shared.vm_acceptance.enabled", return_value=True):
            self.assertEqual(vm_acceptance.main(["enabled"]), 0)
        with mock.patch("kyth_shared.vm_acceptance.enabled", return_value=False):
            self.assertEqual(vm_acceptance.main(["enabled"]), 1)

    def test_run_command_is_a_noop_when_not_enabled(self):
        with mock.patch("kyth_shared.vm_acceptance.enabled", return_value=False), \
             mock.patch("kyth_shared.vm_acceptance.install_from_live_iso") as mock_live, \
             mock.patch("kyth_shared.vm_acceptance.run_installed_lifecycle") as mock_installed:
            self.assertEqual(vm_acceptance.main(["run"]), 0)
            mock_live.assert_not_called()
            mock_installed.assert_not_called()

    def test_run_command_dispatches_to_live_iso_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            env_file = pathlib.Path(tmp) / "kyth-installer.env"
            env_file.write_text("x=1")
            with mock.patch("kyth_shared.vm_acceptance.enabled", return_value=True), \
                 mock.patch("kyth_shared.vm_acceptance.INSTALLER_ENV_FILE", env_file), \
                 mock.patch("kyth_shared.vm_acceptance.install_from_live_iso") as mock_live, \
                 mock.patch("kyth_shared.vm_acceptance.run_installed_lifecycle") as mock_installed:
                self.assertEqual(vm_acceptance.main(["run"]), 0)
                mock_live.assert_called_once()
                mock_installed.assert_not_called()

    def test_run_command_dispatches_to_installed_lifecycle(self):
        with tempfile.TemporaryDirectory() as tmp:
            with mock.patch("kyth_shared.vm_acceptance.enabled", return_value=True), \
                 mock.patch("kyth_shared.vm_acceptance.INSTALLER_ENV_FILE", pathlib.Path(tmp) / "absent"), \
                 mock.patch("kyth_shared.vm_acceptance.install_from_live_iso") as mock_live, \
                 mock.patch("kyth_shared.vm_acceptance.run_installed_lifecycle") as mock_installed:
                self.assertEqual(vm_acceptance.main(["run"]), 0)
                mock_installed.assert_called_once()
                mock_live.assert_not_called()

    def test_expected_errors_route_through_fail(self):
        with tempfile.TemporaryDirectory() as tmp:
            with mock.patch("kyth_shared.vm_acceptance.enabled", return_value=True), \
                 mock.patch("kyth_shared.vm_acceptance.INSTALLER_ENV_FILE", pathlib.Path(tmp) / "absent"), \
                 mock.patch(
                     "kyth_shared.vm_acceptance.run_installed_lifecycle",
                     side_effect=RuntimeError("stage failed"),
                 ), \
                 mock.patch("kyth_shared.vm_acceptance.emit") as mock_emit, \
                 mock.patch("kyth_shared.vm_acceptance.power") as mock_power:
                with self.assertRaises(SystemExit) as cm:
                    vm_acceptance.main(["run"])
                self.assertEqual(cm.exception.code, 1)
                mock_emit.assert_called_once_with("FAILED", "stage failed")
                mock_power.assert_called_once_with("poweroff")


class RunInstalledLifecycleTests(unittest.TestCase):
    def _state_dir(self, tmp):
        state_dir = pathlib.Path(tmp) / "state-dir"
        return state_dir

    def test_invalid_update_ref_fails_before_anything_else(self):
        with tempfile.TemporaryDirectory() as tmp:
            state_dir = self._state_dir(tmp)
            with mock.patch("kyth_shared.vm_acceptance.STATE_DIR", state_dir), \
                 mock.patch("kyth_shared.vm_acceptance.STATE_FILE", state_dir / "state"), \
                 mock.patch("kyth_shared.vm_acceptance.read_update_ref", return_value="bad; poweroff"), \
                 mock.patch("kyth_shared.vm_acceptance.power") as mock_power:
                with self.assertRaises(SystemExit):
                    vm_acceptance.run_installed_lifecycle()
                mock_power.assert_called_once_with("poweroff")

    def test_fresh_state_without_update_ref_powers_off_after_install(self):
        with tempfile.TemporaryDirectory() as tmp:
            state_dir = self._state_dir(tmp)
            with mock.patch("kyth_shared.vm_acceptance.STATE_DIR", state_dir), \
                 mock.patch("kyth_shared.vm_acceptance.STATE_FILE", state_dir / "state"), \
                 mock.patch("kyth_shared.vm_acceptance.read_update_ref", return_value=""), \
                 mock.patch("kyth_shared.vm_acceptance.wait_for_desktop", return_value=True), \
                 mock.patch("kyth_shared.vm_acceptance.run_smoke_check") as mock_smoke, \
                 mock.patch("kyth_shared.vm_acceptance.run_hub_acceptance"), \
                 mock.patch("kyth_shared.vm_acceptance.booted_digest", return_value="sha256:aaa"), \
                 mock.patch("kyth_shared.vm_acceptance.power") as mock_power:
                vm_acceptance.run_installed_lifecycle()
                mock_smoke.assert_called_once_with("INSTALLED")
                mock_power.assert_called_once_with("poweroff")
                self.assertEqual(
                    (state_dir / "initial-digest").read_text().strip(), "sha256:aaa"
                )

    def test_fresh_state_with_update_ref_stages_update_and_reboots(self):
        with tempfile.TemporaryDirectory() as tmp:
            state_dir = self._state_dir(tmp)
            with mock.patch("kyth_shared.vm_acceptance.STATE_DIR", state_dir), \
                 mock.patch("kyth_shared.vm_acceptance.STATE_FILE", state_dir / "state"), \
                 mock.patch(
                     "kyth_shared.vm_acceptance.read_update_ref",
                     return_value="ghcr.io/example/kyth:testing",
                 ), \
                 mock.patch("kyth_shared.vm_acceptance.wait_for_desktop", return_value=True), \
                 mock.patch("kyth_shared.vm_acceptance.run_smoke_check"), \
                 mock.patch("kyth_shared.vm_acceptance.run_hub_acceptance"), \
                 mock.patch("kyth_shared.vm_acceptance.booted_digest", return_value="sha256:aaa"), \
                 mock.patch("kyth_shared.vm_acceptance._logged") as mock_logged, \
                 mock.patch("kyth_shared.vm_acceptance.power") as mock_power:
                vm_acceptance.run_installed_lifecycle()
                mock_logged.assert_called_once_with(
                    ["bootc", "switch", "ghcr.io/example/kyth:testing"], "bootc switch failed"
                )
                mock_power.assert_called_once_with("reboot")
                self.assertEqual((state_dir / "state").read_text().strip(), "update-staged")

    def test_fresh_state_fails_when_desktop_never_becomes_ready(self):
        with tempfile.TemporaryDirectory() as tmp:
            state_dir = self._state_dir(tmp)
            with mock.patch("kyth_shared.vm_acceptance.STATE_DIR", state_dir), \
                 mock.patch("kyth_shared.vm_acceptance.STATE_FILE", state_dir / "state"), \
                 mock.patch("kyth_shared.vm_acceptance.read_update_ref", return_value=""), \
                 mock.patch("kyth_shared.vm_acceptance.wait_for_desktop", return_value=False), \
                 mock.patch("kyth_shared.vm_acceptance.power") as mock_power:
                with self.assertRaises(SystemExit):
                    vm_acceptance.run_installed_lifecycle()
                mock_power.assert_called_once_with("poweroff")

    def test_update_staged_success_stages_rollback_and_reboots(self):
        with tempfile.TemporaryDirectory() as tmp:
            state_dir = self._state_dir(tmp)
            state_dir.mkdir(parents=True)
            (state_dir / "state").write_text("update-staged\n")
            (state_dir / "initial-digest").write_text("sha256:initial\n")
            with mock.patch("kyth_shared.vm_acceptance.STATE_DIR", state_dir), \
                 mock.patch("kyth_shared.vm_acceptance.STATE_FILE", state_dir / "state"), \
                 mock.patch("kyth_shared.vm_acceptance.read_update_ref", return_value=""), \
                 mock.patch("kyth_shared.vm_acceptance.wait_for_desktop", return_value=True), \
                 mock.patch("kyth_shared.vm_acceptance.booted_digest", return_value="sha256:updated"), \
                 mock.patch("kyth_shared.vm_acceptance.deployment_count", return_value=2), \
                 mock.patch("kyth_shared.vm_acceptance.run_smoke_check") as mock_smoke, \
                 mock.patch("kyth_shared.vm_acceptance._logged") as mock_logged, \
                 mock.patch("kyth_shared.vm_acceptance.power") as mock_power:
                vm_acceptance.run_installed_lifecycle()
                mock_smoke.assert_called_once_with("UPDATE")
                mock_logged.assert_called_once_with(["bootc", "rollback"], "bootc rollback failed")
                mock_power.assert_called_once_with("reboot")
                self.assertEqual((state_dir / "state").read_text().strip(), "rollback-staged")

    def test_update_staged_fails_when_digest_did_not_change(self):
        with tempfile.TemporaryDirectory() as tmp:
            state_dir = self._state_dir(tmp)
            state_dir.mkdir(parents=True)
            (state_dir / "state").write_text("update-staged\n")
            (state_dir / "initial-digest").write_text("sha256:same\n")
            with mock.patch("kyth_shared.vm_acceptance.STATE_DIR", state_dir), \
                 mock.patch("kyth_shared.vm_acceptance.STATE_FILE", state_dir / "state"), \
                 mock.patch("kyth_shared.vm_acceptance.read_update_ref", return_value=""), \
                 mock.patch("kyth_shared.vm_acceptance.wait_for_desktop", return_value=True), \
                 mock.patch("kyth_shared.vm_acceptance.booted_digest", return_value="sha256:same"), \
                 mock.patch("kyth_shared.vm_acceptance.power") as mock_power:
                with self.assertRaises(SystemExit):
                    vm_acceptance.run_installed_lifecycle()
                mock_power.assert_called_once_with("poweroff")

    def test_update_staged_fails_without_a_rollback_deployment(self):
        with tempfile.TemporaryDirectory() as tmp:
            state_dir = self._state_dir(tmp)
            state_dir.mkdir(parents=True)
            (state_dir / "state").write_text("update-staged\n")
            (state_dir / "initial-digest").write_text("sha256:initial\n")
            with mock.patch("kyth_shared.vm_acceptance.STATE_DIR", state_dir), \
                 mock.patch("kyth_shared.vm_acceptance.STATE_FILE", state_dir / "state"), \
                 mock.patch("kyth_shared.vm_acceptance.read_update_ref", return_value=""), \
                 mock.patch("kyth_shared.vm_acceptance.wait_for_desktop", return_value=True), \
                 mock.patch("kyth_shared.vm_acceptance.booted_digest", return_value="sha256:updated"), \
                 mock.patch("kyth_shared.vm_acceptance.deployment_count", return_value=1), \
                 mock.patch("kyth_shared.vm_acceptance.power") as mock_power:
                with self.assertRaises(SystemExit):
                    vm_acceptance.run_installed_lifecycle()
                mock_power.assert_called_once_with("poweroff")

    def test_rollback_staged_success_completes_and_powers_off(self):
        with tempfile.TemporaryDirectory() as tmp:
            state_dir = self._state_dir(tmp)
            state_dir.mkdir(parents=True)
            (state_dir / "state").write_text("rollback-staged\n")
            (state_dir / "initial-digest").write_text("sha256:initial\n")
            with mock.patch("kyth_shared.vm_acceptance.STATE_DIR", state_dir), \
                 mock.patch("kyth_shared.vm_acceptance.STATE_FILE", state_dir / "state"), \
                 mock.patch("kyth_shared.vm_acceptance.read_update_ref", return_value=""), \
                 mock.patch("kyth_shared.vm_acceptance.wait_for_desktop", return_value=True), \
                 mock.patch("kyth_shared.vm_acceptance.booted_digest", return_value="sha256:initial"), \
                 mock.patch("kyth_shared.vm_acceptance.run_smoke_check") as mock_smoke, \
                 mock.patch("kyth_shared.vm_acceptance.power") as mock_power:
                vm_acceptance.run_installed_lifecycle()
                mock_smoke.assert_called_once_with("ROLLBACK")
                mock_power.assert_called_once_with("poweroff")
                self.assertFalse((state_dir / "state").exists())

    def test_rollback_staged_fails_when_digest_does_not_match_initial(self):
        with tempfile.TemporaryDirectory() as tmp:
            state_dir = self._state_dir(tmp)
            state_dir.mkdir(parents=True)
            (state_dir / "state").write_text("rollback-staged\n")
            (state_dir / "initial-digest").write_text("sha256:initial\n")
            with mock.patch("kyth_shared.vm_acceptance.STATE_DIR", state_dir), \
                 mock.patch("kyth_shared.vm_acceptance.STATE_FILE", state_dir / "state"), \
                 mock.patch("kyth_shared.vm_acceptance.read_update_ref", return_value=""), \
                 mock.patch("kyth_shared.vm_acceptance.wait_for_desktop", return_value=True), \
                 mock.patch("kyth_shared.vm_acceptance.booted_digest", return_value="sha256:not-initial"), \
                 mock.patch("kyth_shared.vm_acceptance.power") as mock_power:
                with self.assertRaises(SystemExit):
                    vm_acceptance.run_installed_lifecycle()
                mock_power.assert_called_once_with("poweroff")

    def test_unknown_state_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            state_dir = self._state_dir(tmp)
            state_dir.mkdir(parents=True)
            (state_dir / "state").write_text("some-unknown-state\n")
            with mock.patch("kyth_shared.vm_acceptance.STATE_DIR", state_dir), \
                 mock.patch("kyth_shared.vm_acceptance.STATE_FILE", state_dir / "state"), \
                 mock.patch("kyth_shared.vm_acceptance.read_update_ref", return_value=""), \
                 mock.patch("kyth_shared.vm_acceptance.wait_for_desktop", return_value=True), \
                 mock.patch("kyth_shared.vm_acceptance.power") as mock_power:
                with self.assertRaises(SystemExit):
                    vm_acceptance.run_installed_lifecycle()
                mock_power.assert_called_once_with("poweroff")


if __name__ == "__main__":
    unittest.main()
