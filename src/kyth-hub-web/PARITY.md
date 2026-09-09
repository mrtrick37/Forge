# Kyth Hub Parity — Rust/Tauri/React migration

The Tauri/React shell is the Hub UI. It provides the responsive navigation and
complete feature-page surface, with direct native Rust bridge commands under
`src-tauri/`.

`kyth-welcome-launch` starts `/usr/bin/kyth-hub-shell`. The retired Python/Qt
Hub is no longer a supported fallback. The React/Tauri build is covered by
`check-hub-web-shell.sh`.

Native interactive coverage now includes the shared familiar-app chooser and
the verified Gaming recipe set: Steam, Heroic, Lutris, Bottles, Prism Launcher,
Itch.io, Epic, Battle.net, EA App, Ubisoft Connect, Steam export, OBS, GPU
Screen Recorder, GOverlay, MangoJuice, UMU, LACT, Piper, and Solaar. The
native chooser exposes one explicit Flatpak install target per familiar-app
search, while parameterized recipes remain outside the generic recipe field.

## Destination → Section map (single source: `src/kyth-hub-web/src/data/hubRoutes.json`)

| Destination | Sections (Python `DESTINATION_SECTIONS`) | React `HubSection` status | Live data |
|---|---|---|---|
| Home | Welcome (Dashboard) | `Dashboard.tsx` live — Guardian/Channel/GPU/Storage/User/BootChecks/Recovery via `liveData.ts` → `main.rs:kyth-shared` | live, telemetry charts live when sessions exist |
| Play | Gaming, Performance, Compatibility, Controllers | `Play.tsx` → 4 sections, all `LiveSectionCard` with actions | Gaming: `audit-cache` + `gaming_library` + `gaming_slice`. Performance: `audit-cache` + PipeWire quantum presets + recent telemetry sessions. Compatibility: bundled `compat_games.json` matrix through the typed `compatibility_games` bridge, filter/search, anti-cheat explainers, ProtonDB/xCloud guidance, plus `secureboot-state` + `mesa_version` on mount and `mok_status`/`mesa_overlay_dry_run` on demand. Controllers: `controllers-detect` cache + live `controllers_detect` rescan |
| Apps | App Store, Work Setup | `Apps.tsx` → 2 sections | App Store: `flatpak-apps`/`flatpak-updates` + `starter_packs` + `familiar_apps` chooser. Work Setup: `fonts_ready` + `network-summary` on mount, `ipp_discover` on demand |
| This PC | Guardian, Hardware, Plasma Wayland, Diagnostics, Repair, NVIDIA, Kernel, Channels, Just, Feedback (10) | `ThisPc.tsx` → 10 sections, all with actions | Repair: `recovery_status` + `deployment_history` + read-only `snapshot_timeline`/`snapshot_count` + `btrfs_health` + `memory_pressure`. Hardware: `hardware-summary` on mount, `pci_devices_by_class`/`loaded_kernel_modules`/`firmware_updates_count` on demand. Plasma: `display-detect` + `plasma_presets` + `desktop_stack_checks`. Diagnostics: `audit-cache` + `boot_runtime_checks` + `is_live_session`. Feedback: prefilled `kyth-os/kyth` issue via `open_feedback_issue` |
| Move In | Move Files, Cloud Storage, Network Shares, VPN | `MoveIn.tsx` → 4 sections, all with actions | Move Files: `ntfs-drives` cache + live `ntfs_devices` rescan. Cloud: `network-summary` + `cloud_oauth_status` + `rclone_oauth_command`. Shares: `smb_browse`/temporary mount command plus persisted add/remove through the typed privileged socket and native Rust root helper; credentials stay off the user config. VPN: `network-summary` + live `network_identity` refresh plus native Rust openconnect/SAML controls in the Hub. All three network sections escalate to `network_identity` on Refresh |
| Updates | Updates | `Updates.tsx` → dedicated update page | `update_status`/`pending_updates_summary` on mount, `collect_availability` → `update_availability_view` on demand |

`27` page keys total (`Welcome` + `6` landings + `21` sections + `1` Dashboard alias) are derived from the shared route manifest. Updates is a dedicated left-rail destination and is listed last in the web menu.

## Conventions the sections follow

- **Nothing renders a fixture.** `services/liveData.ts` returns `null` on failure, and the section shows an honest empty state. `mockDashboard.ts` is now `dashboardTypes.ts` and holds only types; `SectionPreviewCard.tsx` is deleted.
- **Cheap reads on mount, expensive reads on a button.** `mokutil` (~seconds), `fwupd` (20s timeout), `collect_availability` (45s deadline), `ipp_discover`/`smb_browse` (network) and the live driver scan all sit behind an explicit button so switching tabs never stalls.
- **Where a cached and a live read both exist, mount uses the cache and a Refresh/Rescan button escalates to live.** The live result wins once it exists (Controllers, Move Files, the three network sections, Hardware).
- **A recipe is never reported as complete before it finishes.** `run_hub_action` starts a captured background job for a Rust-deserialized `HubAction`, while upgrade, rollback, channel switching, and staged-update activation use the native Rust update job bridge directly. The Tauri shell waits for the process result, keeps a concise stdout/stderr summary, and the Hub renders running, complete, or failed status inline. KDE's graphical askpass helper handles sudo authentication when needed, so the user never has to hunt for a newly opened terminal window.
- **`*_command` helpers return argv and are rendered as copyable text, not spawned.** A generic "run this argv" bridge command would be a new privilege surface. Where a ujust recipe covers the same ground, the section pairs the text with a `RecipeButton` — that path goes through `run_hub_action`, whose closed Rust enum validates the recipe name before the bounded `/usr/bin/just` orchestration runs.
- **`just` is invoked the way `ujust` invokes it.** `ujust` is `JUST_JUSTFILE="/usr/share/ublue-os/justfile" /usr/bin/just "${@}"`, and `branding/31-ujust-recipes.sh` appends Kyth's import to that file. `system::just` sets that variable and is the only module allowed to spawn `just`.
- **The listing includes upstream's recipes, because ujust's justfile does.** Pointing at `/usr/share/ublue-os/justfile` reaches ublue's own imports (`10-update.just`, `30-distrobox.just`, …) as well as kyth's — 23 more recipes, 14 of them argument-free, so Recipes (Just) offers buttons for `bios`, `clean-system`, `update-firmware`, `enroll-secure-boot-key` and friends. That is the same exposure as typing `ujust <name>`, and the terminal wrapper means each still shows its own output and prompt; it is listed here because it is a behaviour change from the kyth-only list, not a defect.

## Privileged action boundary

Long-running Hub actions return a job and are polled by the frontend; they do not block the Tauri UI thread. User Flatpak removal runs with `--user`. System Flatpak removal and named hardware/storage operations use `/run/kyth/privileged.sock`, provided by the root-owned `kyth-privileged.service` and implemented by the native Rust `kyth-privileged` binary. The socket accepts fixed operation names only (`flatpak_uninstall`, `firmware_update`, `nvidia_install`, `kernel_switch`, `secureboot_enroll`, and `bitlocker_unlock`), validates peer credentials and arguments, passes BitLocker keys on stdin, and records an audit line without the secret. Windows migration verification is a native `migration_readiness` Tauri read rather than a privileged action. The socket is not a generic command or argv bridge; the former Python daemon fixture was removed in P2.
- **Recipe launches stay in the Hub.** The shipped recipes use `sudo`, never `pkexec` (`build_files/just/kyth/*.just`). The Rust Tauri runner invokes `/usr/bin/just` directly with `JUST_JUSTFILE` set, captures output, and supplies `SUDO_ASKPASS=/usr/bin/ksshaskpass` when available. No Konsole/xterm wrapper is used for Hub actions; explicit terminal-app buttons elsewhere remain intentional user requests to open a shell. This is a bounded Rust-owned orchestration boundary, not a claim that every recipe body has been ported from shell/Python to Rust.
- **`tests/test_kyth_hub_web_actions.py` and `tests/test_kyth_hub_web_invocation.py` are the gate.** It fails the build if any `liveData.ts` export is orphaned, if any `generate_handler!` command lacks a wrapper without a documented exemption, if a section key has no component, or if a `RecipeButton` names a recipe that does not exist in `build_files/just/` — or names one that takes parameters. `run_hub_action` only accepts the closed Rust `HubAction` enum, so a typo or arbitrary recipe name is rejected during Tauri deserialization. The runner spawns `just <name>` with no arguments, so a parameterized recipe runs its defaults, which need not be what the button says: `switch-kernel flavor="fedora"` under a "Switch kernel" button staged a switch *off* the CachyOS default. Those belong in a `CommandLine`, where the argument is visible. The Recipes (Just) listing builds its rows from `just --list` at runtime, so no static check can see those names — `just_list` returns a `params` field and the section renders parameterized rows as text instead of buttons. `main.rs`'s `JustRecipeResponse` did not serialize `params`, so that guard read `undefined` and buttoned every row anyway; `test_kyth_hub_web_invocation.py` now checks each bridge struct against the TS interface that reads it. Verified against real `just 1.58` output for ublue's justfile with kyth's import appended — what `ujust --list` prints on the image, not kyth's recipes alone: 223 recipes, 101 argument-free (buttons) and 122 parameterized (text). Two kinds of non-recipe line are dropped: the `[KythOS]` heading `[group('KythOS')]` produces (it used to become a button that spawned `just [KythOS]`), and a doc comment `just` prints on its own line when the signature is long, which used to become a row named `#` — upstream's `distrobox-assemble`/`distrobox-new` produce two of those, so the listing also had duplicate React keys.

## What is still not 100%

### Native Rust/Tauri interactive surface — expanded

The Tauri bridge exposes fixed interactive controls for the high-value
workflows that can be safely represented without a generic command bridge:
updates and rollback, Guardian safe repair, Flatpak search/install, gaming and
balanced performance profiles, firmware update, Office fonts, Windows
verification, save-migration tooling, Tailscale setup, AppImage import/launch,
user-scoped Flatpak removal, curated starter packs, ProtonDB lookup, feedback report generation,
BitLocker unlock, SMB browse/mount, and the read-only
desktop/network/deployment/kernel/channel refresh actions. System-changing
actions use explicit confirmation in React, and the recipe runner publishes
structured running/complete/failed state through the Tauri bridge. All
secret-bearing inputs are validated and kept out of status text.

### 1. Charts — live telemetry wired
`PerformanceChart.tsx`/`SessionsChart.tsx` read `kyth-telem` sessions through `liveData.ts:fetchTelemetryRecent` → `telemetry_recent` → `kyth-shared-rs::system::telemetry::recent_sessions` (read-only sqlite). They show `Live` when usable session data exists and an explicit no-data state otherwise; they never render the old `mockDashboard.ts` series. The active SQLite writer is the feature-gated native Rust `kyth-telem` image binary; its former Python fixture was removed in P2.

### 2. Gaming library/migration/setup sub-tabs — LIVE
Python `page_gaming.py` composes 6 mixins (`page_gaming_dashboard/setup/library/fixes/tools/migration`) each with workers (`DataWorker`, `WindowsLibraryWorker`, `ProtonDbBatchWorker`). React `GamingSection.tsx` covers the workflow with audit/master state, detected launchers and library counts, gaming-slice launch options, ProtonDB batch lookup, anti-cheat compatibility, migration guidance, and recipe-backed setup/fix/tool actions. `kyth-shared-rs::system::gaming_tools` carries `page_gaming_tools_grid.py`'s 14-tool install/launch/uninstall catalog and `page_gaming_fixes.py`'s first-failure playbook (copyable safe launch-option tests, ProtonDB/anti-cheat links, one-shot Discord screen share and OBS PipeWire permission fixes) and Fix My Game card (open compatdata/shadercache, copy a safe Proton-prefix reset hint, copy a support-snapshot command) — fix-command argv verified byte-for-byte identical to Python. `system::gaming_perf` and `system::gaming_per_game` now also port `page_gaming_tools_perf.py`'s overlay install-status badges (MangoHud/Gamescope/vkBasalt) with their copyable launch options, sched-ext scheduler control (`kyth-scx status`/`set`/`stop`, live active/configured status), and the per-game profile builder — goal + FPS-cap combos compute a copyable Steam launch option (verified against all 20 goal/fps/hdr combinations of Python's `_update_profile_builder` dict) and "Save per-game" persists to `~/.config/kyth/gaming-per-game.toml` in the same hand-built TOML format Python writes. **Deliberately not ported**: `_build_advanced_kernel_card`'s Fedora/CachyOS kernel switch — it's the same `bootc_switch_branch` capability the Hub's This PC > Kernel section already exposes; Python just shows a second copy of it here, and faithful parity means matching a genuine gap, not a Python-side redundancy.

### 3. Software sub-tabs — LIVE (AppStoreSection + starter packs + familiar apps + Security)
Python `page_software.py` 7 mixins (Starter Packs, Flatpak Store, AppImages, Installed, Developer, Security, Creator) with `software_catalogs.py` (`STARTER_PACKS`, `SEC_BOX`, `FAMILAR_APPS`). React `AppStoreSection` covers Flatpak counts, starter packs with selectable apps, the familiar-app chooser, debounced Flathub search/install, AppImage discovery/import/launch, installed Flatpak removal, and recipe-backed developer tools. Install actions poll the Rust job bridge and refresh the inventory after completion. The Security tab is now ported too: `kyth-shared-rs::system::security_container` carries `page_software_security_kali.py`'s command builders (tiered create, export, remove) faithfully — diffed byte-for-byte against the Python originals in development, whitespace only — and `main.rs`'s `commands::security` module runs them as background jobs (`kali_create`/`kali_export`/`kali_remove`/`kali_enter_terminal`), reporting running/complete/failed like every other long Hub action rather than porting `KaliInstallProgressTracker`'s live percentage bar — a deliberate, documented simplification, not a silent gap. `page_software_security_hosttools.py`'s Flatpak grid (Wireshark, Burp Suite Community) is ported as `sec_host_tools`/`sec_host_tool_install`/`_uninstall`/`_launch`, validated against the fixed 2-tool catalog so it can't become a generic "install/run any Flatpak" bridge.

### 4. Guardian repairs — DONE (command table + eligibility gate ported, recipe set expanded beyond Python's original)
`guardian_execute_recipe` used to hand the recipe id to `just_run`. Guardian ids are dotted (`audio.restart`) and are not just recipes, so nothing ever ran — and the spawn still succeeded, so the Hub reported every repair as launched, advisory notifications included. `guardian.rs` now carries each recipe's `command` argv from `guardian.py`, ports the user-initiated gate, implements all ten `guardian_actions.py:ACTION_EXECUTORS` with bounded argv calls, verifies results, enforces cooldowns, and records Hub executions in Guardian history — plus eleven Rust-only recipes (`bluetooth.restart`, `disk.review`, `flatpak.refresh-metadata`, `flatpak.repair-user`, `memory.pressure-relief`, `network.restart-user`, `plasma.restart-user`, `storage.smart-warn`, `thermal.notify`, `update.review-health`) with no Python equivalent. `GuardianSection` only offers "Run fix" for a runnable risk. The installed service now runs the native Rust extended deterministic sweep and can use a preinstalled `llama-cli` model for strictly schema-validated investigation; missing model assets are explicit degraded state.

### 5. kyth_shared → kyth-shared-rs coverage
Python `src/kyth_shared/kyth_shared` `251` files / `≈1567` defs vs Rust `src/kyth-shared-rs/src` `176` files (`151` under `system::`) — roughly 70% file-count coverage; check `MIGRATION.md`'s "What's ported so far" table for the current module list. Coverage is skewed toward read-only inventory/parsers by design (`MIGRATION.md`'s "read-only first" rule). The installed probe collector, Guardian extended sweep, update watcher, telemetry writer, VPN/SAML workflow, privileged socket daemon, and network-share helper are native Rust; Python service authorities are no longer installed. Selected compatibility fixtures were removed in P2; installed-image acceptance and remaining helper cleanup are operational follow-up, not active service ownership blockers.

### 6. Launchers & single-instance — TAURI/REACT
Rust/Tauri: the primary shell accepts `--page <key>`, forwards later launches through the single-instance plugin, and preserves the destination contract. The native Rust `kyth-welcome-launch` binary starts `/usr/bin/kyth-hub-shell` and fails clearly if it is absent; the compatibility name does not imply a Python implementation or fallback. `Dockerfile` ships both native binaries, and `23-kyth-helper-ctx-installs.sh` installs only the launcher, desktop metadata, and generated route search entries.

### 7. Work Setup actions — LIVE
`WorkSetupSection.tsx` now covers the old page's day-one workflow: LibreOffice
and Betterbird installs use the existing user-scoped Flatpak job bridge;
Microsoft 365's six fixed web apps can be opened or added to the application
menu; PST/OST files are discovered only in user migration folders and are
converted through a bounded `readpst` job; and Focus Sessions hold off sleep
for the selected timer with an explicit end action. No arbitrary URL, file,
or command is accepted from the webview.

### 8. Repair actions — LIVE
Repair now includes the old quick-fix and File History entry points alongside
the deployment/snapshot view: device and startup diagnostics, firmware and
Windows migration checks, recovery checks, and Pika Backup installation/open.
The Hub reports captured job results inline; it does not silently claim that a
repair succeeded when the helper failed.

### 9. Cloud Storage sync — LIVE
Configured rclone remotes retain their provider/folder/last-result summary and
now expose `Sync now` per saved folder. The command validates the remote
against the user-owned Kyth configuration, runs the same remote-to-local
`rclone sync` direction as the Python workflow, and returns captured output to
the Hub. OAuth setup and schedules remain in the existing full workflow.

### 10. Freshness and acceptance coverage — INSTALLED-IMAGE GATE
Home Guardian activity is limited to the last 24 hours, refreshes on a short
heartbeat, and always includes pending recommendations so an old alert cannot
remain an unexplained "confirmation required" row. Both Home and This PC use
expandable activity rows with confirm/run and dismiss paths. Contract/build/
Contract/build/smoke tests cover the native bridge, and the VM guest now boots
the installed Rust/Tauri shell to check its binary, every manifest-derived
`--page` route, single-instance forwarding, degraded Home state, Updates
probe, and an allowlist rejection. The guest emits these checks into the
existing `qualification.json` and `qualification.md` artifacts. A real
testing ISO run with WebKitGTK, services, and representative network/update
state remains the release gate; the post-cutover observation window remains
open until that run passes.

## Remaining work

The four React feature-completeness items remain implemented in the compatibility
shell: Gaming has a migration checklist, ProtonDB lookup, and anti-cheat
guidance; App Store has Flatpak catalog search, AppImage discovery, and
background Flatpak installation status; presets preview before confirmation;
and Guardian repairs execute with verification/cooldowns/history. The native
Rust/Tauri migration now covers the common interactive paths plus read-only
snapshot/deployment timeline and staged-update truth used by Repair. Remaining
product work is dedicated controls for the listed high-risk workflows,
installed-image acceptance testing and retirement of unrelated compatibility
service code. No Python module is part of the supported Hub build or runtime
path.
