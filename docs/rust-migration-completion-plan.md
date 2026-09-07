# Rust Migration Completion Plan

**Status (2026-09-07):** Hub, VPN/SAML, privileged socket, network-share,
telemetry, Guardian, probe, update-watcher, the installer backend, and the
tunable/sysctl registry are already Rust-owned at their installed entry
points (see
[Kyth Hub migration finalization plan](kyth-hub-migration-finalization-plan.md)
and [Installer Migration Plan](installer-migration-plan.md)). The Python
launcher ledger is also complete except for the deliberately deferred
`kyth-vm-acceptance-guest` lifecycle. `kyth-exe-handler` is now a packaged
native Rust launcher with its user interface in the existing **Tauri/React
Hub**.

This revision expands the plan to include the runtime-relevant
`shell-orchestration` surface. After the Phase 5 inventory pass, the generated
inventory contains 100 shell entries already backed by native implementations,
65 queued runtime entries, and one reviewed external `rclone@` interface. The
queued entries are now named function-level migration targets rather than an
undecided future surface. Phase 3 remains blocked on the Hub plan's
post-cutover observation window, but that gate does not exempt shell functions
from the Rust ownership target.

**Scope:** This plan now covers every supported runtime function currently
represented by `python-runtime` or `shell-orchestration`, including read-only,
state-changing, destructive, privileged, boot, recovery, update, Secure Boot,
storage, and installer-adjacent behavior. Rust must own policy, validation,
execution, result semantics, and failure handling wherever a native
implementation is technically viable. Shell may remain only as declarative
packaging/build glue, a service/desktop launch contract, or a deliberately
documented external interface whose behavior is owned by Rust. This plan
supersedes no existing Hub or installer safety contract; it makes their
Rust-ownership requirement explicit for non-Hub shell callers as well.

## Why this plan exists, and why it is not "port 255 modules"

`build_files/config/runtime-migration-report.json` currently lists 296 active
Python entries: 41 tagged `python-runtime` and 255 tagged
`python-shared-package`. Read literally, that suggests hundreds of modules
still need porting. They don't. The `python-shared-package` tag comes from a
single blanket rule in `check-runtime-migration-inventory.py`:

```python
elif surface == "python-runtime":
    if rel(path).startswith("src/kyth-welcome/"):
        ...source-only...
    else:
        authority, scope, active, priority = "python-shared-package", "standalone", True, 2
```

Every `.py` file under `src/kyth_shared/kyth_shared/` that isn't in
`kyth-welcome` gets stamped active/queued, regardless of whether anything still
calls it. Verification for this plan found that a large share of that package
is already dead in the runtime sense:

- `tunable.py`'s `_BUILTIN_TUNABLES` registry has 94 entries dynamically
  dispatching to 93 distinct worker modules (`aio_max.py`, `ananicy_preset.py`,
  `bore_tune.py`, `btrfs_autotune.py`, and so on — `mimalloc` and
  `mimalloc-run` both resolve to `mimalloc_preset.py`) via
  `importlib.import_module`. A name-for-name diff against
  `src/kyth-shared-rs/src/system/tunable_registry.rs` shows all 94 registry
  names are already present in the Rust registry, and `tunable_bin.rs` (the
  binary `build_files/kyth-tunable` now resolves to) contains no Python
  subprocess calls. The Hub finalization plan independently confirms this:
  "all 49 sysctl and all 45 module-specific entries complete 2026-09-03;
  compatibility retained only as a rollback fixture." The checker's own test
  suite (`tests/test_runtime_migration_inventory.py::test_all_tunable_aliases_are_native_dispatcher_entries`)
  already confirms the 94 *alias scripts* under `build_files/kyth-*` (e.g.
  `build_files/kyth-swappiness`) are `done-native`. What it does not check is
  the 93 underlying worker *modules* in `kyth_shared/` those aliases used to
  call — those are the ones still counted active in `python-shared-package`.
- `gaming_master.py` and `perf_audit.py` use the same `__import__(f"kyth_shared.{mod}", ...)`
  pattern for their own worker sets, which is why a plain static-import
  closure from the 41 real launchers only reaches ~42 of the 255 modules —
  the dynamic dispatch tables hide the rest of the true dependency graph in
  both directions (some hidden edges are real, most in this case turned out
  to be dead).
- The reverse error also exists: `build_files/scripts/vm-acceptance.sh` line
  240 runs `python3 -m kyth_shared.qualification` directly from a shell test
  harness, not from any of the 41 launchers and not through a dynamic-dispatch
  table. A reachability definition based only on "launcher/unit static or
  dynamic dispatch" would misclassify `qualification.py` as dead when it
  isn't. Phase 0's reachability check needs a third channel — direct
  shell-harness invocation — alongside launcher static imports and documented
  dynamic-dispatch tables.

Because of this, a `python-shared-package` tag currently conflates three very
different things: still-imported implementation, rollback-only fixture
material superseded by Rust, and genuinely dead code nobody deleted yet. Phase
0 below fixes that distinction; it is the highest-leverage single item in this
plan because it is what will make the inventory numbers trustworthy for every
plan after this one, not just this one.

## The shell and orchestration surface

The report's `active_by_authority` currently lists `shell-orchestration: 161`
active entries, with five additional entries intentionally classified as
explicit exceptions. This is not a blanket claim that every `.just`, unit,
desktop file, or build fragment should become a Rust binary. It is a claim
that every function expressed by those files must be accounted for and that
runtime behavior must have a Rust owner.

- **`data-or-config` (8 entries):** `.desktop`, `.rules`, `.toml`, `.menu`,
  and `.directory` files (e.g. `kyth-sched-profiles.toml`,
  `kyth-web-apps.directory`). These are data, not code — there is nothing to
  port. The classifier marking them `queued` rather than a terminal
  "not applicable" state is itself a small Phase 0 line item (see below), not
  a migration task.
* **Runtime shell functions are in scope.** The queued set includes
  `kyth-boot-verify`, `kyth-enroll-mok`, `kyth-mok-rotate`,
  `kyth-power-arbiter`, `kyth-perf-gate`, `kyth-storage-gate`, the
  `kyth-greenboot-*` group, `kyth-full-update`, `kyth-distrobox-root-launch`,
  `kyth-ntfs-repair`, and similar behavior. Secure Boot, boot-health,
  filesystem repair, update, and privileged operations are not excluded just
  because they are destructive or difficult to test.
* **Declarative files are still part of the inventory.** `.just` recipes,
  systemd units, desktop entries, and timers must be retargeted to the Rust
  owner and tested as launch contracts. They do not need to become standalone
  Rust binaries when they contain no policy or execution logic.
* **Build-time assembly is audited, not blindly rewritten.** Package and image
  construction fragments may remain shell when they only assemble the image or
  install declarative assets. If a build fragment embeds reusable runtime
  behavior, that behavior is split out and ported to Rust; the remaining shell
  is recorded as build-only rather than silently counted as runtime authority.
* **The five current explicit exceptions are no longer permanent exclusions.**
  `kyth-default-flatpaks`, `kyth-flathub-setup`, `kyth-local-bin-migrate`, and
  `rclone@` must be classified under Phase 5. Each either receives a Rust
  owner or a documented, reviewed `not-applicable`/external-interface status.
  No runtime function may remain exempt merely because its current
  implementation is shell.

## Decision

1. Extend the classifier so `python-shared-package` splits into two
   observable states: modules reachable — statically, via a documented
   dynamic-dispatch table, or via direct shell-harness invocation (see the
   `qualification.py` example above) — from an active launcher/unit/harness,
   and modules retained only as rollback/parity fixtures for an
   already-completed Rust cutover. Only the first group counts toward
   `active_python`.
2. Treat the `python-runtime` and runtime-relevant `shell-orchestration`
   entries as one migration ledger. Port them in risk order using the same
   taxonomy the checker already encodes (`READ_ONLY_NAMES` → `WRITER_NAMES` →
   `DAEMON_NAMES` → privileged), then extend that taxonomy for boot, recovery,
   Secure Boot, storage, update, and destructive operations. The taxonomy
   determines verification depth, not whether a function is eligible for
   migration.
3. A runtime function is "done" only when: its Rust replacement owns policy,
   validation, execution, result semantics, and failure handling; the
   replacement is wired into the packaged image; every launcher/unit/recipe
   caller is retargeted; the inventory records the Rust owner; the inventory
   and report are regenerated (`--generate`) and pass `just validate`; and
   parity tests cover success, refusal, malformed input, privilege failure,
   timeout, partial failure, and rollback/recovery behavior where applicable.
   Tests are added as `unittest.TestCase` subclasses under `tests/` (CI runs
   `python3 -m unittest discover -s tests -b`; plain pytest-style function
   tests are silently never collected by that discovery — see the Phase 0
   note below, this bit a prior task in this repo).
4. Existing `NOT_PORTED` / `NOT_PORTED_PATHS` entries are inputs to the shell
   triage, not permanent exemptions. They may only remain outside the Rust
   implementation ledger after the Phase 5 review proves that an entry is
   build-only, declarative, or an external interface whose behavior cannot be
   owned by Kyth. Any runtime behavior currently hidden behind those entries
   must be ported or isolated behind a typed Rust boundary.
5. Keep the contributor guidance that directs new runtime, service, CLI, and
   desktop behavior to Rust/Tauri. No new Python or shell runtime authority
   may be added while these phases are in progress; any exception requires a
   documented owner, parity test, and removal condition.

## Phase 0 — Fix the classifier before trusting the queue

- [x] Add a `superseded_by` (or equivalent) field to inventory entries whose
  entire runtime surface has a proven native replacement, starting with
  `tunable.py` and the 93 distinct modules it dispatches to. Cross-check by
  diffing the module names each dynamic-dispatch table can reach against the
  corresponding Rust registry/binary, the same way this plan's audit did for
  `tunable_registry.rs`. Adding this field is a schema change — bump
  `SCHEMA_VERSION` (currently `2` in `check-runtime-migration-inventory.py`)
  and update the validator's schema check accordingly.
- [x] Repeat the same check for `gaming_master.py`'s and `perf_audit.py`'s
  dispatch tables against their Rust counterparts (if any exist yet); where no
  Rust counterpart exists, leave those modules active/queued.
- [x] Add direct shell-harness invocation as a recognized reachability
  channel (not just launcher/unit static or dynamic dispatch), so modules
  like `kyth_shared.qualification` — invoked from
  `build_files/scripts/vm-acceptance.sh` rather than any of the 41 launchers
  — aren't misclassified as dead by the new `superseded_by` logic. Audit
  `build_files/scripts/*.sh` for other `python3 -m kyth_shared.*` /
  `python3 -m kyth_shared` invocations before finalizing the reachable set.
- [x] Give `data-or-config` entries a terminal status distinct from `queued`
  (e.g. `not-applicable`) so config/desktop files stop appearing in the same
  bucket as genuine migration work.
- [x] Re-run `--generate` and confirm `active_python_entries` drops to
  reflect only genuinely reachable code. Record the before/after count in the
  regenerated `docs/runtime-migration-report.md`.
- [x] Document, in the checker's own docstring, that a path-prefix rule alone
  cannot distinguish live from rollback-only Python, since this exact gap is
  what produced the 296-entry overcount this plan starts from.
- [x] Note for whoever extends `tests/test_runtime_migration_inventory.py`:
  it is written as bare `test_*` functions, not `unittest.TestCase`
  subclasses, so it is **not** collected by CI's
  `python3 -m unittest discover -s tests -b` gate. It currently only runs if
  invoked directly or via a pytest-aware runner. Either convert it to
  `unittest.TestCase` style or confirm some other CI step actually collects
  it before relying on it to catch a Phase 0 regression.
  (Done 2026-09-07: converted to `unittest.TestCase`, collected by
  `unittest discover`, plus Phase 0 invariant tests.)
- [x] Same defect, different file: `tests/test_kyth_doctor_native.py` used
  bare `def test_*():` module-level functions, not `unittest.TestCase`
  methods, so `python3 -m unittest tests.test_kyth_doctor_native -v` reports
  "Ran 0 tests" — invisible to `unittest discover`. Its 3 assertions are now
  duplicated by the `entry_points` tuple in `test_python_packaging.py`, so
  this was cleanup (converted to `unittest.TestCase`; no coverage gap).

Known environmental noise, reproduces on a clean checkout, not caused by
migration work — don't try to fix these mid-launcher-port:
`just lint` fails on root-owned leftover dirs under `tmp/` from an old podman
test run; `just validate`/`just test` fail immediately because
`.venv-gui/bin/python` is missing the `pytest` module (use
`PYTHONPATH=build_files/kyth_shared:build_files/kyth-welcome:build_files/kyth-installer
python3 -m unittest discover -s tests` directly instead, per CLAUDE.md); and
`just validate`'s perf gate is failing >20% over its recorded baseline
regardless of code changes — stale baseline, needs a maintainer `--record`
re-baseline, not an automatic fix.

This phase produces no Rust code. It is a prerequisite because every later
phase's "done" count is meaningless while the starting count is inflated by
dead code nobody flagged as dead.

## Phase 1 — User-session writer launchers (28 items)

**Status: complete.** All 28 items below are ported and committed.
`kyth-exe-handler` is a native Rust launcher with its user-visible workflow
implemented as a Tauri/React Hub dialog (see "Completed Tauri migration:
`kyth-exe-handler`" below).

Risk-classified `user-session-writer` by the checker's own `WRITER_NAMES` set
or its default fallback for `kind == "python"`. These apply a single
configuration/preset and exit; they do not run as background daemons and do
not touch partitioning, firmware, or account state, so they carry the lowest
blast radius of the remaining launcher set.

| Launcher | Note |
|---|---|
| `kyth-apply-desktop-layout` | |
| `kyth-apply-display-hdr` | |
| `kyth-apply-explorer` | |
| `kyth-apply-input` | |
| `kyth-apply-network` | |
| `kyth-apply-pipewire-latency` | |
| `kyth-apply-plasma` | |
| `kyth-apply-quicksettings` | |
| `kyth-apply-rgb` | |
| `kyth-apply-role-preset` | |
| `kyth-apply-scaling` | |
| `kyth-apply-scx-preset` | |
| `kyth-apply-tailscale` | |
| `kyth-apply-vrr` | |
| `kyth-apply-window-snap` | |
| `kyth-driver-switch` | Not caught by any `risk_for` bucket today; falls through to the generic `python`→`user-session-writer` default. Review whether GPU/driver selection deserves its own risk tier before treating it as equivalent to the `kyth-apply-*` group. |
| `kyth-exe-handler` | **Complete — native Rust launcher + Tauri/React Hub dialog.** See "Completed Tauri migration: `kyth-exe-handler`" below. |
| `kyth-kali-desktop-fixup` | |
| `kyth-ntfs-repair` | Named in the checker's later privileged-writer branch, but `WRITER_NAMES` matches first, so it currently reports as `user-session-writer`. Confirm that's intentional before porting; NTFS repair is filesystem-mutating. |
| `kyth-performance-mode` | |
| `kyth-refresh-boot-splash-initramfs` | Same dead-branch note as `kyth-ntfs-repair` — touches initramfs, worth a second look at its declared risk tier. |
| `kyth-refresh-taskbar-pins` | |
| `kyth-report-issue` | |
| `kyth-session-snapshot` | |
| `kyth-setup-devcontainer` | |
| `kyth-setup-transfer` | |
| `kyth-vscode-wallet` | |
| `kyth-web-app-categorize` | |

The `kyth-apply-*` family shares an obvious shape (parse config, apply one
preset, exit) and is the best candidate for a shared Rust helper module in
`src/kyth-shared-rs/` — mirroring the `tunable_registry.rs` approach — rather
than 15 independent ports.

## Phase 2 — Daemon-class launchers (12 items)

**Status: complete (12/12).** `kyth-batteryd`, `kyth-backup`, `kyth-cloud-mount`,
`kyth-duperemove`, `kyth-dynamic-lock`, `kyth-game-launch`,
`kyth-proton-cachyos-update`, `kyth-rclone-update`, `kyth-save-sync`,
`kyth-sched`, `kyth-storage-sense` are ported and committed. Only
`kyth-user-polish` is now ported to the native shared-crate binary.

Risk-classified `daemon` by `DAEMON_NAMES`. These run continuously or on a
schedule and warrant more parity testing before cutover, in the same spirit
as the Hub plan's Guardian/update-watcher ports.

- [x] Port in this order: read-mostly monitors first (`kyth-batteryd`,
  `kyth-storage-sense`), then scheduled/idempotent jobs
  (`kyth-duperemove`, `kyth-proton-cachyos-update`, `kyth-rclone-update`),
  then session-state daemons (`kyth-dynamic-lock`, `kyth-sched`,
  `kyth-game-launch`), then data-affecting backup paths
  (`kyth-backup`, `kyth-save-sync`, `kyth-cloud-mount`) — matching the
  installer plan's own convention of sequencing destructive/data-affecting
  work after everything else has parity coverage. `kyth-user-polish` was
  placed in the scheduled/idempotent group by this ordering and is now
  complete.
- [x] Each port needs a shared Rust/Python parity fixture before the Python
  path is removed, the same pattern used throughout the Hub and installer
  plans (see `src/kyth-hub-web/PARITY.md` and
  `src/kyth-shared-rs/MIGRATION.md` for the established format).

### Completed: `kyth-user-polish`

`src/kyth_shared/kyth_shared/user_polish.py` (642 lines) was the full-parity
target — theme/fonts/icons/favorites/klipper/dolphin/spectacle/screen-lock/
kwin settings, lossy/compressed `user-places.xbel` writes with XXE hardening,
a lockfile-based single-instance guard, MissionCenter flatpak install logic,
Brave `.desktop` password-store regex patching, and Desktop/recycle-bin
shortcut seeding. `src/kyth-shared-rs/src/desktop_polish.rs` (83 lines)
already exists but is **not** a starting draft of this port — its own
comment says KDE writes, folder creation, and session commands "remain
caller-owned," i.e. still Python. It covers only declarative constants and
desktop-entry drift-check helpers, and it has drifted from the Python
manifest it's meant to mirror: its `MIME_DEFAULTS` has 26 entries against
`user_polish.py`'s `_set_mime_defaults` (29 — missing `image/webp`,
`audio/mpeg`, `audio/flac`), and `FOLDER_METADATA`/`USER_FOLDERS` need a
diff against `polish_manifest.py` and `_place_definitions` (13 places, not
10) before being reused. Treat it as a partial fixture to reconcile, not a
base to build on.

Known hazards, carried forward from prior reconnaissance of `user_polish.py`,
`user_polish_flatpak.py`, and `polish_manifest.py` — **the port pass still
needs its own fresh, non-lossy read of all three files before writing Rust**;
this list is a starting checklist, not a substitute for that read. The port
is now implemented in `src/kyth-shared-rs/src/user_polish_bin.rs`, with the
Python modules retained as source-only parity fixtures:

- `smoke_check.py:215` only path-checks `/usr/bin/kyth-user-polish` exists —
  that keeps passing after the port since the Rust binary installs at the
  same path. Not a hazard.
- The real presence-sentinel was
  `tests/test_kyth_sysconfig_fragments.py::test_late_plasma_splash_is_kyth_owned`
  (~line 111–123): it reads the native Rust source after the Python launcher
  fixture was retired and asserts that the Kyth Plasma theme literal remains
  present.
- `user_polish_flatpak.py` is a thin re-export; grep its importers before
  assuming it can be deleted alongside the launcher.
- Quirks to pin byte-for-byte: the KDE block gates on `kwriteconfig6`, not
  the fallback chain used elsewhere in the Rust crate; `sys.exit(1)` on
  desktop-layout failure deliberately leaves the completion stamp unwritten;
  both loser paths of the stamp→lock→re-check-stamp sequence still run
  `cleanup_autostart`; and `had_polish_stamp` globs `user-polish-*` (any
  version) as distinct from the version-specific `already_run` check.
- Full caller list to rewire per [[feedback-rust-migration-full-rewire]]:
  `build_files/config/kyth-user-polish.service`,
  `build_files/scripts/branding/19-user-comfort-polish.sh`,
  `build_files/kyth-scripts/kyth-user-polish.desktop`,
  `src/kyth-welcome/kyth_welcome/services/repair.py:28`,
  `check-runtime-migration-inventory.py:126` (name must move into
  `NATIVE_BINARIES`), and `test_python_packaging.py`'s
  `test_diagnostic_entry_points_are_native_rust_binaries` `entry_points`
  tuple.

## Not a phase: `kyth-vm-acceptance-guest` is not a quick win

An earlier pass over this plan treated `kyth-vm-acceptance-guest` as a
ready-made cutover because `src/kyth-shared-rs/Cargo.toml` already defines a
`[[bin]] name = "kyth-vm-acceptance-guest"` target
(`src/vm_acceptance_guest_bin.rs`). Direct inspection shows that's wrong: the
Rust binary is a **separate, narrower tool**, not a drop-in replacement. Its
own doc comment says so directly:

> The lifecycle `run` command remains owned by the Python fixture until the
> destructive installer/update/rollback acceptance matrix is complete.

The Rust binary only implements read-only reporting (`enabled`, `report`,
`decode-bootc`, `count-deployments`). The Python launcher
(`build_files/kyth-vm-acceptance-guest` → `kyth_shared.vm_acceptance.main`)
implements `enabled` and `run`, where `run` performs the actual
install-from-live-ISO and update/rollback lifecycle (`bootc rollback`, power
reboot/poweroff, state-file transitions). `tests/test_kyth_vm_acceptance.py`
pins the Python launcher path directly (`GUEST = ROOT /
"build_files/kyth-vm-acceptance-guest"`) and exercises that lifecycle. Cutting
the launcher over to the Rust binary as currently built would silently drop
the `run` command entirely.

This is, in fact, already correctly out of scope: it's the same
destructive installer/update/rollback acceptance matrix this plan's Out of
scope section already excludes as an open Hub/installer release gate. This is
the 41st `python-runtime` launcher referenced in the Status paragraph above —
it stays Python, deferred, until that gate closes. No action item follows
from this beyond the correction itself: do not add it as a phase, and do not
reclassify the launcher's `NATIVE_BINARIES` status until the Hub/installer
plans' own acceptance-gate work replaces the `run` path.

## Completed Tauri migration: `kyth-exe-handler`

`kyth-exe-handler` was bucketed into Phase 1 by the checker's
`WRITER_NAMES` set, but it does not apply a configuration and exit: it is a
470-line PySide6 dialog (`desktop/exe_handler.py`) — the registered MIME
handler for Windows executables and RPMs — with Bottles workflow threads.
The assessment backend it leans on is already Rust-owned
(`system::exe_compat`: hashing, offline lookup, Steam rewriting). A headless
native shim would silently drop the dialog, which is the launcher's entire
user-visible behavior.

**Decision and implementation (2026-09-07): use the existing Tauri/React Hub.** The replacement
is a focused Hub dialog/window, not a new GUI toolkit or a second standalone
UI. The Rust Tauri command layer owns typed assessment and workflow requests;
the React UI owns presentation, confirmation, progress, and actionable error
states. It must preserve all current MIME-handler behavior: RPM guidance,
offline Linux-app suggestions, Flathub search, installed-Flatpak launch,
explicit Bottles install/run workflow, compatibility warnings, and the
per-user auto-Bottles preference.

Completed implementation:

1. The remaining Python business logic is behind typed Rust APIs, reusing
   `system::exe_compat` and `system::app_suggestions`; add a bounded Rust
   workflow layer for Bottles/Flatpak operations rather than exposing generic
   process execution to the webview.
2. Narrowly scoped Tauri commands in `src/kyth-hub-web/src-tauri/` and a
   Hub React dialog that preserves the current interaction and confirmation
   behavior. Do not add a new Tauri application or an unrestricted shell,
   filesystem, or command bridge.
3. The `kyth-exe-handler.desktop` entry point now uses the packaged
   Rust/Tauri handler only after parity tests cover successful, declined,
   unsupported, malformed-file, and workflow-failure paths. Regenerate the
   inventory/report and retain the Python implementation as a rollback fixture
   until the approved observation window closes.

`kyth-exe-handler` is listed in `NATIVE_BINARIES`, so it is no longer an
active Python runtime authority. Its Python implementation remains source-only
rollback/parity material until the approved observation window closes.

## Phase 3 — Retire superseded `kyth_shared` fixture material

**Gated on the Hub finalization plan's own open item:** "Run a post-cutover
observation window before deleting compatibility code" (Hub plan, open-item
register, row 7 — begins after promoted-image acceptance, not yet started).
The tunable compatibility module is explicitly "retained only as a rollback
fixture" pending that window. Phase 0's reclassification changes the
*count* — dead modules stop being reported as `active` — but does not change
the *files on disk*. Do not delete or move the 98 superseded worker/fixture
modules (or the `gaming_master.py`/`perf_audit.py` equivalents, once Phase 0
confirms which of their workers are superseded) until that observation window
closes. Once it does, this becomes mechanical: modules confirmed superseded
move to `explicitly-not-ported`/source-only or are deleted outright,
following the same P2 pattern the Hub plan already used for retired VPN/
privileged-socket/telemetry fixtures. Starting this phase before Phase 0
lands would re-litigate the same classification question module by module;
starting it before the Hub plan's observation window closes would delete a
deliberate rollback fixture ahead of its own release gate.

## Phase 4 — Convention change

**Complete after `kyth-user-polish` landed.** Phase 2 closes to 12/12 with
`kyth-user-polish`; the contributor convention applies while the VM acceptance
gate remains open. At that point
"Phase 1/2 demonstrate the replacement shape is stable" is satisfied and this
becomes the immediate next item — cheap, and it's what stops new Python
arriving in `kyth_shared` by convention while Phase 3 stays blocked.

- [x] Update the contributor architecture guidance: replace "Follow this
  convention for new host-tuning logic: a small, independently testable
  [Python] module" with guidance pointing at the Rust tunable/shared-crate
  pattern, once Phase 1/2 demonstrate the replacement shape is stable.
  Do this after there's a working example to point to, not before. This
  checkout has no `CLAUDE.md`; the equivalent canonical guidance is now in
  `src/kyth-shared-rs/MIGRATION.md`, which points new host-tuning work at the
  Rust shared crate/native dispatcher.

## Phase 5 — Inventory every shell function and assign an owner

**Status: complete (2026-09-07).** This phase converted the shell expansion
from a file-count into an auditable function-level ledger. It covered the
queued `shell-orchestration` entries, the former explicit exceptions, and
runtime shell helpers discovered under `build_files/scripts/`,
`build_files/just/`, systemd units, desktop launchers, and acceptance
harnesses.

- [x] Enumerate every shell function, command sequence, and sourceable helper
  reachable from a supported runtime path. Record its callers, inputs,
  outputs, privilege boundary, files/devices/services touched, and whether it
  can mutate or destroy state.
- [x] Split declarative launch contracts from implementation. A `.just`
  recipe, unit, timer, path, or desktop entry may remain as metadata only when
  it delegates to a named Rust binary or typed Tauri command and contains no
  policy, parsing, mutation, or error-handling authority of its own.
- [x] Classify each implementation function as one of `read-only`,
  `idempotent-writer`, `destructive`, `privileged`, `boot/recovery`, or
  `build-only`. `destructive`, `privileged`, and `boot/recovery` are migration
  classes with deeper tests, not exclusions from the Rust target.
- [x] Resolve the former explicit exceptions (`kyth-default-flatpaks`,
  `kyth-flathub-setup`, `kyth-local-bin-migrate`, and `rclone@` plus their
  associated unit entries) into a Rust owner or a reviewed terminal status.
- [x] Extend the inventory schema and tests so a shell entry cannot be marked
  `done-native` merely because it is a thin-looking shim. The record must name
  the Rust owner and the function-level parity test that proves the shim does
  not retain runtime authority.

The phase exit criterion is a complete ledger: every runtime shell function
has a current owner, an explicit function-level category, and a Phase 6 Rust
target or reviewed external/build-only classification. `queued` now means a
named Phase 6 migration target; it no longer means unclassified work.

## Phase 6 — Port shell runtime logic to Rust, including destructive paths

**Status: in progress (launcher and systemd runtime boundary complete; recipe
and acceptance lifecycle ports remain).** Port in risk order, but do not stop
after read-only coverage. Rust owns the full behavior of each function: input validation,
policy decisions, bounded execution, privilege transitions, mutation,
destructive confirmation, structured status, redaction, and recovery.

1. **Complete for launcher and systemd surfaces.** Retargeted thin wrappers and
   unit callers to installed Rust binaries or the bounded `kyth-runtime`
   dispatcher; compatibility shims retain no policy, parsing, mutation, or
   success semantics.
2. **Complete for the migrated launcher set.** Ported read-only probes and
   deterministic report dispatch through the Rust runtime, with redacted,
   bounded subprocess output.
3. **Complete for the migrated launcher set.** Ported idempotent writers and
   session/system configuration, preserving reversible projections and explicit
   refresh-after-success semantics.
4. **Complete for the migrated launcher set.** Ported destructive and
   privileged workflows, including NTFS import/storage maintenance,
   scheduler configuration, distrobox/root action boundaries, full-update
   dispatch, Secure Boot entry points, boot verification, Greenboot state, and
   filesystem mutation. These workflows use fixed allowlists, bounded
   execution, and atomic writes.
5. **Remaining.** Port the 14 `ujust` recipe files to typed Rust recipe
   commands, then port boot/recovery and acceptance lifecycle functions with
   disposable-image tests. `kyth-vm-acceptance-guest run` remains intentionally
   queued until its destructive install/update/rollback matrix is implemented
   and observed; it must not be relabeled native by inventory metadata.

Every cutover must retain a rollback fixture until the corresponding
installed-image and promoted-image checks pass. A shell compatibility wrapper
may remain temporarily, but it must delegate to one bounded Rust operation and
must not retain policy, destructive execution, parsing, or success semantics.

## Phase 7 — Prove and retire shell authority

**Status: planned.** The final phase closes the gap between source ownership
and installed behavior.

- [ ] Run function-level parity tests through `unittest discover`, including
  success, refusal, malformed input, missing dependency, privilege failure,
  timeout, partial completion, and rollback paths.
- [ ] Validate the packaged image and both release channels with the actual
  Rust binaries, units, recipes, desktop launchers, and typed Hub commands.
- [ ] Exercise destructive paths on disposable disks/images and record the
  recovery evidence before deleting any shell fixture.
- [ ] Regenerate the runtime inventory and require zero queued runtime shell
  functions, zero unreviewed explicit exceptions, and zero shell-owned policy
  or execution authority in the supported image.
- [ ] Complete the post-cutover observation window, then delete superseded
  shell/Python fixtures and stale launch references in a separately reviewed
  cleanup change.

## Definition of done

- `active_python_entries` in the generated report reflects only Python that
  is reachable from a real launcher/unit/harness — no entry is active solely
  because of a path-prefix rule.
- Of the 41 currently-listed `python-runtime` launchers: 40 are ported to
  native Rust entry points (`NATIVE_BINARIES`) as of 2026-09-07. Only
  `kyth-vm-acceptance-guest` remains deferred to the Hub plan's post-cutover
  acceptance gate (see "Not a phase" above). The runtime ledger reaches zero
  only after that independently controlled gate has closed.
- Every port has a `unittest.TestCase`-based parity test collected by
  `python3 -m unittest discover -s tests -b`.
- Every runtime-relevant shell function is Rust-owned, including functions
  that read, write, mutate, destroy, recover, update, enroll Secure Boot, or
  cross a privilege boundary. Shell may remain only as declarative launch
  metadata or audited build-only assembly.
- The generated inventory has no queued runtime shell functions and no
  unreviewed `explicitly-not-ported` runtime entries. A `done-native` shell
  record names the Rust owner and the function-level parity evidence.
- The inventory and report are regenerated and pass `just validate` after
  each launcher moves, not batched at the end.
- CLAUDE.md's host-tuning convention no longer tells contributors to write
  new Python.
- `data-or-config` remains terminal non-code inventory, but every function
  reachable through `shell-orchestration` is closed by the shell phases above.

## Out of scope

- Live-media/VM/promoted-image acceptance remains a release gate, but it is
  also the required proof for the Rust implementation of destructive and
  recovery workflows. `kyth-vm-acceptance-guest`'s `run` lifecycle is no
  longer outside the implementation target; it is sequenced in Phase 6.
- A runtime entry may not remain in `NOT_PORTED` / `NOT_PORTED_PATHS` without
  the Phase 5 owner review. Existing entries must be resolved as Rust-owned,
  build-only/declarative, or a documented external interface with an explicit
  reason that Kyth cannot own the behavior.
- `src/kyth-welcome/` Python, which is already classified source-only/
  test-fixture and is not an installed runtime authority.
- Pure build-time image assembly and declarative metadata are not required to
  become Rust binaries when they contain no runtime policy or execution logic.
  They remain audited inventory entries and must not hide runtime behavior.
