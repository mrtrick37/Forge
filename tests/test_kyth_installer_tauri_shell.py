"""Contracts for the Phase 2 React/Tauri installer shell."""
from __future__ import annotations

import json
import pathlib
import re
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
INSTALLER_WEB = ROOT / "src/kyth-installer-web"
SHELL_RS = INSTALLER_WEB / "src-tauri/src/main.rs"
NATIVE_RS = INSTALLER_WEB / "src-tauri/src/native_main.rs"
NATIVE_SLINT = INSTALLER_WEB / "src-tauri/ui/installer.slint"
LAUNCHER = ROOT / "build_files/kyth-launch-installer"
SERVER = ROOT / "src/kyth-installer/kyth_installer/server.py"


class InstallerTauriShellTests(unittest.TestCase):
    def test_native_shell_owns_the_first_parity_slice(self):
        rust = NATIVE_RS.read_text()
        slint = NATIVE_SLINT.read_text()
        for route in ("/api/start", "/api/cancel", "/api/reboot", "/api/rescue/logs-to-usb"):
            self.assertIn(route, rust)
        for field in ("install_mode", "hostname", "username", "password", "confirm_backup", "confirm_erase", "confirm_current"):
            self.assertIn(field, rust)
        for control in ("LineEdit", "CheckBox", "select-disk", "select-mode", "select-kernel", "start-install", "cancel-install", "rescue-probe", "reboot"):
            self.assertIn(control, slint)
        self.assertIn("post_json", rust)
        self.assertIn("ALLOWED", rust)
        self.assertIn("request_from_window", rust)

    def test_native_shell_validates_the_typed_plan_before_starting(self):
        rust = NATIVE_RS.read_text()
        self.assertIn("mod installer_plan;", rust)
        self.assertIn("fn validated_install_plan", rust)
        self.assertIn("installer_plan::build_plan", rust)
        self.assertIn("installer_validation::validate", rust)
        self.assertIn("install_plan_summary", rust)
        self.assertIn("validated_install_plan(&request)", rust)

    def test_native_shell_carries_guided_storage_choices_into_the_request(self):
        rust = NATIVE_RS.read_text()
        slint = NATIVE_SLINT.read_text()
        for route in ("/api/partitions?disk=", "/api/free-space?disk=", "storage_snapshot", "refresh_storage"):
            self.assertIn(route, rust)
        for field in ("target_partition", "free_region_start", "free_region_end", "resize_gib"):
            self.assertIn(f'"{field}"', rust)
        for control in ("storage-details", "partition-one", "free-region-one", "select-target-partition", "select-free-region", "resize-gib", "timezone", "locale", "keymap", "ComboBox", "timezone-options", "locale-options", "keymap-options"):
            self.assertIn(control, slint)
        self.assertIn('install_mode != "manual"', rust)
        self.assertIn('install-mode != "manual"', slint)
        self.assertIn("!part.current", rust)
        self.assertIn("!part.in_use", rust)
        self.assertIn('request.resize_partition = if window.get_install_mode().as_str() == "resize_ntfs"', rust)
        self.assertRegex(
            rust,
            re.escape('self.install_mode != "resize_ntfs"')
            + r"\s*\|\|\s*\(\s*!self\.resize_partition\.is_empty\(\)\s*&&\s*self\.resize_gib\s*>=\s*32\s*\)",
        )

    def test_native_install_uses_the_authenticated_event_stream(self):
        rust = NATIVE_RS.read_text()
        for marker in ("stream_install_events", "GET /api/stream", "text/event-stream", "data: ", "1800"):
            self.assertIn(marker, rust)
        for event_type in ("log", "progress", "phase", "done", "error"):
            self.assertIn(f'"{event_type}"', rust)

    def test_native_shell_decodes_transaction_reports_with_the_shared_rust_model(self):
        rust = NATIVE_RS.read_text()
        self.assertIn("mod installer_transaction;", rust)
        self.assertIn("installer_transaction::decode", rust)

    def test_native_shell_carries_manual_partition_actions_to_the_service(self):
        rust = NATIVE_RS.read_text()
        slint = NATIVE_SLINT.read_text()
        for route in (
            "/api/disk/new-table",
            "/api/disk/create",
            "/api/disk/delete",
            "/api/disk/resize",
            "/api/disk/format",
            "/api/disk/set-mountpoint",
            "/api/disk/pending/remove",
            "/api/disk/commit",
            "/api/disk/rollback",
        ):
            self.assertIn(route, rust)
        for action in ("new-table", "create", "delete", "resize", "format", "mountpoint", "remove-pending", "rollback", "commit"):
            self.assertIn(f'"{action}"', rust)
            self.assertIn(f'manual-action("{action}")', slint)
        for field in ("manual-filesystem", "manual-mountpoint", "manual-size-gib", "manual-committed"):
            self.assertIn(field, slint)
        self.assertIn('install_mode != "manual" || self.manual_committed', rust)

    def test_shell_embeds_assets_and_has_no_privileged_bridge(self):
        config = json.loads((INSTALLER_WEB / "src-tauri/tauri.conf.json").read_text())
        self.assertEqual(config["build"]["frontendDist"], "../dist")
        self.assertIn("127.0.0.1:7777", config["app"]["security"]["csp"])
        rust = SHELL_RS.read_text()
        self.assertIn('const BACKEND_URL: &str = "http://127.0.0.1:7777";', rust)
        self.assertIn("installer_connection", rust)
        self.assertIn("installer_validate_plan", rust)
        self.assertIn("installer_recovery_guidance", rust)
        self.assertIn("installer_request", rust)
        self.assertIn("installer_stream", rust)
        self.assertIn("allowlisted_path", rust)
        self.assertNotIn("Command::new", rust)
        self.assertNotIn("std::fs", rust)

    def test_launcher_starts_the_daemon_and_execs_the_native_shell(self):
        launcher = LAUNCHER.read_text()
        self.assertIn('if [ "$#" -ne 0 ]; then', launcher)
        self.assertIn("exit 64", launcher)
        self.assertIn('/usr/bin/sudo -n -- /usr/bin/kyth-launch-installer', launcher)
        self.assertIn('/usr/bin/systemctl start kyth-installerd.service', launcher)
        self.assertIn('/usr/bin/systemctl is-active --quiet kyth-installerd.service', launcher)
        self.assertIn('KYTH_INSTALLER_SOCKET="${SOCKET_PATH}"', launcher)
        self.assertIn('KYTH_INSTALLER_SESSION_TOKEN="${session_token}"', launcher)
        self.assertIn('/usr/bin/kyth-installer-shell --socket-path "${SOCKET_PATH}"', launcher)
        self.assertNotIn("chromium", launcher.lower())
        self.assertNotIn("--no-sandbox", launcher)
        self.assertNotIn("kyth_installer", launcher)
        self.assertNotIn("app.py", launcher)
        self.assertNotIn("--bootstrap-token", launcher)

    def test_frontend_uses_backend_token_and_fixed_dev_proxy(self):
        api = (INSTALLER_WEB / "src/api.ts").read_text()
        vite = (INSTALLER_WEB / "vite.config.ts").read_text()
        self.assertIn('invoke<InstallerConnection>("installer_connection")', api)
        self.assertIn('invoke("installer_validate_plan"', api)
        self.assertIn('invoke<NonNullable<RescueProbe["rescue_guidance"]>>("installer_recovery_guidance"', api)
        self.assertIn('X-Kyth-Session-Token', api)
        self.assertIn("session_token=", api)
        self.assertIn("127.0.0.1:7777", vite)

    def test_server_allows_only_tauri_cors_and_stream_auth(self):
        server = SERVER.read_text()
        self.assertIn('"http://tauri.localhost"', server)
        self.assertIn("Access-Control-Allow-Origin", server)
        self.assertIn("def do_OPTIONS", server)
        self.assertIn('parsed.path == "/api/stream"', server)
        self.assertIn('parsed.query).get("session_token")', server)
        self.assertNotIn('Access-Control-Allow-Origin", "*"', server)

    def test_server_has_opt_in_unix_transport_with_peer_credentials(self):
        server = SERVER.read_text()
        config = (ROOT / "src/kyth-installer/kyth_installer/config.py").read_text()
        self.assertIn("class UnixSocketServer", server)
        self.assertIn("SO_PEERCRED", server)
        self.assertIn("SOCKET_PATH", config)
        self.assertIn("SOCKET_GROUP", config)

    def test_image_build_copies_shell_from_isolated_builder(self):
        containerfile = (ROOT / "installer/Containerfile").read_text()
        self.assertIn("AS installer-web-builder", containerfile)
        self.assertIn("npm ci && npm run build", containerfile)
        self.assertIn("cargo build --release --locked", containerfile)
        self.assertIn("--bin kyth-installer-native", containerfile)
        self.assertIn("--bin kyth-installer-exec", containerfile)
        self.assertIn("/build/kyth-installer-native", containerfile)
        self.assertIn("/build/kyth-installer-exec", containerfile)
        self.assertIn("/usr/bin/kyth-installer-native", containerfile)
        self.assertIn("/usr/bin/kyth-installer-exec", containerfile)
        self.assertIn("COPY --from=installer-web-builder", containerfile)
        build = (ROOT / "installer/build.sh").read_text()
        self.assertIn("webkit2gtk4.1", build)
        self.assertIn("gtk3", build)


if __name__ == "__main__":
    unittest.main()
