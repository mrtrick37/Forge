"""Hub deep links — every --page key that ships must resolve.

kyth-welcome-launch forwards `--page KEY` unchanged to the Tauri Hub. Nothing in
that chain validates the key: an unknown one falls back to "/" and silently
opens Home instead of the requested page. The shared route manifest prevents
the packaging-time KRunner entries and the React deep-link table from drifting.

deepLink.ts derives its table from data/destinations.ts, which in turn
lists the section arrays from hubSections.ts, so checking the data source
covers every emitted key is enough — and it stays stable across refactors
of the mapping code itself, which parsing the TS logic would not.
"""
from __future__ import annotations

import json
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
HUB_WEB = ROOT / "src" / "kyth-hub-web" / "src"
ROUTE_MANIFEST = HUB_WEB / "data" / "hubRoutes.json"
HUB_ROUTES = json.loads(ROUTE_MANIFEST.read_text(encoding="utf-8"))
GENERATOR_RS = ROOT / "src" / "kyth-shared-rs" / "src" / "hub_desktop_entries_bin.rs"
CARGO = (ROOT / "src" / "kyth-shared-rs" / "Cargo.toml").read_text(encoding="utf-8")
DEEP_LINK_TS = (HUB_WEB / "deepLink.ts").read_text(encoding="utf-8")
ACCEPTANCE_TS = (HUB_WEB / "services" / "acceptance.ts").read_text(encoding="utf-8")
DESTINATIONS_TS = (HUB_WEB / "data" / "destinations.ts").read_text(encoding="utf-8")
SIDEBAR_TSX = (HUB_WEB / "components" / "Sidebar.tsx").read_text(encoding="utf-8")
HUB_PAGE_TSX = (HUB_WEB / "pages" / "HubPage.tsx").read_text(encoding="utf-8")
LAUNCHER_RS = (ROOT / "src" / "kyth-shared-rs" / "src" / "hub_launcher_bin.rs").read_text(encoding="utf-8")
CTX_INSTALLS_SH = (
    ROOT / "build_files" / "scripts" / "branding" / "23-kyth-helper-ctx-installs.sh"
).read_text(encoding="utf-8")

_PAGE_ARG_RE = re.compile(r'--page "([^"]+)"')


def _code_only(source: str) -> str:
    """Drop comments so assertions match real code, not prose about it —
    these files document the contract in comments that quote the very
    identifiers and keys being asserted on."""
    source = re.sub(r"/\*.*?\*/", "", source, flags=re.S)
    return "\n".join(
        line for line in source.splitlines() if not line.lstrip().startswith(("//", "*"))
    )


DEEP_LINK_CODE = _code_only(DEEP_LINK_TS)
DESTINATIONS_CODE = _code_only(DESTINATIONS_TS)
HUB_PAGE_CODE = _code_only(HUB_PAGE_TSX)


def _section_keys() -> set[str]:
    return {
        section["key"]
        for destination in HUB_ROUTES["destinations"]
        for section in destination["sections"]
    }


def _destination_keys() -> set[str]:
    """Rail destinations from the shared DESTINATIONS table literal.

    The table moved to data/destinations.ts when the search box needed the
    same "what pages exist and where" list deep links use; Welcome is still
    seeded by deepLink.ts's own route table, since it is a route with no
    sections rather than a destination.
    """
    keys = {destination["key"] for destination in HUB_ROUTES["destinations"]}
    if re.search(r'\{\s*Welcome:\s*"/"', DEEP_LINK_CODE):
        keys.add("Welcome")
    return keys


def _resolvable_keys() -> set[str]:
    return _section_keys() | _destination_keys()


class HubWebDeepLinkTests(unittest.TestCase):
    def test_native_rust_launcher_is_the_default_without_a_python_fallback(self):
        self.assertIn('name = "kyth-welcome-launch"', CARGO)
        self.assertIn('const TARGET: &str = "/usr/bin/kyth-hub-shell";', LAUNCHER_RS)
        self.assertNotIn("python", _code_only(LAUNCHER_RS).lower())
        self.assertNotIn("kyth-welcome/kyth_welcome", LAUNCHER_RS)

    def test_every_krunner_page_key_resolves(self):
        self.assertIn('name = "kyth-hub-desktop-entries"', CARGO)
        self.assertTrue(GENERATOR_RS.is_file())
        self.assertIn("kyth-welcome-launch", GENERATOR_RS.read_text(encoding="utf-8"))
        emitted = _section_keys()
        self.assertGreaterEqual(len(emitted), 20)
        missing = sorted(emitted - _resolvable_keys())
        self.assertEqual(
            missing,
            [],
            f"krunner ships --page keys the React Hub cannot route (they open Home): {missing}",
        )

    def test_shipped_desktop_files_page_keys_resolve(self):
        emitted = set(_PAGE_ARG_RE.findall(CTX_INSTALLS_SH))
        self.assertIn("App Store", emitted, "context-menu installer entry lost its --page key")
        self.assertIn("/usr/share/kyth/hubRoutes.json", CTX_INSTALLS_SH)
        self.assertEqual(sorted(emitted - _resolvable_keys()), [])

    def test_destinations_cover_the_full_pulse_rail(self):
        self.assertEqual(
            _destination_keys(),
            {"Welcome", "Play", "Apps", "This PC", "Move In", "Updates"},
        )

    def test_updates_is_the_last_left_rail_destination(self):
        entries = re.findall(r'\{ to: "([^"]+)", label: "([^"]+)",', SIDEBAR_TSX)
        self.assertTrue(entries)
        self.assertEqual(entries[-1], ("/updates", "Updates"))

    def test_sections_are_derived_not_hardcoded_in_the_route_table(self):
        # Guards the regression's actual cause: if someone re-lists sections
        # by hand, adding a section to hubSections.ts stops being enough and
        # the next key silently falls back to Home.
        for array in ("PLAY_SECTIONS", "APPS_SECTIONS", "THIS_PC_SECTIONS", "MOVE_IN_SECTIONS", "UPDATES_SECTIONS"):
            self.assertIn(array, DESTINATIONS_CODE)
        self.assertIn("DESTINATIONS", DEEP_LINK_CODE)
        for key in sorted(_section_keys()):
            for name, code in (("deepLink.ts", DEEP_LINK_CODE), ("destinations.ts", DESTINATIONS_CODE)):
                self.assertNotIn(
                    f'"{key}"', code, f"{key} hardcoded in {name} rather than derived"
                )

    def test_section_deep_links_and_hub_page_agree_on_the_query_param(self):
        # Two halves of one contract: deepLink.ts writes ?section=, HubPage
        # reads it. If either side renames it, deep links go to the
        # destination's first tab instead of the requested one.
        self.assertIn("?section=${encodeURIComponent(section.key)}", DEEP_LINK_CODE)
        self.assertIn('searchParams.get("section")', HUB_PAGE_CODE)
        self.assertIn("useSearchParams", HUB_PAGE_CODE)
        # A mount-time effect syncing state->URL would clobber the incoming
        # deep link; the tab state must live only in the URL.
        self.assertNotIn("useState", HUB_PAGE_CODE)

    def test_single_instance_listener_is_registered_before_initial_dispatch(self):
        listener = DEEP_LINK_TS.index('await listen<string>("navigate"')
        pending = DEEP_LINK_TS.index('const pending = await invoke<string | null>("take_pending_page")')
        self.assertLess(listener, pending)

    def test_acceptance_events_cross_the_tauri_command_boundary(self):
        # The installed-image Hub acceptance harness observes this file to
        # qualify deep links and page probes. A silent no-op here makes every
        # valid Hub launch look like a deep-link failure in the VM.
        self.assertIn('invoke("acceptance_record", { event, detail })', ACCEPTANCE_TS)

if __name__ == "__main__":
    unittest.main()
