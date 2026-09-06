import io
import json
import os
import sys
import unittest
from types import SimpleNamespace
from pathlib import Path
from unittest.mock import MagicMock, mock_open, patch

ROOT = Path(__file__).resolve().parents[1]
INSTALLER_ROOT = ROOT / "build_files/kyth-installer"
WEBUI_DIR = INSTALLER_ROOT / "kyth_installer/webui"
if str(INSTALLER_ROOT) not in sys.path:
    sys.path.insert(0, str(INSTALLER_ROOT))

from kyth_installer import (  # noqa: E402
    disk,
    install,
    partition_ops,
    plan,
    server,
    system,
)
from kyth_installer.context import InstallerContext  # noqa: E402


class InstallerWebuiTests(unittest.TestCase):
    @staticmethod
    def _javascript() -> str:
        return "\n".join(
            path.read_text()
            for path in sorted(WEBUI_DIR.glob("*.js"))
        )

    def test_accessibility_and_region_controls_are_exposed(self):
        html = (WEBUI_DIR / "index.html").read_text()

        self.assertIn('class="skip-link"', html)
        self.assertIn('id="toggle-high-contrast"', html)
        self.assertIn('role="progressbar"', html)
        self.assertIn('id="sel-locale"', html)
        self.assertIn('id="sel-keymap"', html)

    def test_ntfs_resize_ui_uses_backend_safety_flag(self):
        js = self._javascript()

        self.assertIn("p.ntfs_resize_candidate", js)
        self.assertIn("block.ref.ntfs_resize_candidate", js)

    def test_install_log_is_collapsed_until_toggle_opens_it(self):
        html = (WEBUI_DIR / "index.html").read_text()
        css = (WEBUI_DIR / "style.css").read_text()
        js = self._javascript()

        self.assertIn('id="log-toggle"', html)
        self.assertIn('aria-expanded="false"', html)
        self.assertIn('id="log-wrap" aria-hidden="true"', html)
        self.assertRegex(css, r"\.log-wrap\s*\{[^}]*display:\s*none;")
        self.assertRegex(css, r"\.log-wrap\.open\s*\{[^}]*display:\s*block;")
        self.assertIn("label.textContent = open ? 'Hide install log' : 'Show install log'", js)

    def test_disk_continue_button_id_matches_updater(self):
        html = (WEBUI_DIR / "index.html").read_text()
        js = self._javascript()

        self.assertIn('id="disk-next"', html)
        self.assertIn("document.getElementById('disk-next').disabled", js)
        self.assertIn("const btn = document.getElementById('disk-next');", js)
        self.assertNotIn("getElementById('next-disk')", js)

    def test_review_page_treats_target_partition_as_plain_name_string(self):
        # S.target_partition is set from p.name (a string), never a partition
        # object — `.name`/`.fstype` accesses on it are always undefined and
        # silently blank the review page's "which partition gets erased" text.
        js = self._javascript()

        self.assertNotIn("S.target_partition.name", js)
        self.assertNotIn("S.target_partition.fstype", js)

    def test_back_from_error_routes_to_config_not_configure(self):
        js = self._javascript()
        self.assertIn("goto(S.password ? 'review' : 'config')", js)
        self.assertNotIn("goto(S.password ? 'review' : 'configure')", js)

    def test_backend_route_table_keeps_frontend_api_paths(self):
        expected = {
            ("GET", "/api/disks"),
            ("GET", "/api/free-space"),
            ("GET", "/api/partitions"),
            ("GET", "/api/timezones"),
            ("GET", "/api/log"),
            ("POST", "/api/start"),
        }
        actual = {(route.method, route.path) for route in server.ROUTES.values()}

        self.assertTrue(expected.issubset(actual))

    def test_webui_and_backend_share_install_mode_names(self):
        js = self._javascript()

        for field in (
            "install_mode",
            "target_partition",
            "resize_partition",
            "free_region_start",
            "free_region_end",
        ):
            self.assertIn(field, js)

        for mode in ("wipe", "alongside", "free-space", "resize-ntfs"):
            self.assertEqual(plan._install_plan_from_state({"install_mode": mode}).mode, mode)


class InstallerCommandTests(unittest.TestCase):
    def test_installed_region_writes_locale_and_keyboard_configuration(self):
        calls = []

        def fake_run(command, **kwargs):
            calls.append((command, kwargs))
            return MagicMock(returncode=0, stdout="", stderr="")

        state = {
            "hostname": "kyth", "timezone": "Europe/Berlin",
            "locale": "de_DE.UTF-8", "keymap": "de",
        }
        with patch.object(install, "run_command", side_effect=fake_run), \
             patch.object(install, "_as_root", side_effect=lambda command: command):
            install._configure_hostname_timezone("/target/etc", state, lambda _message: None)

        inputs = [kwargs.get("input", "") for _command, kwargs in calls]
        self.assertIn("LANG=de_DE.UTF-8\n", inputs)
        self.assertIn("KEYMAP=de\n", inputs)

    def test_streaming_command_handles_carriage_return_progress(self):
        logs = []
        progress = []
        command = [
            sys.executable,
            "-c",
            (
                "import sys,time; "
                "sys.stdout.write('Downloading layer 1\\r'); sys.stdout.flush(); "
                "time.sleep(0.05); print('Writing image')"
            ),
        ]

        with patch.object(install, "_as_root", side_effect=lambda cmd: cmd), \
             patch.object(install, "get_rx_bytes", return_value=0), \
             patch(
                 "kyth_installer.runner._validate_executable",
                 side_effect=lambda executable: executable,
             ):
            install._run_cmd(
                command, 5, 90, logs.append, progress.append,
                stall_timeout=2, absolute_timeout=None,
            )

        self.assertIn("Downloading layer 1", logs)
        self.assertIn("Writing image", logs)
        self.assertEqual(progress[-1], 90)

    def test_bootc_install_calls_disable_absolute_timeout(self):
        source = (INSTALLER_ROOT / "kyth_installer/install.py").read_text()
        # Phase 2 verbatim: canonical impl moved to phases/storage.py, install.py re-exports
        storage = (INSTALLER_ROOT / "kyth_installer/phases/storage.py").read_text()
        combined = source + "\n" + storage

        self.assertEqual(combined.count("stall_timeout=3600, absolute_timeout=None"), 2)
        self.assertNotIn("absolute_timeout=14400", source)


class InstallerStorageTests(unittest.TestCase):
    def setUp(self):
        self.disk = disk

    def test_list_disks_excludes_protected_live_media(self):
        payload = {
            "blockdevices": [
                {"name": "/dev/nvme0n1", "size": 128 * 1024**3, "model": "Internal", "type": "disk", "tran": "nvme", "rota": False, "rm": False},
                {"name": "/dev/sdb", "size": 32 * 1024**3, "model": "Live USB", "type": "disk", "tran": "usb", "rota": False, "rm": True},
                {"name": "/dev/loop0", "size": 4 * 1024**3, "model": "Squashfs", "type": "disk", "tran": None, "rota": False, "rm": False},
            ]
        }

        with patch.object(self.disk, "_protected_install_disks", return_value={"/dev/sdb"}), \
             patch.object(self.disk, "run_command", return_value=SimpleNamespace(stdout=json.dumps(payload), returncode=0)):
            disks = self.disk.list_disks()

        self.assertEqual([d["name"] for d in disks], ["/dev/nvme0n1"])

    def test_list_disks_flags_running_system_disk_as_current(self):
        payload = {
            "blockdevices": [
                {"name": "/dev/nvme0n1", "size": 128 * 1024**3, "model": "Internal", "type": "disk", "tran": "nvme", "rota": False, "rm": False},
                {"name": "/dev/sdb", "size": 512 * 1024**3, "model": "Secondary", "type": "disk", "tran": "sata", "rota": False, "rm": False},
            ]
        }

        with patch.object(self.disk, "_protected_install_disks", return_value=set()), \
             patch.object(self.disk, "_running_system_disk", return_value="/dev/nvme0n1"), \
             patch.object(self.disk, "_parent_disk", return_value="/dev/nvme0n1"), \
             patch.object(self.disk, "run_command", return_value=SimpleNamespace(stdout=json.dumps(payload), returncode=0)):
            disks = {d["name"]: d for d in self.disk.list_disks()}

        self.assertTrue(disks["/dev/nvme0n1"]["current"])
        self.assertFalse(disks["/dev/sdb"]["current"])

    def test_list_disks_excludes_read_only_devices(self):
        payload = {"blockdevices": [
            {"name": "/dev/sda", "size": 64 * 1024**3, "model": "Read only", "type": "disk", "ro": True},
            {"name": "/dev/sdb", "size": 64 * 1024**3, "model": "Writable", "type": "disk", "ro": False},
        ]}
        with patch.object(self.disk, "_protected_install_disks", return_value=set()), \
             patch.object(self.disk, "_running_system_disk", return_value=""), \
             patch.object(self.disk, "run_command", return_value=SimpleNamespace(stdout=json.dumps(payload), returncode=0)):
            disks = self.disk.list_disks()

        self.assertEqual([item["name"] for item in disks], ["/dev/sdb"])

    def test_parent_disk_walks_through_lvm_and_luks_layers_via_batched_tree(self):
        # Root on an LVM logical volume backed by a LUKS-encrypted partition:
        # LV -> crypt mapper -> partition -> disk is three PKNAME hops, not
        # one. With a pre-fetched tree (as _protected_install_disks/list_disks
        # now pass in), this walk costs zero subprocess calls.
        tree = {
            "/dev/mapper/kyth-root": {"pkname": "/dev/dm-0", "type": "lvm"},
            "/dev/dm-0": {"pkname": "/dev/nvme0n1p3", "type": "crypt"},
            "/dev/nvme0n1p3": {"pkname": "/dev/nvme0n1", "type": "part"},
            "/dev/nvme0n1": {"pkname": None, "type": "disk"},
        }

        def fail_run_command(cmd, **_kwargs):
            raise AssertionError(f"unexpected subprocess call with a tree supplied: {cmd}")

        with patch.object(self.disk, "run_command", side_effect=fail_run_command):
            result = self.disk._parent_disk("/dev/mapper/kyth-root", tree=tree)

        self.assertEqual(result, "/dev/nvme0n1")

    def test_parent_disk_fetches_tree_once_when_not_supplied(self):
        payload = {
            "blockdevices": [
                {
                    "name": "/dev/nvme0n1", "pkname": None, "type": "disk",
                    "children": [
                        {"name": "/dev/nvme0n1p3", "pkname": "nvme0n1", "type": "part"},
                    ],
                },
            ]
        }
        calls = []

        def fake_run_command(cmd, **_kwargs):
            calls.append(cmd)
            return SimpleNamespace(stdout=json.dumps(payload), returncode=0)

        with patch.object(self.disk, "run_command", side_effect=fake_run_command):
            result = self.disk._parent_disk("/dev/nvme0n1p3")

        self.assertEqual(result, "/dev/nvme0n1")
        self.assertEqual(len(calls), 1, f"expected exactly one lsblk call, got {calls}")

    def test_parent_disk_falls_back_when_device_missing_from_tree(self):
        # A device absent from the batched snapshot (e.g. it appeared after
        # the tree was fetched) still resolves, via the old per-device calls,
        # instead of the whole walk silently failing.
        chain = {
            ("lsblk", "-n", "-o", "TYPE", "/dev/sdb1"): "part\n",
            ("lsblk", "-n", "-o", "PKNAME", "/dev/sdb1"): "sdb\n",
            ("lsblk", "-n", "-o", "TYPE", "/dev/sdb"): "disk\n",
        }

        def fake_run_command(cmd, **_kwargs):
            key = tuple(cmd)
            if key not in chain:
                raise AssertionError(f"unexpected lsblk invocation: {cmd}")
            return SimpleNamespace(stdout=chain[key], returncode=0)

        normalize = lambda p: p if p.startswith("/dev/") else f"/dev/{p}"
        with patch.object(self.disk, "_normal_device_path", side_effect=normalize), \
             patch.object(self.disk, "run_command", side_effect=fake_run_command):
            result = self.disk._parent_disk("/dev/sdb1", tree={})

        self.assertEqual(result, "/dev/sdb")

    def test_lsblk_tree_flattens_nested_children(self):
        payload = {
            "blockdevices": [
                {
                    "name": "/dev/nvme0n1", "pkname": None, "type": "disk",
                    "children": [
                        {
                            "name": "/dev/nvme0n1p3", "pkname": "nvme0n1", "type": "part",
                            "children": [
                                {"name": "/dev/dm-0", "pkname": "nvme0n1p3", "type": "crypt"},
                            ],
                        },
                    ],
                },
            ]
        }
        with patch.object(self.disk, "run_command", return_value=SimpleNamespace(stdout=json.dumps(payload), returncode=0)):
            tree = self.disk._lsblk_tree()

        self.assertEqual(tree["/dev/nvme0n1"], {"pkname": None, "type": "disk"})
        self.assertEqual(tree["/dev/nvme0n1p3"], {"pkname": "/dev/nvme0n1", "type": "part"})
        self.assertEqual(tree["/dev/dm-0"], {"pkname": "/dev/nvme0n1p3", "type": "crypt"})

    def test_list_partitions_marks_replaceable_unmounted_partitions(self):
        payload = {
            "blockdevices": [{
                "name": "/dev/nvme0n1",
                "type": "disk",
                "children": [
                    {"name": "/dev/nvme0n1p1", "size": 1024**3, "type": "part", "fstype": "vfat", "parttype": self.disk.EFI_PART_GUID, "label": "EFI", "mountpoints": ["/boot/efi"]},
                    {"name": "/dev/nvme0n1p2", "size": 80 * 1024**3, "type": "part", "fstype": "btrfs", "parttype": "", "label": "shared", "mountpoints": []},
                    {"name": "/dev/nvme0n1p3", "size": 40 * 1024**3, "type": "part", "fstype": "ext4", "parttype": "", "label": "other", "mountpoints": []},
                    {"name": "/dev/nvme0n1p4", "size": 40 * 1024**3, "type": "part", "fstype": "btrfs", "parttype": "", "label": "active", "mountpoints": ["/home"]},
                    {"name": "/dev/nvme0n1p5", "size": 80 * 1024**3, "type": "part", "fstype": "crypto_LUKS", "parttype": "", "label": "vault", "mountpoints": [], "children": [
                        {"name": "/dev/mapper/vault", "size": 80 * 1024**3, "type": "crypt", "fstype": "ext4", "mountpoints": []},
                    ]},
                ],
            }]
        }

        with patch.object(self.disk, "run_command", return_value=SimpleNamespace(stdout=json.dumps(payload), returncode=0)):
            parts = {p["name"]: p for p in self.disk.list_partitions("/dev/nvme0n1")}

        self.assertFalse(parts["/dev/nvme0n1p1"]["alongside_candidate"])
        self.assertTrue(parts["/dev/nvme0n1p2"]["alongside_candidate"])
        self.assertTrue(parts["/dev/nvme0n1p3"]["alongside_candidate"])
        self.assertFalse(parts["/dev/nvme0n1p4"]["alongside_candidate"])
        self.assertFalse(parts["/dev/nvme0n1p5"]["alongside_candidate"])
        self.assertTrue(parts["/dev/nvme0n1p5"]["in_use"])

    def test_list_partitions_never_offers_read_only_ntfs_for_resize(self):
        payload = {"blockdevices": [{
            "name": "/dev/sda", "type": "disk", "children": [{
                "name": "/dev/sda3", "size": 256 * 1024**3, "type": "part",
                "fstype": "ntfs", "parttype": "", "label": "Windows",
                "mountpoints": [], "ro": True,
            }],
        }]}
        with patch.object(self.disk, "run_command", return_value=SimpleNamespace(
            stdout=json.dumps(payload), returncode=0,
        )):
            part = self.disk.list_partitions("/dev/sda")[0]

        self.assertTrue(part["read_only"])
        self.assertFalse(part["alongside_candidate"])
        self.assertFalse(part["ntfs_resize_candidate"])

    def test_find_efi_partition_reads_efi_key_without_keyerror(self):
        partitions = [
            {"name": "/dev/nvme0n1p1", "efi": False},
            {"name": "/dev/nvme0n1p2", "efi": True},
        ]
        with patch.object(self.disk, "list_partitions", return_value=partitions):
            result = self.disk.find_efi_partition("/dev/nvme0n1")

        self.assertEqual(result, "/dev/nvme0n1p2")

    def test_find_efi_partition_falls_back_to_findmnt_when_no_partition_flagged(self):
        with patch.object(self.disk, "list_partitions", return_value=[{"name": "/dev/nvme0n1p1", "efi": False}]), \
             patch.object(self.disk, "_protected_install_disks", return_value=set()), \
             patch.object(self.disk, "_findmnt_source", return_value="/dev/nvme0n1p1"), \
             patch.object(self.disk, "_parent_disk", return_value="/dev/nvme0n1"):
            result = self.disk.find_efi_partition("/dev/nvme0n1")

        self.assertEqual(result, "/dev/nvme0n1p1")

    def test_list_free_space_finds_trailing_gap_after_partitions(self):
        reserve = 1024**2
        p1_start = reserve
        p1_size = 1 * 1024**3 - reserve
        p2_start = p1_start + p1_size  # contiguous with p1, no gap between them
        p2_size = 39 * 1024**3
        disk_size = 120 * 1024**3

        partitions = [
            {"name": "/dev/nvme0n1p1", "size_bytes": p1_size},
            {"name": "/dev/nvme0n1p2", "size_bytes": p2_size},
        ]
        starts = {"/dev/nvme0n1p1": p1_start, "/dev/nvme0n1p2": p2_start}

        with patch.object(self.disk, "list_partitions", return_value=partitions), \
             patch.object(self.disk, "_partition_size_bytes", return_value=disk_size), \
             patch.object(self.disk, "_block_size_bytes", return_value=512), \
             patch.object(self.disk, "_partition_start_bytes", side_effect=lambda name: starts[name]):
            regions = self.disk.list_free_space("/dev/nvme0n1")

        self.assertEqual(len(regions), 1)
        region = regions[0]
        self.assertEqual(region["start_bytes"], p2_start + p2_size)
        self.assertEqual(region["end_bytes"], disk_size - reserve)
        self.assertGreater(region["end_bytes"] - region["start_bytes"], self.disk.MIN_KYTHOS_BYTES)

    def test_list_free_space_omits_gaps_smaller_than_minimum(self):
        partitions = [{"name": "/dev/nvme0n1p1", "size_bytes": 300 * 1024**2}]
        with patch.object(self.disk, "list_partitions", return_value=partitions), \
             patch.object(self.disk, "_partition_size_bytes", return_value=310 * 1024**2), \
             patch.object(self.disk, "_block_size_bytes", return_value=512), \
             patch.object(self.disk, "_partition_start_bytes", return_value=1024**2):
            regions = self.disk.list_free_space("/dev/nvme0n1")

        self.assertEqual(regions, [])

    def test_list_free_space_fails_closed_when_partition_scan_fails(self):
        with patch.object(self.disk, "list_partitions", side_effect=RuntimeError("scan failed")), \
             patch.object(self.disk, "_partition_size_bytes", return_value=120 * 1024**3), \
             patch.object(self.disk, "_block_size_bytes", return_value=512):
            regions = self.disk.list_free_space("/dev/nvme0n1")

        self.assertEqual(regions, [])

    def test_latest_partition_on_disk_natural_sort(self):
        before = {"/dev/sda1", "/dev/sda2"}
        partitions = [
            {"name": "/dev/sda1"},
            {"name": "/dev/sda2"},
            {"name": "/dev/sda10"},
        ]
        with patch.object(self.disk, "list_partitions", return_value=partitions):
            result = self.disk._latest_partition_on_disk("/dev/sda", before)
        self.assertEqual(result, "/dev/sda10")

    def test_find_efi_partition_does_not_scan_other_disks(self):
        def fake_list_partitions(d):
            if d == "/dev/nvme0n1":
                return [{"name": "/dev/nvme0n1p1", "efi": True}]
            return [{"name": "/dev/nvme1n1p1", "efi": False}]

        with patch.object(self.disk, "list_partitions", side_effect=fake_list_partitions), \
             patch.object(self.disk, "list_disks", return_value=[{"name": "/dev/nvme0n1"}, {"name": "/dev/nvme1n1"}]), \
             patch.object(self.disk, "_protected_install_disks", return_value=set()), \
             patch.object(self.disk, "_findmnt_source", return_value=""):
            result = self.disk.find_efi_partition("/dev/nvme1n1")
            self.assertEqual(result, "")


@patch.dict("os.environ", {"KYTH_INSTALL_ALLOW_NO_DISK_LOCK": "1"}, clear=False)
class InstallerPlanTests(unittest.TestCase):
    def setUp(self):
        self.plan = plan

    def test_validate_alongside_requires_partition_on_selected_disk(self):
        with patch.object(self.plan, "list_disks", return_value=[{"name": "/dev/nvme0n1"}]), \
             patch.object(self.plan, "list_partitions", return_value=[]), \
             patch.object(self.plan, "_parent_disk", return_value="/dev/sda"):
            with self.assertRaisesRegex(RuntimeError, "does not belong"):
                self.plan._validate_install_target({
                    "install_mode": "alongside",
                    "disk": "/dev/nvme0n1",
                    "target_partition": "/dev/sda2",
                })

    def test_validate_alongside_allows_any_filesystem_partition(self):
        partition = {
            "name": "/dev/nvme0n1p2",
            "fstype": "ext4",
            "efi": False,
            "current": False,
            "size_bytes": 128 * 1024**3,
        }
        with patch.object(self.plan, "list_disks", return_value=[{"name": "/dev/nvme0n1"}]), \
             patch.object(self.plan, "list_partitions", return_value=[partition]), \
             patch.object(self.plan, "_parent_disk", return_value="/dev/nvme0n1"), \
             patch.object(self.plan, "_is_gpt_disk", return_value=False), \
             patch.object(self.plan, "find_efi_partition", return_value="/dev/nvme0n1p1"):
            disk_name, target = self.plan._validate_install_target({
                "install_mode": "alongside",
                "disk": "/dev/nvme0n1",
                "target_partition": "/dev/nvme0n1p2",
            })
            self.assertEqual((disk_name, target), ("/dev/nvme0n1", "/dev/nvme0n1p2"))

    def test_validate_alongside_rejects_gpt_without_bios_boot_partition(self):
        partition = {
            "name": "/dev/nvme0n1p2",
            "fstype": "ext4",
            "efi": False,
            "current": False,
            "size_bytes": 128 * 1024**3,
        }
        with patch.object(self.plan, "list_disks", return_value=[{"name": "/dev/nvme0n1"}]), \
             patch.object(self.plan, "list_partitions", return_value=[partition]), \
             patch.object(self.plan, "_parent_disk", return_value="/dev/nvme0n1"), \
             patch.object(self.plan, "_is_gpt_disk", return_value=True), \
             patch.object(self.plan, "_has_bios_boot_partition", return_value=False):
            with self.assertRaisesRegex(RuntimeError, "no BIOS boot partition"):
                self.plan._validate_install_target({
                    "install_mode": "alongside",
                    "disk": "/dev/nvme0n1",
                    "target_partition": "/dev/nvme0n1p2",
                })

    def test_validate_alongside_rechecks_explicit_efi_partition(self):
        target = {
            "name": "/dev/nvme0n1p3", "fstype": "ext4", "efi": False,
            "current": False, "size_bytes": 128 * 1024**3,
        }
        with patch.object(self.plan, "list_disks", return_value=[{"name": "/dev/nvme0n1"}]), \
             patch.object(self.plan, "list_partitions", return_value=[target]), \
             patch.object(self.plan, "_parent_disk", return_value="/dev/nvme0n1"), \
             patch.object(self.plan, "_is_gpt_disk", return_value=False), \
             patch.object(self.plan, "find_efi_partition", return_value="/dev/nvme0n1p1"):
            with self.assertRaisesRegex(RuntimeError, "no longer a valid EFI"):
                self.plan._validate_install_target({
                    "install_mode": "alongside", "disk": "/dev/nvme0n1",
                    "target_partition": "/dev/nvme0n1p3",
                    "efi_partition": "/dev/nvme0n1p2",
                })

    def test_validate_alongside_rejects_read_only_efi_partition(self):
        target = {
            "name": "/dev/nvme0n1p3", "fstype": "ext4", "efi": False,
            "current": False, "size_bytes": 128 * 1024**3,
        }
        esp = {"name": "/dev/nvme0n1p1", "fstype": "vfat", "efi": True, "read_only": True}
        with patch.object(self.plan, "list_disks", return_value=[{"name": "/dev/nvme0n1"}]), \
             patch.object(self.plan, "list_partitions", return_value=[esp, target]), \
             patch.object(self.plan, "_parent_disk", return_value="/dev/nvme0n1"), \
             patch.object(self.plan, "_is_gpt_disk", return_value=False), \
             patch.object(self.plan, "find_efi_partition", return_value="/dev/nvme0n1p1"):
            with self.assertRaisesRegex(RuntimeError, "read-only"):
                self.plan._validate_install_target({
                    "install_mode": "alongside", "disk": "/dev/nvme0n1",
                    "target_partition": "/dev/nvme0n1p3",
                    "efi_partition": "/dev/nvme0n1p1",
                })

    def test_validate_wipe_rejects_disk_missing_from_safe_scan(self):
        with patch.object(self.plan, "list_disks", return_value=[]):
            with self.assertRaisesRegex(RuntimeError, "not a safe install target"):
                self.plan._validate_install_target({"install_mode": "wipe", "disk": "/dev/sda"})

    def test_validate_wipe_rejects_disk_below_minimum_size(self):
        with patch.object(self.plan, "list_disks", return_value=[
            {"name": "/dev/sda", "size_bytes": 16 * 1024**3},
        ]):
            with self.assertRaisesRegex(RuntimeError, "too small"):
                self.plan._validate_install_target({"install_mode": "wipe", "disk": "/dev/sda"})

    def test_validate_wipe_accepts_disk_at_minimum_size(self):
        with patch.object(self.plan, "list_disks", return_value=[
            {"name": "/dev/sda", "size_bytes": 32 * 1024**3},
        ]):
            disk_name, target = self.plan._validate_install_target({"install_mode": "wipe", "disk": "/dev/sda"})

        self.assertEqual((disk_name, target), ("/dev/sda", None))

    def test_validate_resize_ntfs_allows_trailing_recovery_partition(self):
        partition = {
            "name": "/dev/nvme0n1p3",
            "fstype": "ntfs",
            "efi": False,
            "current": False,
            "size_bytes": 256 * 1024**3,
        }
        with patch.object(self.plan, "list_disks", return_value=[{"name": "/dev/nvme0n1"}]), \
             patch.object(self.plan, "list_partitions", return_value=[partition]), \
             patch.object(self.plan, "_parent_disk", return_value="/dev/nvme0n1"), \
             patch.object(self.plan, "_is_gpt_disk", return_value=False), \
             patch.object(self.plan, "find_efi_partition", return_value="/dev/nvme0n1p1"):
            result = self.plan._validate_resize_ntfs_target({
                "disk": "/dev/nvme0n1",
                "resize_partition": "/dev/nvme0n1p3",
                "resize_gib": 64,
            })

        self.assertEqual(result, ("/dev/nvme0n1", "/dev/nvme0n1p3", 64 * 1024**3))

    def test_validate_resize_ntfs_accepts_clean_last_ntfs_partition(self):
        partition = {
            "name": "/dev/nvme0n1p3",
            "fstype": "ntfs",
            "efi": False,
            "current": False,
            "size_bytes": 256 * 1024**3,
        }
        with patch.object(self.plan, "list_disks", return_value=[{"name": "/dev/nvme0n1"}]), \
             patch.object(self.plan, "list_partitions", return_value=[partition]), \
             patch.object(self.plan, "_parent_disk", return_value="/dev/nvme0n1"), \
             patch.object(self.plan, "_is_gpt_disk", return_value=False), \
             patch.object(self.plan, "find_efi_partition", return_value="/dev/nvme0n1p1"):
            disk_name, target, shrink = self.plan._validate_resize_ntfs_target({
                "disk": "/dev/nvme0n1",
                "resize_partition": "/dev/nvme0n1p3",
                "resize_gib": 64,
            })

        self.assertEqual(disk_name, "/dev/nvme0n1")
        self.assertEqual(target, "/dev/nvme0n1p3")
        self.assertEqual(shrink, 64 * 1024**3)

    def test_validate_resize_ntfs_rejects_bitlocker_with_targeted_message(self):
        partition = {
            "name": "/dev/nvme0n1p3",
            "fstype": "BitLocker",
            "efi": False,
            "current": False,
            "size_bytes": 256 * 1024**3,
        }
        with patch.object(self.plan, "list_disks", return_value=[{"name": "/dev/nvme0n1"}]), \
             patch.object(self.plan, "list_partitions", return_value=[partition]), \
             patch.object(self.plan, "_parent_disk", return_value="/dev/nvme0n1"), \
             patch.object(self.plan, "_is_gpt_disk", return_value=False), \
             patch.object(self.plan, "find_efi_partition", return_value="/dev/nvme0n1p1"):
            with self.assertRaisesRegex(RuntimeError, "BitLocker"):
                self.plan._validate_resize_ntfs_target({
                    "disk": "/dev/nvme0n1",
                    "resize_partition": "/dev/nvme0n1p3",
                    "resize_gib": 64,
                })

    def test_prepare_ntfs_resize_creates_btrfs_target_after_dry_run(self):
        import tempfile
        partition = "/dev/nvme0n1p3"
        commands = []

        def fake_run(cmd, **kwargs):
            commands.append(cmd)
            return self.plan.subprocess.CompletedProcess(cmd, 0, stdout="ok")

        # _latest_partition_on_disk (defined in disk.py) calls disk's own
        # list_partitions, not plan's imported reference, so both names must
        # be patched to the same mock to share the before/after side_effect.
        # p1 already carries the bios_grub GUID so _ensure_bios_boot_partition
        # (call 1) skips creation; calls 2 and 3 are the before/after
        # snapshots around the KythOS mkpart.
        existing = [
            {"name": "/dev/nvme0n1p1", "parttype": self.plan.BIOS_BOOT_GUID},
            {"name": partition},
        ]
        list_partitions_mock = MagicMock(side_effect=[
            existing,
            existing,
            existing + [{"name": "/dev/nvme0n1p4"}],
        ])
        mock_disk_service_cls = MagicMock()
        mock_disk_service = mock_disk_service_cls.return_value

        with patch.object(self.plan.shutil, "which", return_value="/usr/bin/tool"), \
             patch.object(self.plan, "unmount_target_disk") as mock_unmount, \
             patch.object(self.plan, "shrink_filesystem") as mock_shrink, \
             patch.object(self.plan, "DiskService", mock_disk_service_cls), \
             patch.object(self.plan, "_validate_resize_ntfs_target", return_value=("/dev/nvme0n1", partition, 64 * 1024**3)), \
             patch.object(self.plan, "_partition_size_bytes", side_effect=[256 * 1024**3, 192 * 1024**3]), \
             patch.object(self.plan, "_partition_number", return_value=3), \
             patch.object(self.plan, "_partition_start_bytes", return_value=128 * 1024**3), \
             patch.object(self.plan, "_block_size_bytes", return_value=512), \
             patch.object(self.plan, "list_partitions", list_partitions_mock), \
             patch.object(disk, "list_partitions", list_partitions_mock), \
             patch.object(self.plan, "_settle"), \
             patch.object(self.plan, "_is_gpt_disk", return_value=True), \
             patch.object(self.plan, "run_command", side_effect=fake_run), \
             tempfile.TemporaryDirectory() as marker_dir:
            # marker_root MUST be a throwaway temp dir: the real default
            # (/run/kyth-installer) is a live system path, and a shrink
            # exercised here would otherwise leave a real "already shrunk"
            # marker on the host that falsely blocks every later run.
            created = self.plan._prepare_ntfs_resize_target(
                {"disk": "/dev/nvme0n1", "resize_partition": partition, "resize_gib": 64},
                lambda _msg: None,
                marker_root=Path(marker_dir),
            )

        mock_unmount.assert_called_once_with("/dev/nvme0n1", unittest.mock.ANY)
        self.assertEqual(created, ("/dev/nvme0n1", "/dev/nvme0n1p4"))
        # The NTFS-safe shrink sequence (ntfsresize --check/--info/dry-run/
        # real shrink) now lives in fsresize.shrink_filesystem, with its own
        # tests — this test only verifies it's invoked before the partition
        # boundary moves, and with the right target size.
        mock_shrink.assert_called_once_with(partition, "ntfs", 192 * 1024**3, unittest.mock.ANY)
        flattened = [" ".join(cmd) for cmd in commands]
        mock_disk_service.resize_partition.assert_called_once_with(
            "/dev/nvme0n1", 3, 128 * 1024**3, 192 * 1024**3,
        )
        self.assertTrue(any("mkpart KythOS btrfs" in cmd and "100%" not in cmd for cmd in flattened))
        self.assertTrue(any("mkfs.btrfs -f -L KythOS /dev/nvme0n1p4" in cmd for cmd in flattened))

    def test_prepare_ntfs_resize_restores_partition_table_on_later_failure(self):
        # The NTFS filesystem shrink itself is mocked out (its own safety is
        # fsresize.py's job) — this test is specifically about the partition-
        # table backup/restore safety net around the parted/mkfs steps that
        # run *after* a real, successful filesystem shrink.
        #
        # Only the low-level shrink_filesystem primitive is mocked below; the
        # real shrink_ntfs_filesystem_guarded wrapper still runs and writes an
        # "already shrunk" marker on success — so marker_root must point at a
        # throwaway temp dir, not the real /run/kyth-installer default.
        import tempfile
        partition = "/dev/nvme0n1p3"

        def fake_run(cmd, **kwargs):
            if "mkfs.btrfs" in " ".join(cmd):
                raise RuntimeError("mkfs.btrfs exploded")
            return self.plan.subprocess.CompletedProcess(cmd, 0, stdout="ok")

        existing = [
            {"name": "/dev/nvme0n1p1", "parttype": self.plan.BIOS_BOOT_GUID},
            {"name": partition},
        ]
        list_partitions_mock = MagicMock(side_effect=[
            existing, existing, existing + [{"name": "/dev/nvme0n1p4"}],
        ])
        mock_disk_service_cls = MagicMock()
        mock_disk_service = mock_disk_service_cls.return_value

        with patch.object(self.plan.shutil, "which", return_value="/usr/bin/tool"), \
             patch.object(self.plan, "unmount_target_disk"), \
             patch.object(self.plan, "shrink_filesystem"), \
             patch.object(self.plan, "DiskService", mock_disk_service_cls), \
             patch.object(self.plan, "_validate_resize_ntfs_target", return_value=("/dev/nvme0n1", partition, 64 * 1024**3)), \
             patch.object(self.plan, "_partition_size_bytes", side_effect=[256 * 1024**3, 192 * 1024**3]), \
             patch.object(self.plan, "_partition_number", return_value=3), \
             patch.object(self.plan, "_partition_start_bytes", return_value=128 * 1024**3), \
             patch.object(self.plan, "_block_size_bytes", return_value=512), \
             patch.object(self.plan, "list_partitions", list_partitions_mock), \
             patch.object(disk, "list_partitions", list_partitions_mock), \
             patch.object(self.plan, "_settle"), \
             patch.object(self.plan, "_is_gpt_disk", return_value=True), \
             patch.object(self.plan, "run_command", side_effect=fake_run), \
             tempfile.TemporaryDirectory() as marker_dir:
            with self.assertRaisesRegex(RuntimeError, "mkfs.btrfs exploded"):
                self.plan._prepare_ntfs_resize_target(
                    {"disk": "/dev/nvme0n1", "resize_partition": partition, "resize_gib": 64},
                    lambda _msg: None,
                    marker_root=Path(marker_dir),
                )

        mock_disk_service.backup_table.assert_called_once()
        mock_disk_service.restore_table.assert_called_once()

    def test_prepare_free_space_target_restores_partition_table_on_later_failure(self):
        def fake_run(cmd, **kwargs):
            if "mkfs.btrfs" in " ".join(cmd):
                raise RuntimeError("mkfs.btrfs exploded")
            return self.plan.subprocess.CompletedProcess(cmd, 0, stdout="ok")

        mock_disk_service_cls = MagicMock()
        mock_disk_service = mock_disk_service_cls.return_value

        with patch.object(self.plan.shutil, "which", return_value="/usr/bin/tool"), \
             patch.object(self.plan, "unmount_target_disk"), \
             patch.object(self.plan, "DiskService", mock_disk_service_cls), \
             patch.object(self.plan, "_validate_free_space_target", return_value=("/dev/nvme0n1", 40 * 1024**3, 80 * 1024**3)), \
             patch.object(self.plan, "list_partitions", return_value=[
                 {"name": "/dev/nvme0n1p1", "parttype": self.plan.BIOS_BOOT_GUID},
             ]), \
             patch.object(self.plan, "_latest_partition_on_disk", return_value="/dev/nvme0n1p2"), \
             patch.object(self.plan, "_block_size_bytes", return_value=512), \
             patch.object(self.plan, "_settle"), \
             patch.object(self.plan, "_is_gpt_disk", return_value=True), \
             patch.object(self.plan, "run_command", side_effect=fake_run):
            with self.assertRaisesRegex(RuntimeError, "mkfs.btrfs exploded"):
                self.plan._prepare_free_space_target(
                    {"disk": "/dev/nvme0n1", "free_region_start": 40 * 1024**3, "free_region_end": 80 * 1024**3},
                    lambda _msg: None,
                )

        mock_disk_service.backup_table.assert_called_once()
        mock_disk_service.restore_table.assert_called_once()

    def test_ensure_bios_boot_partition_creates_and_flags_when_missing(self):
        # The OS image ships a bootupd BIOS component and bootc installs every
        # shipped component; without a bios_grub partition grub2-install falls
        # back to blocklists, which Btrfs rejects, failing the install.
        commands = []

        def fake_run(cmd, **kwargs):
            commands.append(" ".join(cmd))
            return self.plan.subprocess.CompletedProcess(cmd, 0, stdout="ok")

        gap_start = 128 * 1024**3
        with patch.object(self.plan, "list_partitions", return_value=[{"name": "/dev/sda1", "parttype": ""}]), \
             patch.object(self.plan, "_latest_partition_on_disk", return_value="/dev/sda2"), \
             patch.object(self.plan, "_partition_number", return_value=2), \
             patch.object(self.plan, "_block_size_bytes", return_value=512), \
             patch.object(self.plan, "_settle"), \
             patch.object(self.plan, "_is_gpt_disk", return_value=True), \
             patch.object(self.plan, "run_command", side_effect=fake_run):
            btrfs_start = self.plan._ensure_bios_boot_partition("/dev/sda", gap_start, lambda _msg: None)

        self.assertEqual(btrfs_start, gap_start + self.plan.BIOS_BOOT_BYTES)
        bios_end = gap_start + self.plan.BIOS_BOOT_BYTES - 512
        self.assertTrue(any(f"mkpart biosboot {gap_start}B {bios_end}B" in cmd for cmd in commands))
        self.assertTrue(any("set 2 bios_grub on" in cmd for cmd in commands))

    def test_ensure_bios_boot_partition_skips_when_already_present(self):
        with patch.object(self.plan, "_is_gpt_disk", return_value=True), \
             patch.object(self.plan, "list_partitions", return_value=[
                 {"name": "/dev/sda1", "parttype": self.plan.BIOS_BOOT_GUID},
             ]), patch.object(self.plan, "run_command") as mock_run:
            btrfs_start = self.plan._ensure_bios_boot_partition("/dev/sda", 4096, lambda _msg: None)

        self.assertEqual(btrfs_start, 4096)
        mock_run.assert_not_called()

    def test_validate_free_space_rejects_region_below_minimum_size(self):
        with patch.object(self.plan, "list_disks", return_value=[{"name": "/dev/nvme0n1"}]), \
             patch.object(self.plan, "find_efi_partition", return_value="/dev/nvme0n1p1"):
            with self.assertRaisesRegex(RuntimeError, "at least"):
                self.plan._validate_free_space_target({
                    "disk": "/dev/nvme0n1",
                    "free_region_start": 1024**2,
                    "free_region_end": 16 * 1024**3,
                })

    def test_validate_free_space_reserves_room_for_new_bios_partition(self):
        start = 40 * 1024**3
        end = start + 32 * 1024**3
        with patch.object(self.plan, "list_disks", return_value=[{"name": "/dev/nvme0n1"}]), \
             patch.object(self.plan, "list_partitions", return_value=[]), \
             patch.object(self.plan, "_is_gpt_disk", return_value=True):
            with self.assertRaisesRegex(RuntimeError, "33 GiB"):
                self.plan._validate_free_space_target({
                    "disk": "/dev/nvme0n1",
                    "free_region_start": start,
                    "free_region_end": end,
                })

    def test_validate_free_space_rejects_stale_region_no_longer_free(self):
        with patch.object(self.plan, "list_disks", return_value=[{"name": "/dev/nvme0n1"}]), \
             patch.object(self.plan, "find_efi_partition", return_value="/dev/nvme0n1p1"), \
             patch.object(self.plan, "list_free_space", return_value=[]):
            with self.assertRaisesRegex(RuntimeError, "no longer available"):
                self.plan._validate_free_space_target({
                    "disk": "/dev/nvme0n1",
                    "free_region_start": 40 * 1024**3,
                    "free_region_end": 80 * 1024**3,
                })

    def test_validate_free_space_requires_efi_partition(self):
        with patch.object(self.plan, "list_disks", return_value=[{"name": "/dev/nvme0n1"}]), \
             patch.object(self.plan, "find_efi_partition", return_value=""):
            with self.assertRaisesRegex(RuntimeError, "EFI system partition"):
                self.plan._validate_free_space_target({
                    "disk": "/dev/nvme0n1",
                    "free_region_start": 40 * 1024**3,
                    "free_region_end": 80 * 1024**3,
                })

    def test_validate_free_space_accepts_exact_region_from_current_scan(self):
        with patch.object(self.plan, "list_disks", return_value=[{"name": "/dev/nvme0n1"}]), \
             patch.object(self.plan, "find_efi_partition", return_value="/dev/nvme0n1p1"), \
             patch.object(self.plan, "list_free_space", return_value=[
                 {"start_bytes": 40 * 1024**3, "end_bytes": 80 * 1024**3},
             ]):
            disk_name, start, end = self.plan._validate_free_space_target({
                "disk": "/dev/nvme0n1",
                "free_region_start": 40 * 1024**3,
                "free_region_end": 80 * 1024**3,
            })

        self.assertEqual((disk_name, start, end), ("/dev/nvme0n1", 40 * 1024**3, 80 * 1024**3))

    def test_validate_free_space_rejects_ui_supplied_subregion(self):
        with patch.object(self.plan, "list_disks", return_value=[{"name": "/dev/nvme0n1"}]), \
             patch.object(self.plan, "find_efi_partition", return_value="/dev/nvme0n1p1"), \
             patch.object(self.plan, "list_free_space", return_value=[
                 {"start_bytes": 40 * 1024**3, "end_bytes": 100 * 1024**3},
             ]):
            with self.assertRaisesRegex(RuntimeError, "no longer available"):
                self.plan._validate_free_space_target({
                    "disk": "/dev/nvme0n1",
                    "free_region_start": 40 * 1024**3,
                    "free_region_end": 80 * 1024**3,
                })

    def test_prepare_free_space_target_creates_btrfs_partition(self):
        commands = []

        def fake_run(cmd, **kwargs):
            commands.append(cmd)
            return self.plan.subprocess.CompletedProcess(cmd, 0, stdout="ok")

        with patch.object(self.plan.shutil, "which", return_value="/usr/bin/tool"), \
             patch.object(self.plan, "unmount_target_disk") as mock_unmount, \
             patch.object(self.plan, "DiskService"), \
             patch.object(self.plan, "_validate_free_space_target", return_value=("/dev/nvme0n1", 40 * 1024**3, 80 * 1024**3)), \
             patch.object(self.plan, "list_partitions", return_value=[
                 {"name": "/dev/nvme0n1p1", "parttype": self.plan.BIOS_BOOT_GUID},
             ]), \
             patch.object(self.plan, "_latest_partition_on_disk", return_value="/dev/nvme0n1p2"), \
             patch.object(self.plan, "_block_size_bytes", return_value=512), \
             patch.object(self.plan, "_settle"), \
             patch.object(self.plan, "_is_gpt_disk", return_value=True), \
             patch.object(self.plan, "run_command", side_effect=fake_run):
            created = self.plan._prepare_free_space_target(
                {"disk": "/dev/nvme0n1", "free_region_start": 40 * 1024**3, "free_region_end": 80 * 1024**3},
                lambda _msg: None,
            )

        mock_unmount.assert_called_once_with("/dev/nvme0n1", unittest.mock.ANY)
        self.assertEqual(created, ("/dev/nvme0n1", "/dev/nvme0n1p2"))
        flattened = [" ".join(cmd) for cmd in commands]
        self.assertTrue(any(
            f"parted -s /dev/nvme0n1 unit B mkpart KythOS btrfs {40 * 1024**3}B {80 * 1024**3 - 512}B" in cmd
            for cmd in flattened
        ))
        self.assertTrue(any("mkfs.btrfs -f -L KythOS /dev/nvme0n1p2" in cmd for cmd in flattened))

    def test_prepare_free_space_target_requires_partitioning_tools(self):
        with patch.object(self.plan.shutil, "which", return_value=None), \
             patch.object(self.plan, "unmount_target_disk"), \
             patch.object(self.plan, "_validate_free_space_target", return_value=("/dev/nvme0n1", 40 * 1024**3, 80 * 1024**3)):
            with self.assertRaisesRegex(RuntimeError, "Required partitioning tools"):
                self.plan._prepare_free_space_target(
                    {"disk": "/dev/nvme0n1", "free_region_start": 40 * 1024**3, "free_region_end": 80 * 1024**3},
                    lambda _msg: None,
                )


class JournalValidateTests(unittest.TestCase):
    """Journal.validate() gates real partitioning safety properties (no
    overlaps, exactly one Btrfs root, never touch a mounted/in-use partition)
    but previously had no direct test coverage of its own."""

    def setUp(self):
        # validate() looks up the disk's current partition_table to gate the
        # MBR 4-primary-partition limit; default to "no such disk" (empty
        # table_type, same as the GPT/non-msdos path) so every test not
        # specifically about msdos doesn't need to mock this itself and
        # never makes a real lsblk call.
        patcher = patch.object(partition_ops, "list_disks", return_value=[])
        patcher.start()
        self.addCleanup(patcher.stop)

    def _journal(self, current_parts=()):
        with patch.object(partition_ops, "list_partitions", return_value=list(current_parts)):
            return partition_ops.Journal("/dev/nvme0n1")

    def test_empty_journal_is_rejected(self):
        journal = self._journal()
        with patch.object(partition_ops, "list_partitions", return_value=[]):
            errors = journal.validate()
        self.assertIn("No partition operations have been added.", errors)

    def test_missing_root_partition_is_rejected(self):
        journal = self._journal()
        journal.add_op("create", {
            "start_bytes": 1024**2, "size_bytes": 10 * 1024**3, "fs_type": "btrfs", "mountpoint": "",
        })
        with patch.object(partition_ops, "list_partitions", return_value=[]):
            errors = journal.validate()
        self.assertTrue(any("No root partition" in e for e in errors))

    def test_root_partition_must_be_btrfs(self):
        journal = self._journal()
        journal.add_op("create", {
            "start_bytes": 1024**2, "size_bytes": 10 * 1024**3, "fs_type": "ext4", "mountpoint": "/",
        })
        with patch.object(partition_ops, "list_partitions", return_value=[]):
            errors = journal.validate()
        self.assertTrue(any("must use the Btrfs filesystem" in e for e in errors))
        # Still flagged as satisfying "has a root", so this must be the only error.
        self.assertEqual(len(errors), 1)

    def test_overlapping_creates_are_rejected(self):
        journal = self._journal()
        journal.add_op("create", {
            "start_bytes": 1024**2, "size_bytes": 10 * 1024**3, "fs_type": "btrfs", "mountpoint": "/",
        })
        journal.add_op("create", {
            "start_bytes": 5 * 1024**3, "size_bytes": 10 * 1024**3, "fs_type": "ext4", "mountpoint": "",
        })
        with patch.object(partition_ops, "list_partitions", return_value=[]):
            errors = journal.validate()
        self.assertTrue(any("overlaps with existing region" in e for e in errors))

    def test_create_overlapping_existing_partition_is_rejected(self):
        journal = self._journal()
        journal.add_op("create", {
            "start_bytes": 5 * 1024**3, "size_bytes": 10 * 1024**3,
            "fs_type": "btrfs", "mountpoint": "/",
        })
        with patch.object(partition_ops, "list_partitions", return_value=[{
            "name": "/dev/nvme0n1p1", "start_bytes": 1024**2,
            "size_bytes": 10 * 1024**3, "fstype": "ntfs",
        }]):
            errors = journal.validate()
        self.assertTrue(any("overlaps with existing region" in error for error in errors))

    def test_cross_disk_partition_operation_is_rejected(self):
        journal = self._journal()
        journal.add_op("set_mountpoint", {"partition": "/dev/sdb1", "mountpoint": "/"})
        with patch.object(partition_ops, "list_partitions", return_value=[]), \
             patch.object(partition_ops, "_parent_disk", return_value="/dev/sdb"):
            errors = journal.validate()
        self.assertTrue(any("does not belong" in error for error in errors))

    def test_multiple_root_assignments_are_rejected(self):
        journal = self._journal()
        for index in range(2):
            journal.add_op("create", {
                "start_bytes": (index * 50 + 1) * 1024**3,
                "size_bytes": 40 * 1024**3, "fs_type": "btrfs", "mountpoint": "/",
            })
        with patch.object(partition_ops, "list_partitions", return_value=[]):
            errors = journal.validate()
        self.assertTrue(any("Exactly one root" in error for error in errors))

    def test_created_efi_partition_must_be_fat32(self):
        journal = self._journal()
        journal.add_op("create", {
            "start_bytes": 1024**2, "size_bytes": 1024**3,
            "fs_type": "ext4", "mountpoint": "/boot/efi",
        })
        journal.add_op("create", {
            "start_bytes": 2 * 1024**3, "size_bytes": 40 * 1024**3,
            "fs_type": "btrfs", "mountpoint": "/",
        })
        with patch.object(partition_ops, "list_partitions", return_value=[]):
            errors = journal.validate()
        self.assertTrue(any("must use FAT32" in error for error in errors))

    def test_existing_root_partition_must_be_btrfs_after_staged_format(self):
        journal = self._journal()
        journal.add_op("format", {"partition": "/dev/nvme0n1p2", "fs_type": "ext4"})
        journal.add_op("set_mountpoint", {"partition": "/dev/nvme0n1p2", "mountpoint": "/"})
        with patch.object(partition_ops, "list_partitions", return_value=[{
            "name": "/dev/nvme0n1p2", "fstype": "btrfs",
        }]), patch.object(partition_ops, "_parent_disk", return_value="/dev/nvme0n1"):
            errors = journal.validate()
        self.assertTrue(any("must use the Btrfs filesystem" in error for error in errors))

    def test_existing_efi_partition_must_be_fat32(self):
        journal = self._journal()
        journal.add_op("set_mountpoint", {
            "partition": "/dev/nvme0n1p1", "mountpoint": "/boot/efi",
        })
        journal.add_op("set_mountpoint", {
            "partition": "/dev/nvme0n1p2", "mountpoint": "/",
        })
        with patch.object(partition_ops, "list_partitions", return_value=[
            {"name": "/dev/nvme0n1p1", "fstype": "ext4"},
            {"name": "/dev/nvme0n1p2", "fstype": "btrfs"},
        ]), patch.object(partition_ops, "_parent_disk", return_value="/dev/nvme0n1"):
            errors = journal.validate()
        self.assertTrue(any("must use FAT32" in error for error in errors))

    def test_valid_single_root_partition_has_no_errors(self):
        journal = self._journal()
        journal.add_op("create", {
            "start_bytes": 1024**2, "size_bytes": 40 * 1024**3, "fs_type": "btrfs", "mountpoint": "/",
        })
        with patch.object(partition_ops, "list_partitions", return_value=[]):
            errors = journal.validate()
        self.assertEqual(errors, [])

    def test_new_table_op_resets_prior_allocations_and_root_flag(self):
        journal = self._journal()
        journal.add_op("create", {
            "start_bytes": 1024**2, "size_bytes": 40 * 1024**3, "fs_type": "btrfs", "mountpoint": "/",
        })
        journal.add_op("new_table", {"table_type": "gpt"})
        with patch.object(partition_ops, "list_partitions", return_value=[]):
            errors = journal.validate()
        # The new_table wipes the earlier root, so validate must complain
        # about a missing root rather than silently accepting the stale one.
        self.assertTrue(any("No root partition" in e for e in errors))

    def test_msdos_table_rejects_a_5th_primary_partition(self):
        journal = self._journal()
        journal.add_op("new_table", {"table_type": "msdos"})
        for i in range(4):
            journal.add_op("create", {
                "start_bytes": i * 10 * 1024**3 + 1024**2, "size_bytes": 9 * 1024**3,
                "fs_type": "btrfs" if i == 0 else "ext4", "mountpoint": "/" if i == 0 else "",
            })
        journal.add_op("create", {
            "start_bytes": 41 * 1024**3, "size_bytes": 9 * 1024**3, "fs_type": "ext4", "mountpoint": "",
        })
        with patch.object(partition_ops, "list_partitions", return_value=[]):
            errors = journal.validate()
        self.assertTrue(any("at most 4 primary partitions" in e for e in errors))

    def test_msdos_table_allows_up_to_4_primary_partitions(self):
        journal = self._journal()
        journal.add_op("new_table", {"table_type": "msdos"})
        for i in range(4):
            journal.add_op("create", {
                "start_bytes": i * 10 * 1024**3 + 1024**2, "size_bytes": 9 * 1024**3,
                "fs_type": "btrfs" if i == 0 else "ext4", "mountpoint": "/" if i == 0 else "",
            })
        with patch.object(partition_ops, "list_partitions", return_value=[]):
            errors = journal.validate()
        self.assertEqual(errors, [])

    def test_msdos_limit_counts_preexisting_partitions_without_a_new_table_op(self):
        journal = self._journal()
        # No new_table op — this journal partitions onto the disk's existing
        # (already-msdos) table, so the 3 pre-existing partitions count too.
        journal.add_op("create", {
            "start_bytes": 31 * 1024**3, "size_bytes": 9 * 1024**3, "fs_type": "btrfs", "mountpoint": "/",
        })
        journal.add_op("create", {
            "start_bytes": 41 * 1024**3, "size_bytes": 9 * 1024**3, "fs_type": "ext4", "mountpoint": "",
        })
        with patch.object(partition_ops, "list_disks", return_value=[
            {"name": "/dev/nvme0n1", "partition_table": "msdos"},
        ]), patch.object(partition_ops, "list_partitions", return_value=[
            {"name": "/dev/nvme0n1p1"}, {"name": "/dev/nvme0n1p2"}, {"name": "/dev/nvme0n1p3"},
        ]):
            errors = journal.validate()
        self.assertTrue(any("at most 4 primary partitions" in e for e in errors))

    def test_format_of_mounted_partition_is_rejected(self):
        journal = self._journal()
        journal.add_op("create", {
            "start_bytes": 1024**2, "size_bytes": 40 * 1024**3, "fs_type": "btrfs", "mountpoint": "/",
        })
        journal.add_op("format", {"partition": "/dev/nvme0n1p3", "fs_type": "ext4"})
        with patch.object(partition_ops, "list_partitions", return_value=[
            {"name": "/dev/nvme0n1p3", "current": True},
        ]):
            errors = journal.validate()
        self.assertTrue(any("currently mounted or in use" in e for e in errors))

    def test_set_mountpoint_root_on_in_use_partition_is_rejected(self):
        journal = self._journal()
        journal.add_op("set_mountpoint", {"partition": "/dev/nvme0n1p3", "mountpoint": "/"})
        with patch.object(partition_ops, "list_partitions", return_value=[
            {"name": "/dev/nvme0n1p3", "in_use": True},
        ]):
            errors = journal.validate()
        self.assertTrue(any("Cannot set /dev/nvme0n1p3 as the root partition" in e for e in errors))

    def test_resize_growing_into_neighboring_partition_is_rejected(self):
        # The Journal is the authoritative safety gate for partition ops
        # (see partition_ops.py module docstring); it must not rely on the
        # only current caller (InstallerService.resize_partition) disallowing
        # growth to keep a growing resize from overlapping its neighbor.
        journal = self._journal()
        journal.add_op("set_mountpoint", {"partition": "/dev/nvme0n1p1", "mountpoint": "/"})
        journal.add_op("resize", {"partition": "/dev/nvme0n1p1", "new_size_bytes": 15 * 1024**3})
        with patch.object(partition_ops, "list_partitions", return_value=[
            {"name": "/dev/nvme0n1p1", "start_bytes": 1024**2, "size_bytes": 10 * 1024**3, "fstype": "btrfs"},
            {"name": "/dev/nvme0n1p2", "start_bytes": 1024**2 + 10 * 1024**3, "size_bytes": 10 * 1024**3, "fstype": "ntfs"},
        ]), patch.object(partition_ops, "_parent_disk", return_value="/dev/nvme0n1"):
            errors = journal.validate()
        self.assertTrue(any("would overlap with existing region" in e for e in errors))

    def test_resize_growing_past_end_of_disk_is_rejected(self):
        journal = self._journal()
        journal.add_op("set_mountpoint", {"partition": "/dev/nvme0n1p1", "mountpoint": "/"})
        journal.add_op("resize", {"partition": "/dev/nvme0n1p1", "new_size_bytes": 25 * 1024**3})
        with patch.object(partition_ops, "list_disks", return_value=[
            {"name": "/dev/nvme0n1", "size_bytes": 20 * 1024**3},
        ]), patch.object(partition_ops, "list_partitions", return_value=[
            {"name": "/dev/nvme0n1p1", "start_bytes": 1024**2, "size_bytes": 10 * 1024**3, "fstype": "btrfs"},
        ]), patch.object(partition_ops, "_parent_disk", return_value="/dev/nvme0n1"):
            errors = journal.validate()
        self.assertTrue(any("extends past the end of" in e for e in errors))

    def test_resize_shrink_within_bounds_has_no_errors(self):
        journal = self._journal()
        journal.add_op("set_mountpoint", {"partition": "/dev/nvme0n1p1", "mountpoint": "/"})
        journal.add_op("resize", {"partition": "/dev/nvme0n1p1", "new_size_bytes": 5 * 1024**3})
        with patch.object(partition_ops, "list_partitions", return_value=[
            {"name": "/dev/nvme0n1p1", "start_bytes": 1024**2, "size_bytes": 10 * 1024**3, "fstype": "btrfs"},
            {"name": "/dev/nvme0n1p2", "start_bytes": 1024**2 + 10 * 1024**3, "size_bytes": 10 * 1024**3, "fstype": "ntfs"},
        ]), patch.object(partition_ops, "_parent_disk", return_value="/dev/nvme0n1"):
            errors = journal.validate()
        self.assertEqual(errors, [])


class JournalCommitResizeTests(unittest.TestCase):
    """Journal._commit_resize must shrink the filesystem before ever moving
    the partition boundary — parted's resizepart only moves the table entry
    and never touches filesystem metadata, so skipping the shrink corrupts
    whatever filesystem already lives on the partition."""

    def _journal(self):
        with patch.object(partition_ops, "list_partitions", return_value=[]):
            return partition_ops.Journal("/dev/nvme0n1")

    def test_shrinks_filesystem_before_moving_partition_boundary(self):
        journal = self._journal()
        journal._disk_service = MagicMock(dry_run=False)
        call_order = []
        mock_shrink = MagicMock(side_effect=lambda *a, **k: call_order.append("shrink"))
        journal._disk_service.resize_partition = MagicMock(
            side_effect=lambda *a, **k: call_order.append("resize_partition")
        )

        with patch.object(partition_ops, "shrink_filesystem", mock_shrink), \
             patch.object(partition_ops, "list_partitions", return_value=[
                 {"name": "/dev/nvme0n1p2", "fstype": "ntfs"},
             ]), \
             patch.object(partition_ops, "_partition_number", return_value=2), \
             patch.object(partition_ops, "_partition_start_bytes", return_value=1024**2):
            journal._commit_resize(
                {"partition": "/dev/nvme0n1p2", "new_size_bytes": 20 * 1024**3}, lambda _m: None
            )

        self.assertEqual(call_order, ["shrink", "resize_partition"])
        mock_shrink.assert_called_once_with("/dev/nvme0n1p2", "ntfs", 20 * 1024**3, unittest.mock.ANY)

    def test_rejects_a_partition_missing_from_the_current_disk_scan(self):
        journal = self._journal()
        journal._disk_service = MagicMock(dry_run=False)
        with patch.object(partition_ops, "list_partitions", return_value=[]):
            with self.assertRaisesRegex(RuntimeError, "was not found"):
                journal._commit_resize(
                    {"partition": "/dev/nvme0n1p2", "new_size_bytes": 20 * 1024**3}, lambda _m: None
                )

    def test_dry_run_skips_the_filesystem_shrink_entirely(self):
        journal = self._journal()
        journal._disk_service = MagicMock(dry_run=True)
        with patch.object(partition_ops, "shrink_filesystem") as mock_shrink, \
             patch.object(partition_ops, "_partition_number", return_value=99), \
             patch.object(partition_ops, "_partition_start_bytes", return_value=1024**2):
            journal._commit_resize(
                {"partition": "/dev/nvme0n1p2", "new_size_bytes": 20 * 1024**3}, lambda _m: None
            )
        mock_shrink.assert_not_called()
        journal._disk_service.resize_partition.assert_called_once()

    def test_unsupported_filesystem_propagates_and_never_touches_partition_table(self):
        journal = self._journal()
        journal._disk_service = MagicMock(dry_run=False)
        with patch.object(partition_ops, "list_partitions", return_value=[
                 {"name": "/dev/nvme0n1p2", "fstype": "xfs"},
             ]), \
             patch.object(partition_ops, "_partition_number", return_value=2), \
             patch.object(partition_ops, "_partition_start_bytes", return_value=1024**2):
            with self.assertRaisesRegex(RuntimeError, "not supported"):
                journal._commit_resize(
                    {"partition": "/dev/nvme0n1p2", "new_size_bytes": 20 * 1024**3}, lambda _m: None
                )
        journal._disk_service.resize_partition.assert_not_called()


class InstallerServerConfirmationTests(unittest.TestCase):
    """/api/start must re-check the review-page acknowledgement checkboxes
    server-side, not just trust the frontend to keep "Install Now" disabled."""

    def _make_handler(self, body: dict) -> server.Handler:
        handler = server.Handler.__new__(server.Handler)
        payload = json.dumps(body).encode()
        handler.headers = {"Content-Length": str(len(payload))}
        handler.rfile = io.BytesIO(payload)
        handler.wfile = io.BytesIO()
        handler.path = "/api/start"
        handler.send_response = MagicMock()
        handler.send_header = MagicMock()
        handler.end_headers = MagicMock()
        handler.send_error = MagicMock()
        handler.server = SimpleNamespace(context=InstallerContext())
        return handler

    @patch.object(server.Handler, "_require_same_origin_context", return_value=True)
    @patch.object(server.Handler, "_require_auth", return_value=True)
    def test_start_rejects_missing_confirmation_checkboxes(self, *_mocks):
        disks = [{"name": "/dev/sda", "current": False, "size_bytes": 64 * 1024**3}]
        handler = self._make_handler({
            "disk": "/dev/sda",
            "install_mode": "wipe",
            "confirm_backup": True,
            "confirm_erase": False,
        })
        with patch.object(disk, "list_disks", return_value=disks), \
             patch.object(plan, "_validate_storage_intent"), \
             patch.object(install, "_run_install") as run_install:
            handler.do_POST()

        handler.send_error.assert_not_called()
        written = handler.wfile.getvalue().decode().lower()
        self.assertIn('"started": false', written)
        run_install.assert_not_called()

    @patch.object(server.Handler, "_require_same_origin_context", return_value=True)
    @patch.object(server.Handler, "_require_auth", return_value=True)
    def test_start_accepts_when_confirmations_present(self, *_mocks):
        disks = [{"name": "/dev/sda", "current": False, "size_bytes": 64 * 1024**3}]
        handler = self._make_handler({
            "disk": "/dev/sda",
            "install_mode": "wipe",
            "username": "user",
            "password": "x",
            "confirm_backup": True,
            "confirm_erase": True,
        })
        with patch.object(disk, "list_disks", return_value=disks), \
             patch.object(plan, "_validate_storage_intent"), \
             patch.object(system, "list_timezones", return_value=["UTC"]), \
             patch.object(install, "_run_install"):
            handler.do_POST()

        handler.send_error.assert_not_called()
        written = handler.wfile.getvalue().decode().lower()
        self.assertIn('"started": true', written)
        self.assertEqual(handler.context.state["disk"], "/dev/sda")
        self.assertEqual(handler.context.state["install_mode"], "wipe")
        self.assertEqual(handler.context.state["username"], "user")
        self.assertEqual(handler.context.state["timezone"], "UTC")
        self.assertTrue(handler.context.state["password_hash"].startswith("$6$"))

    @patch.object(server.Handler, "_require_same_origin_context", return_value=True)
    @patch.object(server.Handler, "_require_auth", return_value=True)
    def test_start_rejects_empty_password(self, *_mocks):
        disks = [{"name": "/dev/sda", "current": False, "size_bytes": 64 * 1024**3}]
        handler = self._make_handler({
            "disk": "/dev/sda",
            "install_mode": "wipe",
            "username": "user",
            "password": "",
            "confirm_backup": True,
            "confirm_erase": True,
        })
        with patch.object(disk, "list_disks", return_value=disks), \
             patch.object(plan, "_validate_storage_intent"), \
             patch.object(system, "list_timezones", return_value=["UTC"]), \
             patch.object(install, "_run_install"):
            handler.do_POST()

        written = handler.wfile.getvalue().decode().lower()
        self.assertIn('"started": false', written)
        self.assertIn('could not hash password', written)


class InstallerSystemTests(unittest.TestCase):
    @patch.object(system, "run_command")
    def test_locales_and_keymaps_are_discovered_with_localectl(self, mock_run):
        mock_run.side_effect = [
            MagicMock(stdout="de_DE.UTF-8\nen_US.UTF-8\n"),
            MagicMock(stdout="de\nus\n"),
        ]
        self.assertEqual(system.list_locales(), ["de_DE.UTF-8", "en_US.UTF-8"])
        self.assertEqual(system.list_keymaps(), ["de", "us"])

    @patch("kyth_installer.system.subprocess.run")
    def test_hash_password_success(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stdout="$6$hashedpassword\n")
        res = system._hash_password("mypassword")
        self.assertEqual(res, "$6$hashedpassword")
        mock_run.assert_called_once()
        cmd = mock_run.call_args[0][0]
        self.assertEqual(cmd, ["openssl", "passwd", "-6", "-stdin"])
        self.assertEqual(mock_run.call_args[1].get("input"), "mypassword")

    def test_hash_password_empty_raises(self):
        with self.assertRaisesRegex(RuntimeError, "Password cannot be empty"):
            system._hash_password("")

    @patch("kyth_installer.system.subprocess.run")
    def test_hash_password_invalid_hash_raises(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stdout="invalidhash\n")
        with self.assertRaisesRegex(RuntimeError, "invalid SHA-512 crypt value"):
            system._hash_password("mypassword")

    def test_write_lines_creates_file_with_correct_permissions(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmpdir:
            test_file = Path(tmpdir) / "test_file"
            # Simulate the elevated process (launcher always runs as root).
            with patch.object(system.os, "geteuid", return_value=0):
                system._write_lines(test_file, ["line1", "line2"], 0o644)
            self.assertEqual(test_file.read_text(), "line1\nline2\n")
            mode = test_file.stat().st_mode & 0o777
            self.assertEqual(mode, 0o644)

    def test_write_lines_creates_sensitive_file_with_restricted_permissions(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmpdir:
            test_file = Path(tmpdir) / "sensitive_file"
            try:
                with patch.object(system.os, "geteuid", return_value=0):
                    system._write_lines(test_file, ["secret1", "secret2"], 0o000)
                mode = test_file.stat().st_mode & 0o777
                self.assertEqual(mode, 0o000)
                os.chmod(test_file, 0o600)
                self.assertEqual(test_file.read_text(), "secret1\nsecret2\n")
            finally:
                # Best-effort so TemporaryDirectory's own cleanup can still
                # delete the 0o000 file; nothing meaningful to log in a test.
                try:
                    os.chmod(test_file, 0o600)
                except Exception:  # noqa: S110
                    pass

    def test_write_lines_uses_elevated_mkdir_tee_chmod(self):
        """Account DB writes must go through _as_root, not bare open()."""
        import tempfile

        with tempfile.TemporaryDirectory() as tmpdir:
            test_file = Path(tmpdir) / "subdir" / "passwd"
            with patch.object(system, "run_command") as mock_run:
                mock_run.return_value = MagicMock(returncode=0, stdout="", stderr="")
                system._write_lines(test_file, ["user:x:1000:1000::/home/user:"], 0o644)

            cmds = [c.args[0] for c in mock_run.call_args_list]
            # First arg is the argv list (possibly with sudo -n prefix when non-root).
            flat = [" ".join(str(p) for p in cmd) for cmd in cmds]
            self.assertTrue(any(("mkdir" in s or "test" in s) and "subdir" in s for s in flat), flat)
            self.assertTrue(any("tee" in s and "passwd" in s for s in flat), flat)
            self.assertTrue(any("chmod" in s and "644" in s for s in flat), flat)

    @patch.object(system._accounts, "_write_lines")
    def test_write_lines_wraps_non_os_error_as_runtime_error(self, mock_write):
        mock_write.side_effect = ValueError("boom")
        with self.assertRaisesRegex(RuntimeError, r"Could not write .*shadow"):
            system._write_lines(Path("/target/etc/shadow"), ["root:!:::::::"], 0o000)

    def test_require_root_rejects_non_root(self):
        with patch.object(system.os, "geteuid", return_value=1000):
            with self.assertRaisesRegex(RuntimeError, "must run as root"):
                system.require_root()

    def test_require_root_accepts_root(self):
        with patch.object(system.os, "geteuid", return_value=0):
            system.require_root()  # does not raise

    def test_format_os_error_includes_path_and_errno(self):
        err = OSError(13, "Permission denied")
        err.filename = "/target/etc/shadow"
        text = system.format_os_error(err)
        self.assertIn("Permission denied", text)
        self.assertIn("path=/target/etc/shadow", text)
        self.assertIn("errno=13", text)
        self.assertIn("EACCES", text)

    def test_format_install_error_wraps_permission_error(self):
        err = PermissionError(30, "Read-only file system")
        err.filename = "/var/tmp/kyth-install-root"  # noqa: S108 — fixture string, not a real path opened on disk
        text = system.format_install_error(err)
        self.assertIn("PermissionError", text)
        self.assertIn("Read-only file system", text)
        self.assertIn("/var/tmp/kyth-install-root", text)  # noqa: S108 — fixture string, not a real path opened on disk

    def test_require_no_symlink_rejects_symlink(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmpdir:
            real = os.path.join(tmpdir, "real")
            link = os.path.join(tmpdir, "link")
            os.makedirs(real)
            os.symlink(real, link)
            with self.assertRaisesRegex(RuntimeError, "already exists as a symlink"):
                system._require_no_symlink(link)

    def test_require_no_symlink_allows_real_dir_and_missing_path(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmpdir:
            real = os.path.join(tmpdir, "real")
            os.makedirs(real)
            system._require_no_symlink(real)  # does not raise
            system._require_no_symlink(os.path.join(tmpdir, "does-not-exist"))  # does not raise

    def test_safe_umount_defaults_to_check_false_and_captures_output(self):
        mock_run = MagicMock(return_value=MagicMock(returncode=1))
        result = system._safe_umount(mock_run, "/mnt/target")
        argv = mock_run.call_args.args[0]
        self.assertIn("umount", argv)
        self.assertIn("-l", argv)
        self.assertIn("/mnt/target", argv)
        self.assertEqual(mock_run.call_args.kwargs.get("check"), False)
        self.assertEqual(mock_run.call_args.kwargs.get("capture_output"), True)
        self.assertIs(result, mock_run.return_value)

    def test_safe_umount_check_true_propagates_to_run(self):
        mock_run = MagicMock()
        system._safe_umount(mock_run, "/mnt/target", check=True)
        self.assertEqual(mock_run.call_args.kwargs.get("check"), True)

    @patch.object(system, "run_command")
    def test_settle_runs_partprobe_then_udevadm_settle(self, mock_run):
        system._settle()
        self.assertEqual(mock_run.call_count, 2)
        self.assertIn("partprobe", mock_run.call_args_list[0].args[0])
        self.assertEqual(mock_run.call_args_list[1].args[0], ["udevadm", "settle"])

    @patch.object(system, "run_command")
    def test_list_timezones_uses_timedatectl_when_available(self, mock_run):
        mock_run.return_value = MagicMock(stdout="America/New_York\nUTC\n")
        zones = system.list_timezones()
        self.assertEqual(zones, ["America/New_York", "UTC"])

    @patch.object(system, "run_command")
    def test_list_timezones_falls_back_to_zone_tab_when_timedatectl_fails(self, mock_run):
        mock_run.side_effect = RuntimeError("timedatectl not found")
        tab_data = "# comment\nUS\t+404251-0740023\tAmerica/New_York\tEastern\n"
        with patch("builtins.open", mock_open(read_data=tab_data)):
            zones = system.list_timezones()
        self.assertIn("America/New_York", zones)
        self.assertIn("UTC", zones)

    @patch.object(system, "run_command")
    def test_list_timezones_falls_back_to_utc_when_everything_fails(self, mock_run):
        mock_run.side_effect = RuntimeError("timedatectl not found")
        with patch("builtins.open", side_effect=OSError("no such file")):
            zones = system.list_timezones()
        self.assertEqual(zones, ["UTC"])

    def test_find_deploy_etc_returns_latest_sorted_candidate(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmpdir:
            base = Path(tmpdir) / "ostree/deploy/default/deploy"
            (base / "abc123.0" / "etc").mkdir(parents=True)
            (base / "def456.1" / "etc").mkdir(parents=True)
            result = system.find_deploy_etc(tmpdir)
            self.assertEqual(result, str(base / "def456.1" / "etc"))

    def test_find_deploy_etc_returns_none_when_missing(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmpdir:
            self.assertIsNone(system.find_deploy_etc(tmpdir))

    @patch.object(system._accounts, "_read_lines")
    def test_read_lines_wraps_os_error_with_path_context(self, mock_read):
        mock_read.side_effect = OSError(13, "Permission denied")
        with self.assertRaisesRegex(OSError, "path=/target/etc/passwd"):
            system._read_lines(Path("/target/etc/passwd"))

    @patch.object(system._accounts, "_chmod_path")
    def test_chmod_path_wraps_os_error_with_path_context(self, mock_chmod):
        mock_chmod.side_effect = OSError(13, "Permission denied")
        with self.assertRaisesRegex(OSError, "path=/target/etc/shadow"):
            system._chmod_path(Path("/target/etc/shadow"), 0o600)

    @patch.object(system._accounts, "_path_exists", return_value=True)
    def test_path_exists_delegates_to_accounts_module(self, mock_exists):
        self.assertTrue(system._path_exists(Path("/target/etc/passwd")))
        mock_exists.assert_called_once()

    @patch.object(system._accounts, "ensure_system_accounts")
    def test_ensure_system_accounts_wraps_os_error_with_path_context(self, mock_ensure):
        mock_ensure.side_effect = OSError(13, "Permission denied")
        with self.assertRaisesRegex(OSError, "path=/mnt/target"):
            system.ensure_system_accounts("/mnt/target", lambda _m: None)

    @patch.object(system._accounts, "ensure_system_accounts")
    def test_ensure_system_accounts_wraps_generic_exception_as_runtime_error(self, mock_ensure):
        mock_ensure.side_effect = ValueError("boom")
        with self.assertRaisesRegex(RuntimeError, "Could not repair system accounts under /mnt/target"):
            system.ensure_system_accounts("/mnt/target", lambda _m: None)

    @patch.object(system, "run_command")
    def test_lsblk_target_mounts_walks_children_and_sorts_deepest_first(self, mock_run):
        payload = {
            "blockdevices": [
                {
                    "name": "/dev/sda",
                    "mountpoints": [None],
                    "children": [
                        {"name": "/dev/sda1", "mountpoints": ["/boot/efi"]},
                        {"name": "/dev/sda2", "mountpoints": ["/mnt/target/home", "/mnt/target"]},
                    ],
                }
            ]
        }
        mock_run.return_value = MagicMock(stdout=json.dumps(payload))
        mounts = system._lsblk_target_mounts("/dev/sda")
        self.assertEqual(
            mounts,
            [
                ("/dev/sda2", "/mnt/target/home"),
                ("/dev/sda1", "/boot/efi"),
                ("/dev/sda2", "/mnt/target"),
            ],
        )

    @patch.object(system, "_lsblk_target_mounts")
    @patch.object(system, "run_command")
    def test_unmount_target_disk_succeeds_with_no_mounts(self, mock_run, mock_lsblk):
        mock_run.return_value = MagicMock(returncode=0, stdout="", stderr="")
        mock_lsblk.side_effect = [[], []]
        logs = []
        system.unmount_target_disk("/dev/sda", logs.append)
        self.assertTrue(any("Unmounting any existing mounts" in m for m in logs))

    @patch.object(system, "_safe_umount")
    @patch.object(system, "_lsblk_target_mounts")
    @patch.object(system, "run_command")
    def test_unmount_target_disk_skips_lazy_umount_for_critical_mount(self, mock_run, mock_lsblk, mock_safe_umount):
        mock_run.return_value = MagicMock(returncode=1, stdout="", stderr="target is busy")
        mock_lsblk.side_effect = [[("/dev/sda2", "/boot")], []]
        logs = []
        system.unmount_target_disk("/dev/sda", logs.append)
        mock_safe_umount.assert_not_called()
        self.assertTrue(any("Skipping lazy unmount" in m for m in logs))

    @patch.object(system, "_safe_umount")
    @patch.object(system, "_lsblk_target_mounts")
    @patch.object(system, "run_command")
    def test_unmount_target_disk_lazy_umounts_non_critical_mount(self, mock_run, mock_lsblk, mock_safe_umount):
        mock_run.return_value = MagicMock(returncode=1, stdout="", stderr="target is busy")
        mock_lsblk.side_effect = [[("/dev/sda3", "/mnt/target/home")], []]
        logs = []
        system.unmount_target_disk("/dev/sda", logs.append)
        mock_safe_umount.assert_called_once_with(mock_run, "/mnt/target/home")

    @patch.object(system, "_lsblk_target_mounts")
    @patch.object(system, "run_command")
    def test_unmount_target_disk_refuses_when_initial_scan_fails(self, mock_run, mock_lsblk):
        mock_run.return_value = MagicMock(returncode=0, stdout="", stderr="")
        mock_lsblk.side_effect = [RuntimeError("lsblk not found"), []]
        with self.assertRaisesRegex(RuntimeError, "Could not inspect mounts"):
            system.unmount_target_disk("/dev/sda", lambda _m: None)

    @patch.object(system, "_lsblk_target_mounts")
    @patch.object(system, "run_command")
    def test_unmount_target_disk_refuses_when_final_scan_fails(self, mock_run, mock_lsblk):
        mock_run.return_value = MagicMock(returncode=0, stdout="", stderr="")
        mock_lsblk.side_effect = [[], RuntimeError("lsblk not found")]
        with self.assertRaisesRegex(RuntimeError, "Could not verify"):
            system.unmount_target_disk("/dev/sda", lambda _m: None)

    @patch.object(system, "_lsblk_target_mounts")
    @patch.object(system, "run_command")
    def test_unmount_target_disk_raises_when_mounts_remain(self, mock_run, mock_lsblk):
        mock_run.return_value = MagicMock(returncode=0, stdout="", stderr="")
        mock_lsblk.side_effect = [[], [("/dev/sda2", "/mnt/target")]]
        with self.assertRaisesRegex(RuntimeError, "still has mounted partitions"):
            system.unmount_target_disk("/dev/sda", lambda _m: None)

    @patch.dict(os.environ, {"KYTH_STAGE_MOK": "0"})
    def test_mok_enrollment_skipped_for_non_cachy_kernel(self):
        logs = []
        result = system._try_stage_mok_enrollment(logs.append, kernel="fedora")
        self.assertEqual(result, "skipped")

    @patch.dict(os.environ, {"KYTH_STAGE_MOK": "0"})
    @patch.object(system, "Path")
    def test_mok_enrollment_skipped_when_cert_missing(self, mock_path_cls):
        mock_path_cls.return_value.exists.return_value = False
        result = system._try_stage_mok_enrollment(lambda _m: None, kernel="cachy")
        self.assertEqual(result, "skipped")

    @patch.dict(os.environ, {"KYTH_STAGE_MOK": "0"})
    @patch.object(system, "shutil")
    @patch.object(system, "Path")
    def test_mok_enrollment_skipped_when_mokutil_missing(self, mock_path_cls, mock_shutil):
        mock_path_cls.return_value.exists.return_value = True
        mock_shutil.which.return_value = None
        result = system._try_stage_mok_enrollment(lambda _m: None, kernel="cachy")
        self.assertEqual(result, "skipped")

    @patch.dict(os.environ, {"KYTH_STAGE_MOK": "0"})
    @patch.object(system, "run_command")
    @patch.object(system, "shutil")
    @patch.object(system, "Path")
    def test_mok_enrollment_skipped_when_secure_boot_disabled(self, mock_path_cls, mock_shutil, mock_run):
        mock_path_cls.return_value.exists.return_value = True
        mock_shutil.which.side_effect = lambda name: "/usr/bin/mokutil" if name == "mokutil" else None
        mock_run.return_value = MagicMock(stdout="SecureBoot disabled\n")
        result = system._try_stage_mok_enrollment(lambda _m: None, kernel="cachy")
        self.assertEqual(result, "skipped")
        mock_run.assert_called_once()

    @patch.dict(os.environ, {"KYTH_STAGE_MOK": "0"})
    @patch.object(system, "run_command")
    @patch.object(system, "shutil")
    @patch.object(system, "Path")
    def test_mok_enrollment_reports_already_enrolled(self, mock_path_cls, mock_shutil, mock_run):
        mock_path_cls.return_value.exists.return_value = True
        mock_shutil.which.side_effect = lambda name: "/usr/bin/mokutil" if name == "mokutil" else None
        mock_run.side_effect = [
            MagicMock(stdout="SecureBoot enabled\n"),
            MagicMock(stdout="KythOS Secure Boot\n"),
        ]
        result = system._try_stage_mok_enrollment(lambda _m: None, kernel="cachy")
        self.assertEqual(result, "enrolled")

    @patch.dict(os.environ, {"KYTH_STAGE_MOK": "0"})
    @patch.object(system, "run_command")
    @patch.object(system, "shutil")
    @patch.object(system, "Path")
    def test_mok_enrollment_reports_already_pending(self, mock_path_cls, mock_shutil, mock_run):
        mock_path_cls.return_value.exists.return_value = True
        mock_shutil.which.side_effect = lambda name: "/usr/bin/mokutil" if name == "mokutil" else None
        mock_run.side_effect = [
            MagicMock(stdout="SecureBoot enabled\n"),
            MagicMock(stdout="no keys enrolled\n"),
            MagicMock(stdout="KythOS Secure Boot\n"),
        ]
        result = system._try_stage_mok_enrollment(lambda _m: None, kernel="cachy")
        self.assertEqual(result, "pending")

    @patch.dict(os.environ, {"KYTH_STAGE_MOK": "0"})
    @patch.object(system, "run_command")
    @patch.object(system, "shutil")
    @patch.object(system, "Path")
    def test_mok_enrollment_stages_successfully(self, mock_path_cls, mock_shutil, mock_run):
        mock_path_cls.return_value.exists.return_value = True
        mock_shutil.which.side_effect = lambda name: "/usr/bin/mokutil" if name == "mokutil" else None
        mock_run.side_effect = [
            MagicMock(stdout="SecureBoot enabled\n"),
            MagicMock(stdout="no keys enrolled\n"),
            MagicMock(stdout="no keys pending\n"),
            MagicMock(returncode=0),
        ]
        result = system._try_stage_mok_enrollment(lambda _m: None, kernel="cachy", mok_password="hunter2")
        self.assertEqual(result, "staged")
        self.assertEqual(mock_run.call_args_list[-1].kwargs.get("input"), "hunter2\n")

    @patch.dict(os.environ, {"KYTH_STAGE_MOK": "0"})
    @patch.object(system, "run_command")
    @patch.object(system, "shutil")
    @patch.object(system, "Path")
    def test_mok_enrollment_reports_failed_import(self, mock_path_cls, mock_shutil, mock_run):
        mock_path_cls.return_value.exists.return_value = True
        mock_shutil.which.side_effect = lambda name: "/usr/bin/mokutil" if name == "mokutil" else None
        mock_run.side_effect = [
            MagicMock(stdout="SecureBoot enabled\n"),
            MagicMock(stdout="no keys enrolled\n"),
            MagicMock(stdout="no keys pending\n"),
            MagicMock(returncode=1, stderr="import error"),
        ]
        result = system._try_stage_mok_enrollment(lambda _m: None, kernel="cachy")
        self.assertEqual(result, "failed")


    @patch.object(system, "_as_root", side_effect=lambda argv: argv)
    @patch.object(system, "run_command")
    @patch.object(system, "shutil")
    def test_mok_enrollment_native_helper_stages_via_stdin_password(self, mock_shutil, mock_run, mock_as_root):
        mock_shutil.which.return_value = "/usr/bin/kyth-installer-exec"
        mock_run.return_value = MagicMock(
            stdout=json.dumps({"state": "staged", "message": "enrollment staged"})
        )
        logs = []
        result = system._try_stage_mok_enrollment(logs.append, kernel="fedora", mok_password="hunter2")
        self.assertEqual(result, "staged")
        self.assertIn("enrollment staged", logs[0])
        self.assertEqual(
            mock_run.call_args.args[0],
            ["kyth-installer-exec", "--operation", "secure-boot-stage"],
        )
        payload = json.loads(mock_run.call_args.kwargs["input"])
        self.assertEqual(payload["password"], "hunter2")
        self.assertNotIn("hunter2", " ".join(mock_run.call_args.args[0]))

    @patch.object(system, "_as_root", side_effect=lambda argv: argv)
    @patch.object(system, "run_command")
    @patch.object(system, "shutil")
    def test_mok_enrollment_native_helper_rejects_malformed_response(self, mock_shutil, mock_run, mock_as_root):
        mock_shutil.which.return_value = "/usr/bin/kyth-installer-exec"
        mock_run.return_value = MagicMock(stdout=json.dumps({"state": "staged"}))
        result = system._try_stage_mok_enrollment(lambda _m: None, kernel="fedora")
        self.assertEqual(result, "failed")

    @patch.object(system, "_as_root", side_effect=lambda argv: argv)
    @patch.object(system, "run_command")
    @patch.object(system, "shutil")
    def test_mok_enrollment_native_helper_failure_is_caught(self, mock_shutil, mock_run, mock_as_root):
        mock_shutil.which.return_value = "/usr/bin/kyth-installer-exec"
        mock_run.side_effect = RuntimeError("helper crashed")
        logs = []
        result = system._try_stage_mok_enrollment(logs.append, kernel="fedora")
        self.assertEqual(result, "failed")
        self.assertIn("helper crashed", logs[0])


class InstallerGptDiskTests(unittest.TestCase):
    @patch("kyth_installer.plan.run_command")
    def test_is_gpt_disk_via_blkid(self, mock_run):
        mock_run.return_value = SimpleNamespace(stdout="gpt\n", returncode=0)
        self.assertTrue(plan._is_gpt_disk("/dev/sda"))
        mock_run.assert_called_once_with(
            ["blkid", "-o", "value", "-s", "PTTYPE", "/dev/sda"],
            capture_output=True, text=True, check=True, timeout=5,
        )

    @patch("kyth_installer.plan.run_command")
    def test_is_gpt_disk_via_parted(self, mock_run):
        mock_run.side_effect = [
            RuntimeError("blkid failed"),
            SimpleNamespace(stdout="Model: Virtual Disk\nPartition Table: gpt\n", returncode=0),
        ]
        self.assertTrue(plan._is_gpt_disk("/dev/sda"))
        self.assertEqual(mock_run.call_count, 2)

    @patch("kyth_installer.plan.run_command")
    def test_is_gpt_disk_non_gpt(self, mock_run):
        mock_run.return_value = SimpleNamespace(stdout="dos\n", returncode=0)
        self.assertFalse(plan._is_gpt_disk("/dev/sda"))


class InstallerDiskServiceTests(unittest.TestCase):
    def test_partition_table_restore_is_checked(self):
        from kyth_installer.services.disk_service import DiskService
        svc = DiskService()
        with patch.object(svc, "execute") as execute, patch.object(svc, "settle"):
            svc.restore_table("/dev/sda", "/tmp/table.backup")

        self.assertTrue(execute.call_args.kwargs["check"])
        self.assertEqual(
            json.loads(execute.call_args.kwargs["input"]),
            {
                "operation": "restore_table",
                "disk": "/dev/sda",
                "backup_path": "/tmp/table.backup",
            },
        )

    def test_live_disk_operations_use_typed_rust_payloads(self):
        from kyth_installer.services.disk_service import DiskService

        svc = DiskService()
        with patch.object(svc, "execute") as execute, \
             patch.object(svc, "settle"), \
             patch("kyth_installer.disk._block_size_bytes", return_value=512), \
             patch("kyth_installer.services.disk_service.shutil.which", return_value="/usr/sbin/parted"), \
             patch("kyth_installer.partition_ops._require_mkfs"):
            svc.create_label("/dev/sda", "gpt")
            svc.create_partition("/dev/sda", 1024 * 1024, 1024 * 1024, "btrfs", "ROOT")
            svc.create_unformatted_partition("/dev/sda", 2 * 1024 * 1024, 1024 * 1024, "biosboot")
            svc.delete_partition("/dev/sda", 1)
            svc.set_partition_flag("/dev/sda", 1, "esp")
            svc.resize_partition("/dev/sda", 2, 2 * 1024 * 1024, 1024 * 1024)
            svc.format_filesystem("/dev/sda1", "ext4", "DATA")

        payloads = [json.loads(call.kwargs["input"]) for call in execute.call_args_list]
        self.assertEqual(
            [payload["operation"] for payload in payloads],
            [
                "create_label",
                "create_partition",
                "create_unformatted_partition",
                "delete_partition",
                "set_partition_flag",
                "resize_partition",
                "format_filesystem",
            ],
        )
        self.assertEqual(payloads[1]["sector_size"], 512)
        self.assertEqual(payloads[-1]["device"], "/dev/sda1")

    def test_live_image_installs_partition_backup_tool(self):
        build_script = (ROOT / "installer/build.sh").read_text()
        self.assertRegex(build_script, r"dnf5? install -y[^\n]*\bgdisk\b")

    def test_disk_service_dry_run_collects_journal(self):
        from kyth_installer.services.disk_service import DiskService
        svc = DiskService(dry_run=True)
        svc.create_label("/dev/sda", "gpt")
        svc.create_partition("/dev/sda", 1024**2, 10 * 1024**3, "btrfs", "KythOS")
        svc.delete_partition("/dev/sda", 1)
        svc.resize_partition("/dev/sda", 2, 1024**2, 20 * 1024**3)
        svc.format_filesystem("/dev/sda2", "ext4", "mydata")

        # Verify command journal
        self.assertEqual(len(svc.journal), 5)
        self.assertIn("mklabel gpt", " ".join(svc.journal[0]))
        self.assertIn("mkpart KythOS btrfs", " ".join(svc.journal[1]))
        self.assertIn("rm 1", " ".join(svc.journal[2]))
        self.assertIn("resizepart 2", " ".join(svc.journal[3]))
        self.assertIn("mkfs.ext4", " ".join(svc.journal[4]))

    def test_disk_service_can_mark_an_efi_partition(self):
        from kyth_installer.services.disk_service import DiskService
        svc = DiskService(dry_run=True)
        svc.set_partition_flag("/dev/sda", 1, "esp")
        self.assertEqual(svc.journal[-1][-7:], ["parted", "-s", "/dev/sda", "set", "1", "esp", "on"])

    @patch.dict("os.environ", {"KYTH_INSTALL_ALLOW_NO_DISK_LOCK": "1"}, clear=False)
    def test_journal_with_dry_run_disk_service_executes_safely(self):
        from kyth_installer.services.disk_service import DiskService
        svc = DiskService(dry_run=True)
        # Mock normal device path and disk block size
        with patch("kyth_installer.partition_ops._normal_device_path", return_value="/dev/sda"):
            journal = partition_ops.Journal("/dev/sda", disk_service=svc)
            journal.add_op("new_table", {"table_type": "gpt"})
            journal.add_op("create", {
                "start_bytes": 2 * 1024**2, "size_bytes": 40 * 1024**3, "fs_type": "btrfs", "mountpoint": "/",
            })
            # Verify we can validate and commit without throwing any command execution errors
            errors = journal.validate()
            self.assertEqual(errors, [])
            root_part = journal.commit(lambda _msg: None)
            self.assertEqual(root_part, "/dev/sdap99")
            self.assertGreater(len(svc.journal), 0)


if __name__ == "__main__":
    unittest.main()
