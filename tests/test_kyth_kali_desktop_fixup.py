"""Behavioral tests for the Kali desktop fixup.

The fixup logic is the single shared implementation of "fix up
Kali-exported .desktop launchers for the host menu" — used by both the
`ujust setup-kali-box`/`export-kali-apps` CLI recipes and the System Hub
Security page's GUI install/export flow. The installed entry point is the
native `kyth-kali-desktop-fixup` binary (see `system::desktop_shortcuts`
and `kali_fixup_bin`); these tests pin the behavior contract against the
retained Python fixture (`kyth_shared.desktop.shortcut`) with a temp
$HOME, plus the native packaging contract.
"""
from __future__ import annotations

import pathlib
import tempfile
import unittest
from unittest import mock

from kyth_shared.desktop.shortcut import fixup_kali_desktop_launchers

ROOT = pathlib.Path(__file__).resolve().parents[1]


def _write_desktop(path: pathlib.Path, body: str) -> None:
    path.write_text(body, encoding="utf-8")


def _run_fixup(home: pathlib.Path) -> bool:
    with mock.patch.object(pathlib.Path, "home", return_value=home):
        return fixup_kali_desktop_launchers()


def _apps_dir(home: pathlib.Path) -> pathlib.Path:
    apps = home / ".local" / "share" / "applications"
    apps.mkdir(parents=True, exist_ok=True)
    return apps


class KaliDesktopFixupTests(unittest.TestCase):
    def test_native_binary_owns_the_installed_entry_point(self):
        # No Python launcher remains; the Rust crate builds and ships it.
        self.assertFalse((ROOT / "build_files" / "kyth-kali-desktop-fixup").exists())
        cargo = (ROOT / "src" / "kyth-shared-rs" / "Cargo.toml").read_text(encoding="utf-8")
        self.assertIn('name = "kyth-kali-desktop-fixup"', cargo)

    def test_patches_categories_and_strips_display_hints_on_kali_entries(self):
        with tempfile.TemporaryDirectory() as home_dir:
            home = pathlib.Path(home_dir)
            apps = _apps_dir(home)
            entry = apps / "kali-nmap.desktop"
            _write_desktop(
                entry,
                "[Desktop Entry]\n"
                "Name=nmap\n"
                "Exec=distrobox-enter --name kali -- nmap\n"
                "Categories=Network;\n"
                "NoDisplay=true\n"
                "OnlyShowIn=GNOME;\n"
                "NotShowIn=KDE;\n",
            )

            self.assertTrue(_run_fixup(home))

            text = entry.read_text()
            self.assertIn("Categories=X-KythSecurity;", text)
            self.assertNotIn("NoDisplay=true", text)
            self.assertNotIn("OnlyShowIn=", text)
            self.assertNotIn("NotShowIn=", text)

    def test_rewrites_pkexec_escalation_to_sudo(self):
        with tempfile.TemporaryDirectory() as home_dir:
            home = pathlib.Path(home_dir)
            apps = _apps_dir(home)
            entry = apps / "kali-wireshark.desktop"
            _write_desktop(
                entry,
                "[Desktop Entry]\n"
                "Name=Wireshark\n"
                "Exec=distrobox-enter --name kali -- pkexec wireshark\n"
                "Categories=Network;\n",
            )

            self.assertTrue(_run_fixup(home))

            text = entry.read_text()
            self.assertIn("sudo -E wireshark", text)
            self.assertNotIn("pkexec", text)

    def test_ignores_entries_not_belonging_to_kali(self):
        with tempfile.TemporaryDirectory() as home_dir:
            home = pathlib.Path(home_dir)
            apps = _apps_dir(home)
            entry = apps / "firefox.desktop"
            original = (
                "[Desktop Entry]\n"
                "Name=Firefox\n"
                "Exec=firefox %u\n"
                "Categories=Network;\n"
                "NoDisplay=true\n"
            )
            _write_desktop(entry, original)

            self.assertFalse(_run_fixup(home))
            self.assertEqual(entry.read_text(), original)

    def test_repoints_zenmap_root_launcher_through_distrobox_root_launch(self):
        with tempfile.TemporaryDirectory() as home_dir:
            home = pathlib.Path(home_dir)
            apps = _apps_dir(home)
            entry = apps / "kali-zenmap-root.desktop"
            _write_desktop(
                entry,
                "[Desktop Entry]\n"
                "Name=Zenmap (as root)\n"
                "Exec=distrobox-enter --name kali -- zenmap\n"
                "TryExec=distrobox-enter\n"
                "Categories=Network;\n",
            )

            self.assertTrue(_run_fixup(home))

            text = entry.read_text()
            self.assertIn(
                "Exec=kyth-distrobox-root-launch --root kali /usr/bin/zenmap", text,
            )
            self.assertIn("TryExec=kyth-distrobox-root-launch", text)

    def test_no_desktop_files_is_a_noop(self):
        with tempfile.TemporaryDirectory() as home_dir:
            home = pathlib.Path(home_dir)
            _apps_dir(home)
            self.assertFalse(_run_fixup(home))


if __name__ == "__main__":
    unittest.main()
