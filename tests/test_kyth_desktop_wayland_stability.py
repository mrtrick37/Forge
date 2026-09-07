"""OS-level Plasma / Wayland / desktop stack stability (not System Hub)."""
from __future__ import annotations

import pathlib
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "build_files" / "kyth_shared"))

from kyth_shared import plasma_drift as drift_mod  # noqa: E402
from kyth_shared import pipewire_latency as pw_mod  # noqa: E402
from kyth_shared.system import desktop_stack as stack_mod  # noqa: E402
from kyth_shared.system import plasma_hdr as hdr_mod  # noqa: E402


class PlasmaHdrTests(unittest.TestCase):
    def test_unknown_preset_rejected(self):
        ok, msg = hdr_mod.apply_preset("nope")
        self.assertFalse(ok)
        self.assertIn("unknown", msg)

    def test_dry_run(self):
        ok, msg = hdr_mod.apply_preset("vrr", dry_run=True)
        self.assertTrue(ok)
        self.assertIn("dry-run", msg)

    def test_vrr_writes_wayland_vrrpolicy_and_rolls_back_on_failure(self):
        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            kwinrc = home / "kwinrc"
            kwinrc.write_text("[Wayland]\nVrrPolicy=1\n", encoding="utf-8")
            with mock.patch.dict("os.environ", {"XDG_CONFIG_HOME": str(home)}), mock.patch.object(
                hdr_mod, "_kwriteconfig_bin", return_value="/bin/kwriteconfig6"
            ), mock.patch.object(hdr_mod, "_reconfigure_kwin"), mock.patch.object(
                hdr_mod,
                "_run",
                side_effect=RuntimeError("boom"),
            ):
                ok, msg = hdr_mod.apply_preset("vrr_off")
            self.assertFalse(ok)
            self.assertIn("boom", msg)
            self.assertEqual(kwinrc.read_text(encoding="utf-8"), "[Wayland]\nVrrPolicy=1\n")

    def test_apply_uses_section_keys_via_kwriteconfig(self):
        calls: list[list[str]] = []

        def fake_run(args, **_kwargs):
            calls.append(list(args))
            return SimpleNamespace(returncode=0, stdout="", stderr="")

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            with mock.patch.dict("os.environ", {"XDG_CONFIG_HOME": str(home), "XDG_SESSION_TYPE": "x11"}), mock.patch.object(
                hdr_mod, "_kwriteconfig_bin", return_value="kwriteconfig6"
            ), mock.patch.object(hdr_mod, "_run", side_effect=fake_run), mock.patch.object(
                hdr_mod, "_reconfigure_kwin"
            ), mock.patch.object(hdr_mod.shutil, "which", return_value=None):
                ok, msg = hdr_mod.apply_preset("vrr")
            self.assertTrue(ok)
            self.assertIn("Wayland.VrrPolicy=1", msg)
            self.assertTrue(any("--group" in c and "Wayland" in c and "VrrPolicy" in c for c in calls))

    def test_preset_status_is_section_aware(self):
        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            (home / "kwinrc").write_text(
                "[Compositing]\nVrrPolicy=1\n[Wayland]\nVrrPolicy=0\n",
                encoding="utf-8",
            )
            with mock.patch.dict("os.environ", {"XDG_CONFIG_HOME": str(home)}):
                # vrr wants Wayland VrrPolicy=1 — Compositing value must not count.
                self.assertIn("not active", hdr_mod.preset_status("vrr"))
                (home / "kwinrc").write_text("[Wayland]\nVrrPolicy=1\n", encoding="utf-8")
                self.assertEqual(hdr_mod.preset_status("vrr"), "active")


class PlasmaDriftTests(unittest.TestCase):
    def test_flatten_nested_toml_sections(self):
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "plasma.toml"
            p.write_text(
                '[kwinrc.Compositing]\nAllowTearing = "false"\n'
                '[plasmarc]\nTheme = "kyth-dark"\n',
                encoding="utf-8",
            )
            loaded = drift_mod.load_plasma(p)
            self.assertEqual(loaded["kwinrc.Compositing"]["AllowTearing"], "false")
            self.assertEqual(loaded["plasmarc"]["Theme"], "kyth-dark")

    def test_parse_section_defaults_to_general(self):
        conf, groups = drift_mod._parse_section("kwinrc")
        self.assertEqual(conf, "kwinrc")
        self.assertEqual(groups, ["General"])
        conf, groups = drift_mod._parse_section("kwinrc.Compositing")
        self.assertEqual(groups, ["Compositing"])

    def test_apply_uses_nested_groups_and_kwriteconfig6(self):
        calls: list[list[str]] = []

        def fake_run(args, **_kwargs):
            calls.append(list(args))
            return SimpleNamespace(returncode=0, stdout="", stderr="")

        with mock.patch.object(drift_mod, "_kwriteconfig_bin", return_value="kwriteconfig6"), mock.patch.object(
            drift_mod, "run", side_effect=fake_run
        ), mock.patch.object(drift_mod, "_reconfigure_kwin"), mock.patch.object(
            drift_mod, "_atomic_write_text"
        ):
            applied = drift_mod.apply_plasma({"kwinrc.Compositing": {"AllowTearing": "false"}})
        self.assertEqual(applied, ["kwinrc.Compositing:AllowTearing=false"])
        self.assertEqual(
            calls[0],
            [
                "kwriteconfig6",
                "--file",
                "kwinrc",
                "--group",
                "Compositing",
                "--key",
                "AllowTearing",
                "false",
            ],
        )


class PipewireLatencyApplyTests(unittest.TestCase):
    def test_apply_writes_real_quantum_dropin_and_env_map(self):
        with tempfile.TemporaryDirectory() as td:
            td_path = Path(td)
            dropin = td_path / "pipewire.conf.d" / "99-kyth-latency.conf"
            env_map = td_path / "pipewire-latency.env"
            notes = pw_mod.apply_pipewire_latency(
                {"default": 128, "game.exe": 64},
                quantum_dropin=dropin,
                env_map=env_map,
            )
            self.assertTrue(dropin.exists())
            body = dropin.read_text(encoding="utf-8")
            self.assertIn("default.clock.quantum = 128", body)
            self.assertNotIn("-- ", body)  # must not be comment-only
            env_body = env_map.read_text(encoding="utf-8")
            self.assertIn("game.exe=PIPEWIRE_LATENCY=64/48000", env_body)
            self.assertTrue(any("quantum=128" in n for n in notes))


class DesktopStackTests(unittest.TestCase):
    def test_greeter_context_skips_user_units(self):
        checks = stack_mod.desktop_stack_checks(
            has_session_bus=lambda: False,
            path_exists=lambda p: p.endswith("xdg-desktop-portal"),
            which=lambda _n: None,
        )
        names = {c.name for c in checks}
        self.assertIn("Portal packages", names)
        self.assertIn("User desktop session", names)
        self.assertTrue(all(c.passed for c in checks if c.name == "User desktop session"))
        self.assertNotIn("PipeWire", names)

    def test_wayland_session_reports_missing_portal_unit(self):
        checks = stack_mod.desktop_stack_checks(
            has_session_bus=lambda: True,
            session_type=lambda: "wayland",
            wayland_display=lambda: "wayland-0",
            user_unit_active=lambda unit: unit in {"pipewire.service", "wireplumber.service"},
            path_exists=lambda _p: True,
            which=lambda _n: "/usr/bin/xdg-desktop-portal",
        )
        by_name = {c.name: c for c in checks}
        self.assertTrue(by_name["Wayland display"].passed)
        self.assertFalse(by_name["xdg-desktop-portal"].passed)
        self.assertTrue(by_name["xdg-desktop-portal"].advisory)
        self.assertTrue(by_name["PipeWire"].passed)

    def test_x11_session_is_unsupported(self):
        checks = stack_mod.desktop_stack_checks(
            has_session_bus=lambda: True,
            session_type=lambda: "x11",
            wayland_display=lambda: "",
            user_unit_active=lambda _unit: False,
            path_exists=lambda _p: True,
            which=lambda _n: "/usr/bin/xdg-desktop-portal",
        )
        by_name = {c.name: c for c in checks}
        self.assertFalse(by_name["Wayland display"].passed)
        self.assertIn("Plasma Wayland only", by_name["Wayland display"].detail)

    def test_packages_script_lists_portal_rpms(self):
        body = (
            ROOT / "build_files/scripts/packages/18-desktop-helper-and-creator-tooling.sh"
        ).read_text(encoding="utf-8")
        self.assertIn("xdg-desktop-portal-kde", body)
        self.assertIn("xdg-desktop-portal", body)

    def test_docs_describe_wayland_bare_metal_default(self):
        body = (ROOT / "docs/plasma-wayland-polish.md").read_text(encoding="utf-8")
        self.assertIn("Bare metal", body)
        self.assertIn("Wayland", body)
        self.assertIn("plasma.desktop", body)
        self.assertNotIn("intentionally starts Plasma X11", body)
        self.assertNotIn("plasmax11", body)
        self.assertIn("Ctrl+Alt+F3", body)
        self.assertIn("journalctl -u plasmalogin", body)


class VrrApplyTests(unittest.TestCase):
    def test_apply_writes_vrrpolicy_and_nightcolor(self):
        from kyth_shared import vrr as vrr_mod

        calls: list[list[str]] = []

        def fake_run(args, **_kwargs):
            calls.append(list(args))
            return SimpleNamespace(returncode=0, stdout="", stderr="")

        with mock.patch.object(vrr_mod, "_kwriteconfig_bin", return_value="kwriteconfig6"), mock.patch.object(
            vrr_mod, "run", side_effect=fake_run
        ), mock.patch.object(vrr_mod, "_reconfigure_kwin"), mock.patch.object(
            vrr_mod, "_atomic_write_text"
        ), mock.patch.object(vrr_mod.shutil, "which", return_value=None):
            notes = vrr_mod.apply_vrr(
                {
                    "outputs": {"DP-1": {"vrr": "always"}},
                    "night": {"enabled": True, "temperature": 4200},
                }
            )
        self.assertTrue(any("VrrPolicy=2" in n for n in notes))
        self.assertTrue(any("NightColor.Active=True" in n for n in notes))
        self.assertTrue(any("NightTemperature=4200" in n for n in notes))
        groups_keys = {(c[c.index("--group") + 1], c[c.index("--key") + 1]) for c in calls if "--group" in c}
        self.assertIn(("Wayland", "VrrPolicy"), groups_keys)
        self.assertIn(("NightColor", "NightTemperature"), groups_keys)


class ScalingApplyTests(unittest.TestCase):
    def test_apply_scaling_calls_kscreen_doctor(self):
        from kyth_shared import scaling as scaling_mod

        calls: list[list[str]] = []

        def fake_run(args, **_kwargs):
            calls.append(list(args))
            if args[:2] == ["kscreen-doctor", "-o"]:
                return SimpleNamespace(
                    returncode=0,
                    stdout="Output: 1 DP-1\nconnected\nenabled\n",
                    stderr="",
                )
            return SimpleNamespace(returncode=0, stdout="", stderr="")

        with mock.patch.object(scaling_mod.shutil, "which", return_value="/usr/bin/kscreen-doctor"), mock.patch.object(
            scaling_mod, "run", side_effect=fake_run
        ), mock.patch.object(scaling_mod, "_atomic_write_text"):
            notes = scaling_mod.apply_scaling({"DP-1": {"scale": 1.25, "icc": ""}})
        self.assertTrue(any("DP-1.scale=1.25" in n for n in notes))
        self.assertTrue(any(c == ["kscreen-doctor", "output.DP-1.scale.1.25"] for c in calls))


class DisplayHdrApplyTests(unittest.TestCase):
    def test_apply_display_hdr_enables_with_sdr_brightness(self):
        from kyth_shared import display_hdr as hdr_mod

        calls: list[list[str]] = []

        def fake_run(args, **_kwargs):
            calls.append(list(args))
            if args[:2] == ["kscreen-doctor", "-o"]:
                return SimpleNamespace(
                    returncode=0,
                    stdout="Output: 1 HDMI-A-1\nconnected\nenabled\n",
                    stderr="",
                )
            return SimpleNamespace(returncode=0, stdout="", stderr="")

        with mock.patch.dict("os.environ", {"XDG_SESSION_TYPE": "wayland"}), mock.patch.object(
            hdr_mod.shutil, "which", return_value="/usr/bin/kscreen-doctor"
        ), mock.patch.object(hdr_mod, "run", side_effect=fake_run):
            notes = hdr_mod.apply_display_hdr(
                {"HDMI-A-1": {"hdr_enabled": True, "sdr_nits": 250, "peak_nits": 600}}
            )
        self.assertTrue(any("HDMI-A-1.hdr.enable" in n and "sdr=250" in n for n in notes))
        self.assertTrue(
            any(
                "output.HDMI-A-1.hdr.enable" in c
                and "output.HDMI-A-1.wcg.enable" in c
                and "output.HDMI-A-1.sdr-brightness.250" in c
                for c in calls
            )
        )


class KwinLatencyTests(unittest.TestCase):
    def test_gaming_dropin_omits_latency_policy(self):
        from kyth_shared import kwin_latency as kl

        with tempfile.TemporaryDirectory() as td:
            dropin = Path(td) / "99-kyth-latency.conf"
            env = Path(td) / "99-kyth-kwin.conf"
            kl.generate_kwin_latency({"profile": "gaming", "tearing": True}, dropin=dropin, env=env)
            body = dropin.read_text(encoding="utf-8")
            self.assertIn("AllowTearing=true", body)
            self.assertNotIn("LatencyPolicy", body)


class BrandingInstallTests(unittest.TestCase):
    def test_vrr_and_scaling_branding_install_apply_binaries(self):
        vrr = (ROOT / "build_files/scripts/branding/47-vrr-night.sh").read_text(encoding="utf-8")
        scaling = (ROOT / "build_files/scripts/branding/41-scaling-color.sh").read_text(encoding="utf-8")
        self.assertIn("kyth-apply-vrr", vrr)
        self.assertIn("kyth-apply-scaling", scaling)
        self.assertIn("kyth-apply-display-hdr", scaling)
        self.assertTrue((ROOT / "build_files/kyth-apply-vrr").is_file())
        # Native ports: the binaries come from the Rust crate, no Python launchers.
        self.assertFalse((ROOT / "build_files/kyth-apply-scaling").exists())
        self.assertFalse((ROOT / "build_files/kyth-apply-display-hdr").exists())
        cargo = (ROOT / "src/kyth-shared-rs/Cargo.toml").read_text()
        self.assertIn('name = "kyth-apply-display-hdr"', cargo)
        self.assertIn('name = "kyth-apply-scaling"', cargo)


if __name__ == "__main__":
    unittest.main()
