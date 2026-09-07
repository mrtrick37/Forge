"""Unit tests for the shared session module."""
from __future__ import annotations

import pathlib
import sys
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "build_files" / "kyth_shared"))

from kyth_shared.session import (
    already_run,
    check_firstboot_app_status,
    disable_vscode_brave_wallet_prompts,
    enable_vscode_brave_wallet_prompts,
    mark_run,
    marker_path,
    write_chromium_flags,
    write_code_argv,
    generate_session_snapshot,
)


class SessionTests(unittest.TestCase):
    def test_vscode_wallet_launcher_is_native(self) -> None:
        # The installed entry point is the native `kyth-vscode-wallet`
        # binary (see `system::browser_wallet` and `vscode_wallet_bin`);
        # this pins the behavior contract against the retained Python
        # fixture (`kyth_shared.session`) plus the native packaging
        # contract. No Python launcher remains in the source tree.
        self.assertFalse((ROOT / "build_files" / "kyth-vscode-wallet").exists())
        cargo = (ROOT / "src/kyth-shared-rs/Cargo.toml").read_text()
        dockerfile = (ROOT / "Dockerfile").read_text()
        self.assertIn('name = "kyth-vscode-wallet"', cargo)
        self.assertIn("/build/kyth-vscode-wallet /usr/bin/kyth-vscode-wallet", dockerfile)
        # The build-time sysconfig seeding runs the staged native binary.
        self.assertIn("/build/kyth-vscode-wallet /ctx/kyth-vscode-wallet", dockerfile)

    def test_vscode_wallet_fixture_enables_kwallet(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as home:
            home_path = pathlib.Path(home)
            (home_path / ".config" / "Code").mkdir(parents=True)
            (home_path / ".config" / "Code" / "argv.json").write_text(
                '{"zoom": 2}', encoding="utf-8"
            )
            enable_vscode_brave_wallet_prompts(home_path)
            argv = (home_path / ".config" / "Code" / "argv.json").read_text(
                encoding="utf-8"
            )
            self.assertIn('"password-store": "kwallet5"', argv)
            flags = home_path / ".config" / "brave-flags.conf"
            self.assertIn("--password-store=kwallet5", flags.read_text(encoding="utf-8"))

    @mock.patch("pathlib.Path.home")
    def test_marker_path(self, mock_home) -> None:
        mock_home.return_value = pathlib.Path("/dummy/home")
        self.assertEqual(marker_path("some-stamp"), pathlib.Path("/dummy/home/.local/share/kyth/some-stamp"))

    @mock.patch("pathlib.Path.is_file")
    def test_already_run(self, mock_is_file) -> None:
        mock_is_file.return_value = True
        self.assertTrue(already_run("some-stamp"))
        mock_is_file.return_value = False
        self.assertFalse(already_run("some-stamp"))

    @mock.patch("pathlib.Path.touch")
    @mock.patch("pathlib.Path.mkdir")
    def test_mark_run(self, mock_mkdir, mock_touch) -> None:
        mark_run("some-stamp")
        mock_mkdir.assert_called_once_with(parents=True, exist_ok=True)
        mock_touch.assert_called_once_with(exist_ok=True)

    @mock.patch("pathlib.Path.mkdir")
    def test_mark_run_ignores_errors(self, mock_mkdir) -> None:
        mock_mkdir.side_effect = OSError
        mark_run("some-stamp")
    @mock.patch("pathlib.Path.is_file")
    @mock.patch("pathlib.Path.read_text")
    @mock.patch("pathlib.Path.write_text")
    def test_write_code_argv(self, mock_write, mock_read, mock_is_file) -> None:
        mock_is_file.return_value = True
        mock_read.return_value = '{"some-setting": true}'
        p = pathlib.Path("/dummy/argv.json")
        write_code_argv(p)
        mock_write.assert_called_once()
        self.assertIn('"password-store": "kwallet5"', mock_write.call_args[0][0])

    @mock.patch("pathlib.Path.is_file")
    @mock.patch("pathlib.Path.read_text")
    @mock.patch("pathlib.Path.write_text")
    def test_write_chromium_flags(self, mock_write, mock_read, mock_is_file) -> None:
        mock_is_file.return_value = True
        mock_read.return_value = "--enable-features=UseOzonePlatform\n"
        p = pathlib.Path("/dummy/flags.conf")
        write_chromium_flags(p)
        mock_write.assert_called_once()
        self.assertIn("--password-store=kwallet5", mock_write.call_args[0][0])

    @mock.patch("kyth_shared.session.write_chromium_flags")
    @mock.patch("kyth_shared.session.write_code_argv")
    def test_disable_vscode_brave_wallet_prompts(self, mock_argv, mock_flags) -> None:
        # disable is kept as alias to enable for backward compatibility
        disable_vscode_brave_wallet_prompts(pathlib.Path("/dummy/home"))
        mock_argv.assert_called_once()
        self.assertEqual(mock_flags.call_count, 7)

    @mock.patch("kyth_shared.session.write_chromium_flags")
    @mock.patch("kyth_shared.session.write_code_argv")
    def test_enable_vscode_brave_wallet_prompts(self, mock_argv, mock_flags) -> None:
        from kyth_shared.session import enable_vscode_brave_wallet_prompts
        enable_vscode_brave_wallet_prompts(pathlib.Path("/dummy/home"))
        mock_argv.assert_called_once()
        self.assertEqual(mock_flags.call_count, 7)

    @mock.patch("shutil.which")
    @mock.patch("subprocess.run")
    @mock.patch("pathlib.Path.is_file")
    @mock.patch("time.sleep")
    def test_check_firstboot_app_status_ready(self, mock_sleep, mock_is_file, mock_run, mock_which) -> None:
        mock_which.return_value = "/bin/flatpak"
        mock_is_file.return_value = False

        # Mock flatpak info App ID commands: returncode=0 means installed
        mock_run.return_value = mock.Mock(returncode=0)

        with mock.patch("kyth_shared.session.write_app_status") as mock_write_status:
            ret = check_firstboot_app_status(force=True, delay=0)
            self.assertEqual(ret, 0)
            mock_write_status.assert_called_once_with(
                mock.ANY,
                "ready",
                "Steam, launchers, Bottles, and save backup tools are installed.",
            )

    @mock.patch("subprocess.run")
    @mock.patch("shutil.which")
    def test_generate_session_snapshot(self, mock_which, mock_run) -> None:
        import tempfile
        mock_which.side_effect = lambda cmd: "/dummy/bin/" + cmd if cmd == "kyth-nvidia-status" else None
        mock_run.return_value = mock.Mock(returncode=0, stdout="mocked stdout\n", stderr="")

        with tempfile.TemporaryDirectory() as tmpdir:
            out_dir = pathlib.Path(tmpdir)
            snapshot_path = generate_session_snapshot(out_dir)
            self.assertTrue(snapshot_path.is_file())
            content = snapshot_path.read_text(encoding="utf-8")
            self.assertIn("KythOS Session Snapshot", content)
            self.assertIn("== System ==", content)
            self.assertIn("== Desktop ==", content)
            self.assertIn("== Installed Flatpaks ==", content)
            self.assertIn("== Gaming Paths ==", content)
            self.assertIn("== KythOS Checks ==", content)
            self.assertIn("== Restore Notes ==", content)
            self.assertIn("$ kyth-nvidia-status", content)


if __name__ == "__main__":
    unittest.main()
