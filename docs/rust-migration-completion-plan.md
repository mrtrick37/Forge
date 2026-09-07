# Rust Migration Completion Plan

**Status (2026-09-07):** Hub, VPN/SAML, privileged socket, network-share,
telemetry, Guardian, probe, update-watcher, the installer backend, and the
tunable/sysctl registry are already Rust-owned at their installed entry
points (see
[Kyth Hub migration finalization plan](kyth-hub-migration-finalization-plan.md)
and [Installer Migration Plan](installer-migration-plan.md)). What remains is
a smaller, previously uncatalogued surface: 41 standalone launcher scripts
under `build_files/kyth-*` that `ujust` recipes and systemd units invoke
directly. Of those 41, **40 are ported and committed**. The remaining
`kyth-vm-acceptance-guest` launcher is deferred to an existing Hub-plan
release gate. `kyth-exe-handler` is now a packaged native Rust launcher with
its user interface in the existing **Tauri/React Hub**. Phase 3 stays blocked
on the Hub plan's own post-cutover observation window (not yet started, not
agent-schedulable).

**Scope:** This plan completes the **Python** runtime authority — the 41
`python-runtime` launchers plus the classifier fix that makes the
`python-shared-package` count trustworthy. It does not cover the non-Python
`shell-orchestration` surface; that surface is enumerated below and
deliberately deferred pending its own triage, not silently dropped. This plan
supersedes no existing plan — it is additive to the Hub and installer plans,
which already cover their own surfaces and already declare Python retired
there except for deferred VM/image-acceptance gates.

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

## The non-Python remainder

The report's `active_by_authority` also lists `data-or-config: 8` and
`shell-orchestration: 171`. Neither is Python, and neither is in this plan's
scope, but both are enumerated here so "all remaining items" isn't read as
silently ignoring them.

- **`data-or-config` (8 entries):** `.desktop`, `.rules`, `.toml`, `.menu`,
  and `.directory` files (e.g. `kyth-sched-profiles.toml`,
  `kyth-web-apps.directory`). These are data, not code — there is nothing to
  port. The classifier marking them `queued` rather than a terminal
  "not applicable" state is itself a small Phase 0 line item (see below), not
  a migration task.
- **`shell-orchestration` (171 entries):** roughly 97 are `.just` recipe
  files, `.service`/`.timer`/`.path` systemd units, and `.desktop` glue —
  orchestration, not implementation. `grep -rnE "python3?\b|kyth_shared"
  build_files/just/` returns zero matches, so the shipped `ujust` command
  surface specifically is already Python-free; those recipes call out to the
  41 launchers this plan tracks and complete automatically as those launchers
  move. (That check is scoped to `build_files/just/` — it is not a claim that
  all shell in the repo is Python-free; `vm-acceptance.sh` above is a
  counterexample.) The remaining ~74 are genuine bash scripts carrying real
  logic — `kyth-boot-verify`, `kyth-enroll-mok`, `kyth-mok-rotate`,
  `kyth-power-arbiter`, `kyth-perf-gate`, `kyth-storage-gate`, the
  `kyth-greenboot-*` trio, and similar. Their `queued` status in the inventory
  is a classifier default ("not recognized as a native shim"), not a verified
  migration verdict — the same field also marks `kyth-aio-max` and
  `kyth-ananicy` (both bash shims over the Rust tunable binary) as
  `shell-orchestration` + `done-native`, so `queued` here doesn't distinguish
  real logic from glue. This plan does not commit to porting the 74; it names
  them as a **deferred, undecided surface** requiring a triage pass (does this
  script contain logic, or is it a shim/unit-wrapper?) before any porting
  commitment is made. Several of them — `kyth-enroll-mok`, `kyth-mok-rotate`,
  `kyth-boot-verify`, and the `kyth-greenboot-*` group — touch Secure Boot
  enrollment and boot-health gating, so if triage does turn any of them into a
  migration target, they'd need the same parity-fixture discipline as
  Phase 2, not a casual port.

## Decision

1. Extend the classifier so `python-shared-package` splits into two
   observable states: modules reachable — statically, via a documented
   dynamic-dispatch table, or via direct shell-harness invocation (see the
   `qualification.py` example above) — from an active launcher/unit/harness,
   and modules retained only as rollback/parity fixtures for an
   already-completed Rust cutover. Only the first group counts toward
   `active_python`.
2. Treat the 41 `python-runtime` launcher entries as the real migration
   ledger. Port them in risk order using the same taxonomy the checker
   already encodes (`READ_ONLY_NAMES` → `WRITER_NAMES` → `DAEMON_NAMES` →
   privileged), rather than inventing a new ordering.
3. A launcher is "done" only when: the Rust replacement exists and is wired
   into the packaged image, the launcher name is added to `NATIVE_BINARIES`
   in `check-runtime-migration-inventory.py`, the inventory and report are
   regenerated (`--generate`) and pass `just validate`, and are committed, and
   tests are added as `unittest.TestCase` subclasses under `tests/` (CI runs
   `python3 -m unittest discover -s tests -b`; plain pytest-style
   function tests are silently never collected by that discovery — see the
   Phase 0 note below, this bit a prior task in this repo).
4. `NOT_PORTED` / `NOT_PORTED_PATHS` in the checker (`kyth-default-flatpaks`,
   `kyth-flathub-setup`, `kyth-local-bin-migrate`, `rclone@`, `scx_loader`,
   the welcome privileged fixture) remain deliberate exceptions. "All
   remaining items" in this plan means all Python items not already on that
   list; overturning one of those exclusions, or committing to port the 74
   bash scripts named above, is a separate, explicit decision this plan does
   not make.
5. Once Phase 1 substantially lands, flip the CLAUDE.md guidance that tells
   contributors to add new host-tuning logic as a small Python module in
   `kyth_shared` — otherwise every finished phase is undermined by the next
   feature landing in Python by convention.

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
- The inventory and report are regenerated and pass `just validate` after
  each launcher moves, not batched at the end.
- CLAUDE.md's host-tuning convention no longer tells contributors to write
  new Python.
- This plan does not claim to close out `shell-orchestration` or
  `data-or-config` — see "The non-Python remainder" above. Closing those out
  is a separate, not-yet-scoped decision.

## Out of scope

- Live-media/VM/promoted-image acceptance for the Hub and installer,
  including `kyth-vm-acceptance-guest`'s `run` lifecycle — already tracked as
  an open release-gate item in the Hub finalization plan, not code-migration
  work this plan adds to.
- Anything in `NOT_PORTED` / `NOT_PORTED_PATHS` unless a separate decision
  overturns a specific exception.
- `src/kyth-welcome/` Python, which is already classified source-only/
  test-fixture and is not an installed runtime authority.
- The ~74 non-glue bash scripts under `shell-orchestration` (see "The
  non-Python remainder"). Naming them here is not a commitment to port them;
  it's a record that they were looked at and deliberately left for a future,
  separately-scoped decision.
