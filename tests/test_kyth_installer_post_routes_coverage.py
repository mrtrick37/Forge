import json
import sys
import tempfile
import unittest
from pathlib import Path, PosixPath
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "build_files" / "kyth-installer"))

from kyth_installer.context import InstallerContext  # noqa: E402
from kyth_installer.post_routes import PostRouteService  # noqa: E402


class PostRouteCoverageTests(unittest.TestCase):
    def setUp(self):
        self.context = InstallerContext()
        self.routes = PostRouteService(self.context)

    def test_dispatch_rejects_unknown_and_locked_partition_routes(self):
        self.assertEqual(self.routes.dispatch("missing", {}).status, 404)
        self.context.install_lock.acquire()
        try:
            response = self.routes.dispatch("new_table", {})
        finally:
            self.context.install_lock.release()
        self.assertEqual(response.status, 409)

    def test_route_status_translation(self):
        cases = (
            ("new_table", "new_table", {"ok": True}, 200),
            ("new_table", "new_table", {"ok": False}, 400),
            ("commit_partitions", "commit_partitions", {"ok": False, "errors": []}, 400),
            ("commit_partitions", "commit_partitions", {"ok": False}, 500),
            ("rollback_partitions", "rollback_partitions", {"ok": False}, 500),
            ("cancel", "cancel_install", {"ok": False}, 409),
            ("reboot", "reboot", {"ok": False}, 500),
        )
        for route, method, result, status in cases:
            with self.subTest(route=route, result=result):
                with mock.patch.object(self.routes.installer_service, method, return_value=result):
                    self.assertEqual(self.routes.dispatch(route, {}).status, status)

    def test_start_maps_success_conflict_and_validation_failure(self):
        for result, status in (
            ({"started": True}, 200),
            ({"started": False, "message": "An installation is already running."}, 409),
            ({"started": False, "message": "bad request"}, 400),
        ):
            with mock.patch.object(
                self.routes.installer_service, "start_install", return_value=result
            ):
                self.assertEqual(self.routes.start({}).status, status)

    def test_rescue_logs_copies_only_safe_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            mount = Path(tmp) / "usb"
            mount.mkdir()
            log_file = Path(tmp) / "install.log"
            log_file.write_text("log")
            missing = Path(tmp) / "missing"
            run = mock.Mock()
            with (
                mock.patch("kyth_installer.config.LOG_FILE", log_file),
                mock.patch("kyth_installer.config.TRANSACTION_FILE", missing),
                mock.patch("kyth_installer.config.FAILURE_SUMMARY_FILE", missing),
                mock.patch("kyth_installer.runner.run_command", run),
                mock.patch("kyth_installer.system._as_root", side_effect=lambda argv: argv),
            ):
                response = self.routes.rescue_logs_to_usb({"usb_mount": str(mount)})
        self.assertEqual(response.status, 200)
        self.assertEqual(response.payload["copied"], ["install.log"])

    def test_reboot_uses_native_operation_when_helper_is_installed(self):
        with (
            mock.patch("kyth_installer.post_routes.shutil.which", return_value="/usr/bin/kyth-installer-exec"),
            mock.patch("kyth_installer.orchestration.native_operation", return_value=None) as native,
        ):
            response = self.routes.reboot({})
        self.assertEqual(response.status, 200)
        self.assertEqual(response.payload, {"ok": True})
        native.assert_called_once_with("reboot", {})

    def test_reboot_native_failure_maps_to_500(self):
        with (
            mock.patch("kyth_installer.post_routes.shutil.which", return_value="/usr/bin/kyth-installer-exec"),
            mock.patch(
                "kyth_installer.orchestration.native_operation",
                side_effect=RuntimeError("native reboot failed"),
            ),
        ):
            response = self.routes.reboot({})
        self.assertEqual(response.status, 500)
        self.assertIn("native reboot failed", response.payload["error"])

    def test_rescue_logs_uses_native_recovery_export_when_helper_is_installed(self):
        with tempfile.TemporaryDirectory() as tmp:
            mount = Path(tmp) / "usb"
            mount.mkdir()
            native_response = mock.Mock(
                stdout=json.dumps({
                    "ok": True,
                    "dest": f"{mount}/kyth-installer-logs",
                    "copied": ["log"],
                })
            )
            with (
                mock.patch("kyth_installer.post_routes.shutil.which", return_value="/usr/bin/kyth-installer-exec"),
                mock.patch("kyth_installer.runner.run_command", return_value=native_response) as run,
                mock.patch("kyth_installer.system._as_root", side_effect=lambda argv: argv),
            ):
                response = self.routes.rescue_logs_to_usb({"usb_mount": str(mount)})

        self.assertEqual(response.status, 200)
        self.assertEqual(response.payload["copied"], ["log"])
        self.assertEqual(run.call_args.args[0], ["kyth-installer-exec", "--operation", "recovery-export"])
        payload = json.loads(run.call_args.kwargs["input"])
        self.assertEqual(payload["usb_mount"], str(mount))

    def test_rescue_logs_native_export_rejects_malformed_or_failed_responses(self):
        with tempfile.TemporaryDirectory() as tmp:
            mount = Path(tmp) / "usb"
            mount.mkdir()
            for result in (
                mock.Mock(stdout=json.dumps({"ok": False})),
                mock.Mock(stdout="not json"),
            ):
                with (
                    mock.patch("kyth_installer.post_routes.shutil.which", return_value="/usr/bin/kyth-installer-exec"),
                    mock.patch("kyth_installer.runner.run_command", return_value=result),
                    mock.patch("kyth_installer.system._as_root", side_effect=lambda argv: argv),
                ):
                    response = self.routes.rescue_logs_to_usb({"usb_mount": str(mount)})
                self.assertEqual(response.status, 500)

    def test_rescue_logs_reports_missing_media_empty_logs_and_copy_failure(self):
        self.assertEqual(
            self.routes.rescue_logs_to_usb({"usb_mount": "/definitely/missing"}).status, 400
        )
        with tempfile.TemporaryDirectory() as tmp:
            missing = Path(tmp) / "missing"
            with (
                mock.patch("kyth_installer.config.LOG_FILE", missing),
                mock.patch("kyth_installer.config.TRANSACTION_FILE", missing),
                mock.patch("kyth_installer.config.FAILURE_SUMMARY_FILE", missing),
                mock.patch("kyth_installer.runner.run_command"),
            ):
                self.assertEqual(
                    self.routes.rescue_logs_to_usb({"usb_mount": tmp}).status, 500
                )
            with mock.patch(
                "kyth_installer.runner.run_command", side_effect=RuntimeError("copy failed")
            ):
                response = self.routes.rescue_logs_to_usb({"usb_mount": tmp})
            self.assertEqual(response.status, 500)
            self.assertIn("copy failed", response.payload["message"])

    def test_rescue_logs_auto_detect_covers_rglob_and_findmnt_branches(self):
        # success: rglob finds USB, findmnt confirms it, copy succeeds
        with tempfile.TemporaryDirectory() as tmp:
            mount = Path(tmp) / "usb"
            mount.mkdir()
            log_file = Path(tmp) / "install.log"
            log_file.write_text("log")
            # Computed before pathlib.Path is patched below: pathlib.Path.__new__ on
            # 3.11 dispatches on `cls is Path` against the *live* pathlib.Path global,
            # so constructing a real Path while that name is monkeypatched raises
            # "type object 'Path' has no attribute '_flavour'".
            missing_transaction_file = Path(tmp) / "missing"
            missing_failure_summary_file = Path(tmp) / "missing"
            fake_usb = mock.MagicMock(spec=Path)
            fake_usb.__str__.return_value = "/run/media/user/USB"
            fake_usb.is_dir.return_value = True
            mock_path_instance = mock.MagicMock()
            mock_path_instance.rglob.return_value = [fake_usb]
            # pathlib.Path("/run/media") returns mock_path_instance
            #
            # The fallback constructs PosixPath, not Path: while pathlib.Path is
            # patched below, Path(arg) recurses into Path.__new__'s `cls is Path`
            # OS-dispatch check, which compares against the now-patched module
            # global and raises "type object 'Path' has no attribute '_flavour'".
            # PosixPath(arg) is already the concrete class, so that dispatch is
            # never consulted. This suite only runs on Linux.
            def fake_path(arg):
                if arg == "/run/media":
                    return mock_path_instance
                return PosixPath(arg)

            mock_run = mock.Mock(return_value=mock.Mock(returncode=0))
            with (
                mock.patch("pathlib.Path", side_effect=fake_path),
                mock.patch("kyth_installer.runner.run_command", mock_run),
                mock.patch("kyth_installer.system._as_root", side_effect=lambda argv: argv),
                mock.patch("kyth_installer.config.LOG_FILE", log_file),
                mock.patch("kyth_installer.config.TRANSACTION_FILE", missing_transaction_file),
                mock.patch("kyth_installer.config.FAILURE_SUMMARY_FILE", missing_failure_summary_file),
                mock.patch("os.path.isdir", return_value=True),
            ):
                # need to also patch the local Path import inside function - pathlib.Path is already patched
                # provide mount via auto-detect: body has no usb_mount
                response = self.routes.rescue_logs_to_usb({})
                self.assertEqual(response.status, 200)
                mock_run.assert_any_call(["findmnt", "-n", str(fake_usb)], capture_output=True, timeout=3)

        # per-item exception is swallowed and next candidate is tried
        with tempfile.TemporaryDirectory() as tmp:
            cand_a = mock.MagicMock(spec=Path)
            cand_a.__str__.return_value = "/run/media/a"
            cand_a.is_dir.return_value = True
            cand_b = mock.MagicMock(spec=Path)
            cand_b.__str__.return_value = "/run/media/b"
            cand_b.is_dir.return_value = True
            candidates = [cand_a, cand_b]
            mock_path_instance = mock.MagicMock()
            mock_path_instance.rglob.return_value = candidates

            def fake_path2(arg):
                if arg == "/run/media":
                    return mock_path_instance
                return PosixPath(arg)  # see fake_path's comment above

            calls = []

            def run_side_effect(argv, **kwargs):
                calls.append(argv)
                if argv[0] == "findmnt":
                    if str(candidates[0]) in argv:
                        raise RuntimeError("findmnt boom")
                    return mock.Mock(returncode=0)
                return mock.Mock(returncode=0)

            log_file = Path(tmp) / "install.log"
            log_file.write_text("log")
            # See the note in the first block above: compute before pathlib.Path is patched.
            missing_transaction_file = Path(tmp) / "missing"
            missing_failure_summary_file = Path(tmp) / "missing"
            with (
                mock.patch("pathlib.Path", side_effect=fake_path2),
                mock.patch("kyth_installer.runner.run_command", side_effect=run_side_effect),
                mock.patch("kyth_installer.system._as_root", side_effect=lambda argv: argv),
                mock.patch("kyth_installer.config.LOG_FILE", log_file),
                mock.patch("kyth_installer.config.TRANSACTION_FILE", missing_transaction_file),
                mock.patch("kyth_installer.config.FAILURE_SUMMARY_FILE", missing_failure_summary_file),
                mock.patch("os.path.isdir", return_value=True),
            ):
                response = self.routes.rescue_logs_to_usb({})
                self.assertEqual(response.status, 200)

        # outer rglob exception is swallowed -> 400 (no USB found)
        mock_path_instance = mock.MagicMock()
        mock_path_instance.rglob.side_effect = OSError("rglob failed")

        def fake_path3(arg):
            if arg == "/run/media":
                return mock_path_instance
            return PosixPath(arg)  # see fake_path's comment above

        with mock.patch("pathlib.Path", side_effect=fake_path3), mock.patch("os.path.isdir", return_value=False):
            response = self.routes.rescue_logs_to_usb({})
            self.assertEqual(response.status, 400)
            self.assertIn("No USB", response.payload["message"])

        # findmnt returns non-zero for all candidates -> 400
        cand_x = mock.MagicMock(spec=Path)
        cand_x.__str__.return_value = "/run/media/x"
        cand_x.is_dir.return_value = True
        mock_path_instance = mock.MagicMock()
        mock_path_instance.rglob.return_value = [cand_x]

        def fake_path4(arg):
            if arg == "/run/media":
                return mock_path_instance
            return PosixPath(arg)  # see fake_path's comment above

        with (
            mock.patch("pathlib.Path", side_effect=fake_path4),
            mock.patch("kyth_installer.runner.run_command", return_value=mock.Mock(returncode=1)),
            mock.patch("os.path.isdir", return_value=False),
        ):
            response = self.routes.rescue_logs_to_usb({})
            self.assertEqual(response.status, 400)


if __name__ == "__main__":
    unittest.main()
