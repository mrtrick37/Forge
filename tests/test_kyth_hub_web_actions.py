"""React Hub mutating actions must stay reachable and real.

The Phase 2 commits landed bootc upgrade/rollback/switch and Guardian
execute as Tauri commands with liveData.ts wrappers, but no component ever
called them — the backend was complete and the UI never followed, so
Updates/Channels/Guardian were read-only in the React Hub while the Qt Hub
could act. `bootc_switch_branch` was worse than unreachable: it validated
its input and returned "switch queued" prose without calling anything.

Nothing in the invoke chain catches either failure — an unused export is
valid TS, and a command that returns a success string looks successful.
These are static checks over the shipped sources for that reason.
"""
from __future__ import annotations

import json
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
HUB_WEB = ROOT / "src" / "kyth-hub-web" / "src"
TAURI_SRC = ROOT / "src" / "kyth-hub-web" / "src-tauri" / "src"
MAIN_RS = (TAURI_SRC / "main.rs").read_text(encoding="utf-8")
# Commands are intentionally split into domain modules. Include those source
# files in static checks that verify command definitions, while keeping the
# generate_handler! parsing against main.rs itself.
MAIN_RS += "\n" + "\n".join(path.read_text(encoding="utf-8") for path in sorted((TAURI_SRC / "commands").glob("*.rs")))
LIVE_DATA = (HUB_WEB / "services" / "liveData.ts").read_text(encoding="utf-8")
COMMAND_LEDGER = (ROOT / "src" / "kyth-hub-web" / "COMMAND_LEDGER.md").read_text(encoding="utf-8")

HUB_ROUTES = json.loads(
    (HUB_WEB / "data" / "hubRoutes.json").read_text(encoding="utf-8")
)

# The mutating wrappers, and the section each one belongs to.
MUTATING_WRAPPERS = {
    "invokeBootcUpgrade": "UpdatesSection.tsx",
    "invokeBootcRollback": "UpdatesSection.tsx",
    "invokeBootcSwitchBranch": "ChannelsSection.tsx",
    "invokeGuardianExecute": "GuardianSection.tsx",
    "invokeOpenFeedbackIssue": "FeedbackSection.tsx",
}

# Bridge commands with no liveData.ts wrapper on purpose. Each needs a
# reason, because "no wrapper" is exactly what the orphan check below is
# meant to catch — an entry here is a deliberate exception, not a TODO.
UNWRAPPED_COMMANDS = {
    # Plumbing the generic fetchProbeSection helper drives, not a section.
    "probe_backend": "read through fetchProbeSection, not a named wrapper",
    # Consumed by the deep-link path in App.tsx, not by a section.
    "take_pending_page": "deep-link bootstrap, invoked from App.tsx",
    # CHANNEL_DISPLAY in liveData.ts is the one authority for channel
    # labels, and it is synchronous, which its two callers need.
    "branch_display_name": "superseded by the synchronous CHANNEL_DISPLAY map",
    # Text/byte helpers with no display of their own.
    "strip_ansi": "text helper, no section renders raw command output",
    "disk_write_bytes": "installer progress helper, no Hub surface",
    "amd64_manifest_entry": "registry parsing detail behind collect_availability",
    "acceptance_record": "installed-image evidence channel used by deepLink.ts and acceptance probes",
    "acceptance_mode": "installed-image evidence channel used by acceptance probes",
    "acceptance_degraded_dashboard": "installed-image degraded-state fixture used by Dashboard.tsx",
}


# Recipes that take parameters but whose *defaults* are what the button
# promises. Everything else parameterized belongs in a CommandLine — see
# test_recipe_buttons_do_not_use_parameterized_recipes.
PARAMETERIZED_RECIPE_BUTTONS = {
    # `gaming-audit mode=""` — the empty default is the full read-only audit,
    # which is exactly what "Gaming stack audit" says it does.
    "gaming-audit": 'mode=""',
}


def _ui_sources() -> dict[str, str]:
    sources = {}
    for sub in ("components", "pages"):
        for path in (HUB_WEB / sub).rglob("*.tsx"):
            sources[path.name] = path.read_text(encoding="utf-8")
    return sources


class HubWebActionTests(unittest.TestCase):
    def test_release_ledger_records_the_mutating_bridge_surface(self):
        """The release inventory is a contract, not advisory prose."""
        for command in (
            "guardian_execute_recipe", "privileged_action", "bootc_upgrade",
            "install_flatpak", "smb_save_configured_share", "kali_create",
            "gaming_tool_install", "scx_set_scheduler",
        ):
            self.assertIn(command, COMMAND_LEDGER)

    def test_every_mutating_wrapper_is_reachable_from_the_ui(self):
        sources = _ui_sources()
        for wrapper, expected_file in MUTATING_WRAPPERS.items():
            consumers = [name for name, text in sources.items() if re.search(rf"\b{wrapper}\b", text)]
            self.assertTrue(
                consumers,
                f"{wrapper} has no component consumer — the action is unreachable in the UI",
            )
            self.assertIn(expected_file, consumers, f"{wrapper} should be wired into {expected_file}")

    def test_switch_branch_actually_runs_the_recipe(self):
        body = re.search(
            r"fn bootc_switch_branch\(.*?\n\}", MAIN_RS, re.S
        )
        self.assertIsNotNone(body, "bootc_switch_branch not found")
        text = body.group(0)
        # It must delegate, not just format a reassuring string. The guarded
        # native command keeps the channel mapping fixed and auditable.
        self.assertIn("kyth-bootc-guard", text)
        # Rustfmt is free to place the format string on the next line; the
        # contract is the fixed operation template, not its source layout.
        self.assertRegex(text, r'format!\(\s*"switch-\{\}"')
        self.assertNotIn("start_just_job", text)
        self.assertNotIn('Command::new("just")', text)
        self.assertIn("switch_channel_arg", text)
        self.assertNotIn("queued — run bootc switch via polkit terminal", text)

    def test_switch_branch_does_not_pass_caller_input_to_argv(self):
        body = re.search(r"fn bootc_switch_branch\(.*?\n\}", MAIN_RS, re.S).group(0)
        # The allowlist returns a fixed literal; passing `branch` straight
        # to .arg() would put user-controlled text on the command line.
        self.assertNotRegex(body, r"\.arg\(\s*&?branch")

    def test_guardian_execute_is_gated_on_pending_recommendations(self):
        body = re.search(r"fn guardian_execute_recipe\(.*?\n\}", MAIN_RS, re.S)
        self.assertIsNotNone(body, "guardian_execute_recipe not found")
        self.assertIn("is_pending_recipe", body.group(0))

    def test_recipe_id_reaches_the_frontend_so_a_fix_can_be_run(self):
        # Without recipe_id in the snapshot response there is no id to pass
        # back to guardian_execute_recipe, so no "Run fix" button can exist.
        pending_struct = re.search(r"struct GuardianPendingResponse \{.*?\n\}", MAIN_RS, re.S)
        self.assertIsNotNone(pending_struct)
        self.assertIn("recipe_id", pending_struct.group(0))
        self.assertIn("recipe_id: string;", LIVE_DATA)
        self.assertIn("recipeId: string;", LIVE_DATA)
        self.assertIn("recipeId: item.recipe_id", LIVE_DATA)

    def test_home_guardian_history_has_confirm_and_dismiss_paths(self):
        history = (HUB_WEB / "components" / "GuardianHistoryCard.tsx").read_text(encoding="utf-8")
        dashboard = (HUB_WEB / "pages" / "Dashboard.tsx").read_text(encoding="utf-8")
        self.assertIn("aria-expanded={isExpanded}", history)
        self.assertIn("Confirm & run", history)
        self.assertIn("Dismiss", history)
        self.assertIn("dismissGuardianRecommendation", dashboard)


class HubWebCoverageTests(unittest.TestCase):
    """Every backend read has to reach a section, and every section a page.

    The mutating-action gap the tests above cover recurred at a larger
    scale once the bridge grew to ~60 commands: 31 liveData.ts fetchers
    existed that no component ever called, so sections rendered a
    "Preview" badge while their backend sat finished behind them. Same
    blind spot as before — an unused export typechecks — so it is checked
    the same way.
    """

    def test_no_live_data_export_is_orphaned(self):
        exports = re.findall(r"^export (?:async function|function|const) (\w+)", LIVE_DATA, re.M)
        self.assertGreater(len(exports), 40, "liveData.ts exports not parsed — did the file format change?")
        sources = _ui_sources()
        orphans = [
            name
            for name in exports
            if not any(re.search(rf"\b{name}\b", text) for text in sources.values())
        ]
        self.assertEqual(
            [],
            sorted(orphans),
            "these liveData.ts reads are wired to the backend but no section renders them",
        )

    def test_every_bridge_command_has_a_wrapper_or_a_documented_exemption(self):
        handler = re.search(r"generate_handler!\[(.*?)\]", MAIN_RS, re.S)
        self.assertIsNotNone(handler, "generate_handler! block not found")
        commands = [c.strip().rsplit("::", 1)[-1] for c in handler.group(1).replace("\n", " ").split(",") if c.strip()]
        self.assertGreater(len(commands), 50, "command list not parsed — did main.rs change shape?")
        missing = [
            command
            for command in commands
            if command not in UNWRAPPED_COMMANDS and not re.search(rf'"{command}"', LIVE_DATA)
        ]
        self.assertEqual(
            [],
            sorted(missing),
            "these bridge commands have no liveData.ts wrapper; add one or document the exemption",
        )

    def test_exemptions_still_name_real_commands(self):
        # A stale exemption would silently hide a genuinely orphaned command.
        handler = re.search(r"generate_handler!\[(.*?)\]", MAIN_RS, re.S).group(1)
        commands = {c.strip().rsplit("::", 1)[-1] for c in handler.replace("\n", " ").split(",") if c.strip()}
        self.assertEqual(set(), set(UNWRAPPED_COMMANDS) - commands)

    def test_every_section_key_has_a_component(self):
        # HubPage renders nothing for a key with no component, which reads
        # as a blank tab rather than an error.
        keys = [
            section["key"]
            for destination in HUB_ROUTES["destinations"]
            for section in destination["sections"]
        ]
        self.assertGreaterEqual(len(keys), 20, "hubSections.ts keys not parsed")
        wired = set()
        for page in ("Play.tsx", "Apps.tsx", "ThisPc.tsx", "MoveIn.tsx", "Updates.tsx"):
            text = (HUB_WEB / "pages" / page).read_text(encoding="utf-8")
            block = re.search(r"sectionContent=\{\{(.*?)\}\}", text, re.S)
            self.assertIsNotNone(block, f"{page} has no sectionContent map")
            # Not line-anchored: a formatter collapsing the map onto one
            # line must not fail this test for a cosmetic reason.
            wired.update(quoted or bare for quoted, bare in re.findall(r'(?:^|[{,])\s*(?:"([^"\n]+)"|([\w-]+))\s*:\s*\w+Section', block.group(1)))
        self.assertEqual(set(), set(keys) - wired, "section keys with no component wired in their page")

    def test_every_recipe_button_names_a_real_recipe(self):
        # RecipeButton spawns `just <name>` fire-and-forget: a typo gives a
        # button that reports success and does nothing, which is exactly the
        # failure mode this file exists to catch.
        shipped = set()
        for path in (ROOT / "build_files" / "just").rglob("*.just"):
            shipped.update(
                re.findall(r"^([a-z][a-z0-9-]*)(?:\s+[^:\n]*)?:(?!=)", path.read_text(encoding="utf-8"), re.M)
            )
        self.assertGreater(len(shipped), 50, "no just recipes parsed — did build_files/just move?")
        referenced = set()
        for text in _ui_sources().values():
            referenced.update(re.findall(r'recipe="([^"]+)"', text))
        self.assertTrue(referenced, "no RecipeButton call sites found")
        self.assertEqual(set(), referenced - shipped, "RecipeButton names a recipe that does not exist")

    def test_recipe_buttons_are_in_the_typed_rust_hub_action_allowlist(self):
        updates = (TAURI_SRC / "commands" / "updates.rs").read_text(encoding="utf-8")
        allowed = set(re.findall(r'=>\s*"([a-z0-9-]+)"', updates))
        referenced = set()
        for source in HUB_WEB.rglob("*.tsx"):
            referenced.update(re.findall(r'recipe="([^"]+)"', source.read_text(encoding="utf-8")))
        self.assertTrue(referenced, "no static RecipeButton call sites found")
        self.assertEqual(set(), referenced - allowed, "RecipeButton bypasses the Rust HubAction allowlist")

    def test_recipe_buttons_do_not_use_parameterized_recipes(self):
        # `just_run` spawns `just <name>` with no arguments, so a recipe with
        # parameters runs its *defaults*, which need not match the button.
        # `switch-kernel flavor="fedora"` shipped under a "Switch kernel"
        # button and staged a switch off the CachyOS default; the name check
        # above cannot see it, because the name is real. Parameterized
        # recipes belong in a CommandLine, where the argument is visible.
        signatures = {}
        for path in (ROOT / "build_files" / "just").rglob("*.just"):
            # The installed native manifest intentionally accepts an argv
            # tail so Rust can validate it. Its defaults and policy are not
            # implemented by just, so this UI check applies only to the
            # legacy source recipe fixtures.
            if path.name == "native.just":
                continue
            for match in re.finditer(
                r"^([a-z][a-z0-9-]*)((?:\s+[^:\n]*)?):(?!=)", path.read_text(encoding="utf-8"), re.M
            ):
                signatures[match.group(1)] = match.group(2).strip()
        self.assertGreater(len(signatures), 50, "no just recipes parsed — did build_files/just move?")
        referenced = set()
        for text in _ui_sources().values():
            referenced.update(re.findall(r'recipe="([^"]+)"', text))
        offenders = {
            name: signatures[name]
            for name in referenced
            if signatures.get(name) and name not in PARAMETERIZED_RECIPE_BUTTONS
        }
        self.assertEqual({}, offenders, "RecipeButton runs a recipe whose defaults it cannot show")

    def test_parameterized_button_allowlist_is_not_stale(self):
        referenced = set()
        for text in _ui_sources().values():
            referenced.update(re.findall(r'recipe="([^"]+)"', text))
        self.assertEqual(set(), set(PARAMETERIZED_RECIPE_BUTTONS) - referenced)

    def test_just_listing_does_not_button_parameterized_recipes(self):
        # JustSection builds its buttons from `just --list` at runtime, so
        # the recipe name is a variable and the two checks above cannot see
        # it: a "kernel" filter put a one-click `switch-kernel` — which
        # defaults to Fedora — in front of the user. The gate has to be in
        # the row itself, on the `params` field just_list now returns.
        text = (HUB_WEB / "components" / "JustSection.tsx").read_text(encoding="utf-8")
        self.assertIn("r.params", text, "JustSection ignores whether a recipe takes arguments")
        self.assertNotIn("RecipeButton", text, "dynamic recipe names must not become executable buttons")
        # The field has to survive the bridge, or the branch is always false.
        self.assertIn("params: string", LIVE_DATA)

    def test_open_feedback_issue_encodes_caller_input(self):
        # Both halves are caller-supplied. The shared Rust projection owns
        # encoding now; this bridge must keep the target fixed, scrub the
        # report, and delegate to that helper rather than rebuilding a URL.
        body = re.search(r"fn open_feedback_issue\(.*?\n\}", MAIN_RS, re.S)
        self.assertIsNotNone(body, "open_feedback_issue not found")
        text = body.group(0)
        self.assertIn('"https://github.com/kyth-os/kyth"', text)
        self.assertIn("kyth_shared::diagnostic_report::github_issue_url", text)
        self.assertIn("kyth_shared::diagnostics_scrub::scrub_logs", text)
        # No raw interpolation of either argument into the URL.
        self.assertNotRegex(text, r"\{title\}|\{body\}")

    def test_no_section_still_advertises_itself_as_a_preview(self):
        # The retired SectionPreviewCard told the user a section "exists
        # and works in the current Qt Hub today". The Qt Hub is gone, so
        # that copy would now be a lie about shipped behaviour.
        self.assertFalse(
            (HUB_WEB / "components" / "SectionPreviewCard.tsx").exists(),
            "SectionPreviewCard is unreachable — every section key has a component",
        )
        for name, text in _ui_sources().items():
            self.assertNotIn("in the current Qt Hub today", text, f"{name} still points users at the retired Qt Hub")


if __name__ == "__main__":
    unittest.main()
