# Kyth Hub migration finalization plan

## Goal

Retire the old Python/Qt System Hub as a user-facing application. The only
System Hub UI and command entry point should be the React frontend hosted by
the Tauri shell, with every read and action initiated through a typed Tauri
`invoke` command backed by Rust.

This does not require deleting every Python system service immediately. The
scope boundary is:

- **In scope:** the Python/PySide6 Hub UI, its launcher and routing, Hub-only
  Python bridges, direct Python subprocesses from Hub workflows, and all user
  visible Hub reads/actions.
- **Allowed transitional dependencies:** OS-owned services such as
  `kyth-guardian` and the update watcher may continue to own collection or
  privileged execution behind a typed Rust boundary while their replacements
  are implemented. They must not be launched as a second Hub UI or called
  through an untyped frontend bridge.
- **Strict no-Python interpretation:** if “all reads and actions” also means
  that no Python service may remain in the runtime path, complete the optional
  service-port work in Phase 5 before Phase 7.

## Current status — 2026-09-05

The React/Tauri shell is the primary implementation and its code-level build,
contract, SSR, Rust, and embedded-asset checks pass on the `testing` branch.
`kyth-welcome-launch` is Tauri-only and fails clearly when
`/usr/bin/kyth-hub-shell` is absent; the normal image no longer installs the
Python Hub package. The selected compatibility fixtures for the retired Hub
authorities were removed in P2. Transitional Python helpers unrelated to the
supported Hub have now been replaced at their installed entry points by Rust
binaries (post-update confidence, first-login app status, and Steam launcher
export); none is an active Hub action authority. The update watcher is also
the sole update notifier, so no Python/Qt tray process is installed or
autostarted.
The pushed cutover commit passed GitHub Validation workflow
`33665906864` (all four jobs, including the Hub shell and coverage/lint gates).
Local KVM/QEMU/SPICE acceptance has since passed against the locally built ISO,
including the installed Rust/Tauri Hub, every manifest-derived deep link,
single-instance forwarding, degraded dashboard behavior, Hub update probing,
and the privileged allowlist failure path. The local run was `install-only`;
update staging/rollback and promoted-image security drills remain separate open
gates. The remaining transitional Python service package is a technical
follow-up item, not the old Hub UI.

The current local P0 follow-up also closes the last static Hub-action routing
gap: Secure Boot enrollment is in the Rust `HubAction` allowlist, and contract
tests reject any static `RecipeButton` that is not represented by that enum.

The source parity record is [Kyth Hub Parity](../src/kyth-hub-web/PARITY.md),
and the Rust port boundary is [MIGRATION.md](../src/kyth-shared-rs/MIGRATION.md).

## Target architecture

```text
React page/component
        │ typed @tauri-apps/api invoke
        ▼
Tauri command handler (Rust)
        ├── kyth-shared-rs read/model/policy code
        ├── fixed, bounded OS argv where appropriate
        └── typed privileged service boundary for root operations
        ▼
validated result + running/complete/failed status
```

The webview must not import Python, spawn processes, accept arbitrary command
strings, or treat a launched process as proof that an operation completed.
Rust command handlers remain responsible for validation, allowlists, bounded
execution, redaction, and result semantics.

## Frozen Phase 0 inventory — 2026-09-01

The frozen legacy source was the deleted `page_registry.py`; it is retained in
history only. The replacement source of truth is `src/kyth-hub-web/src/data/`, with page
composition in `src/kyth-hub-web/src/pages/` and native registration in the
Tauri handler in `src/kyth-hub-web/src-tauri/src/main.rs`. The frontend
contract test verifies that every literal frontend `invoke` name is registered
by that handler.

| Legacy key(s) | React owner | Rust/Tauri owner | Result/error contract | Exception or note |
|---|---|---|---|---|
| `Welcome` | `Dashboard.tsx` | `dashboard.rs`, `main.rs`, `liveData.ts` | Reads degrade to `null`/empty state; actions return typed job status or an error | `kyth-guardian` still writes some backing state |
| `Play`, `Gaming`, `Performance`, `Compatibility`, `Controllers` | `Play.tsx` plus the four matching `*Section.tsx` components | `gaming.rs`, shared Rust gaming modules, `liveData.ts` | Cached reads are allowed; refresh/actions report bounded running/complete/failed state | Telemetry collection and some device mutation remain service-owned |
| `Apps`, `App Store`, `Work Setup` | `Apps.tsx`, `AppStoreSection.tsx`, `WorkSetupSection.tsx` | `main.rs`, `security.rs`, shared catalog modules, `liveData.ts` | Search/read failures show empty or unavailable state; installs/removals are polled jobs | Flatpak/AppImage and office workflows use fixed Rust command paths |
| `This PC`, `Guardian`, `Hardware`, `Plasma Wayland`, `Diagnostics`, `Repair`, `NVIDIA`, `Kernel`, `Channels`, `Just`, `Feedback` | `ThisPc.tsx` plus matching section components | `dashboard.rs`, `updates.rs`, `security.rs`, `main.rs`, shared Rust modules | Snapshot failures stay visible as degraded state; system-changing actions require confirmation and bounded result reporting | Legacy `Update` is re-homed to the dedicated React `/updates` page |
| `Move In`, `Move Files`, `Cloud Storage`, `Network Shares`, `VPN` | `MoveIn.tsx` plus the four matching section components | `main.rs`, `privilege.rs`, shared transfer/network modules, `liveData.ts` | Network/storage reads are nullable and refreshable; mutations use fixed commands or the typed privileged boundary | VPN/SAML and some network helpers remain transitional service-owned paths |
| Legacy `Update` | `Updates.tsx` / `UpdatesSection.tsx` | `updates.rs`, `dashboard.rs`, `liveData.ts` | Update checks and jobs expose explicit running/complete/failed state; rollback/restart guidance is data-backed | Extended watcher parity and service-level validation remain tracked in Phase 5 |

All page and section rows have a React owner. The remaining unowned work is
runtime qualification and security/rollback hardening, not an untracked page:
the open exceptions are listed in the register below and map to Phases 2, 6,
and 7.

### Production reference inventory

| Reference | Current role | Classification | Retirement action |
|---|---|---|---|
| `src/kyth-welcome/kyth-welcome-launch` | Starts Tauri only | Stable compatibility-named launcher | Retain while desktop/notification callers migrate to a neutral name |
| `build_files/scripts/branding/23-kyth-helper-ctx-installs.sh` and `Dockerfile` | Package Python Hub, desktop metadata, and Tauri binary | Obsolete UI packaging plus transitional metadata generation | Stop installing the UI; move route/search generation to React/Rust-owned data |
| `Justfile` `setup-hub`/`run-hub` and former PySide6 smoke job | Developer/test entry points | Tauri tooling; old smoke removed | Keep Tauri dev/test commands |
| `src/kyth-shared-rs/src/update_watcher_bin.rs` and `kyth-update-watcher.service` | Root update scheduling, registry comparison, staging, status, firmware staging, and notifications | Native Rust service authority | Keep service packaging; installed-image acceptance remains waived |
| `build_files/just/kyth/dualboot.just` | Opens Hub from a recipe | Legacy launch reference | Call `kyth-welcome-launch --page ...` or a typed Tauri route |
| `kyth-probe.service`, `kyth-guardian.service` | Populate probe/Guardian state consumed by Rust reads | Native Rust probe and extended Guardian sweep; optional local-model investigation | Model assets are optional and missing assets are reported as degraded; legacy Python source is fixture-only |
| `kyth-update-watcher.service` | Root update scheduling, registry comparison, staging, status, firmware staging, and notifications | Native Rust with free-space, lock, retry, and session/network gates | Installed-image acceptance remains waived; legacy Python source is fixture-only |
| `src/kyth-hub-web/src-tauri/src/commands/vpn.rs` | VPN profile editor, openconnect worker, and SAML browser | Native Tauri/Rust workflow; standalone Python/Qt client removed in P2 | Keep typed command and secure callback handling; provider qualification remains operational follow-up |
| `src/kyth-shared-rs/src/network_share_bin.rs` behind `kyth-privileged.service` | Root-owned network-share add/remove and systemd mount setup | Typed Rust request boundary with a native Rust helper executor | Keep fixed operations, credential isolation, and audit behavior |
| `src/kyth-shared-rs/src/privileged_bin.rs` and `kyth-privileged.service` | Root-owned local socket for Hub-authorized system mutations | Native Rust daemon | Keep the fixed operation allowlist, peer-credential gate, bounded execution, stdin-only secrets, and audit behavior |
| `kyth-telem.service` and `src/kyth-shared-rs/src/telemetry_writer_bin.rs` | MangoHud CSV ingestion and telemetry SQLite writes | Feature-gated native Rust writer | Native build/schema/CSV parity is covered by Rust tests; installed-image acceptance remains waived |
| `kyth-windows-verify` and `ujust windows-verify` | Standalone tunable/recipe for a migration readiness report | Outside the Hub action path; the Hub uses native `migration_readiness` instead | Retire the Python tunable wrapper with the remaining compatibility fixtures after the observation window |
| `kyth-vpn-status` and other compatibility helpers | Independent system utilities | Out of Hub UI scope unless their output/action is exposed by Hub | Audit ownership when closing the corresponding Hub workflow; `kyth-ai-perfd` is now native Rust |

## Phases

### Phase 0 — Freeze the migration contract and inventory

Status: complete (2026-09-01).

- [x] Frozen destination/section inventory and page ownership matrix recorded
      above.
- [x] Mapped each legacy page family to its React owner, Rust/Tauri owner, and
      result/error contract.
- [x] Inventoried production launch, packaging, test, and service references
      to the Python/Qt Hub.
- [x] Classified remaining Python dependencies as compatibility fallback,
      obsolete UI packaging/tooling, transitional backing service, or out of
      Hub UI scope.
- [x] CI guard verifies every frontend `invoke` is registered and rejects
      Python/Qt UI imports, direct frontend process execution, and generic
      command bridge names.

Exit criteria met: the matrix has no unowned page or frontend bridge; every
remaining runtime exception has an owner and a retirement phase. The matrix
does not claim runtime acceptance or service-port completion; those remain
explicitly open in later phases.

### Phase 1 — Close Tauri/Rust feature parity

Status: complete for the code-level/native surface (2026-09-01); installed-image
acceptance remains in Phase 2 and strict service ownership remains in Phase 5.

- [x] Keep all user-facing destinations and their sections live through the React/Tauri
  surface, including Guardian, Hardware, Apps, Gaming, Move In, and Updates.
- [x] Finish the currently identified safe/native parity gaps in `PARITY.md`;
  high-risk collector/service ownership remains explicitly tracked in Phase 5.
- [x] Ensure every read returns an honest degraded state when a probe/service is
  absent, stale, or unavailable.
- [x] Ensure every action has a typed Rust command, fixed allowlist/argv policy,
      bounded timeout, structured job status, and refresh-after-success behavior.
- [x] Ensure every static recipe button maps to the closed Rust `HubAction`
      enum; unknown action names fail at Tauri deserialization.
- [x] Keep secret-bearing values out of React state, status text, audit details,
  process arguments, URLs, and logs.
- [x] Keep focused Tauri command tests in CI; the canonical Hub build script now
  runs the Tauri unit-test harness as well as compiling the crate.

Exit criteria met for the native surface: all currently exposed rows are backed
by a Rust command or an explicitly approved transitional service boundary, with
contract and failure tests. Installed-image behavior and Python-owned collector
replacement are intentionally not certified by this phase.

### Phase 2 — Prove installed-image runtime behavior

Status: local install-only acceptance complete (2026-09-05); full lifecycle and
promoted-image gates remain open.

- Build a testing ISO containing `/usr/bin/kyth-hub-shell` and WebKitGTK
  runtime dependencies.
- Extend the VM acceptance guest or add a dedicated Hub acceptance probe to
  verify the real installed binary, not only the build artifact.
- Exercise every destination and section deep-link, including the desktop
  entries and `--page` aliases generated from the route inventory.
- Launch twice and verify the existing Tauri window is focused and receives
  the requested page.
- Test dashboard and section degraded states with probe services stopped or
  data absent.
- Exercise safe reads and representative actions for Guardian, Hardware,
  App Store, Gaming, Move In, and Updates; verify bounded failures and
  truthful completion status.
- Test update check, staging, rollback, restart guidance, and post-reboot
  state on both testing and stable images.
- Run the same acceptance with representative network, Flatpak, storage,
  and update states; record qualification artifacts and screenshots. The local
  install-only evidence is under
  `output/live-iso/vm-acceptance-final/`; it does not certify the update and
  rollback lifecycle.

Exit criteria: a disposable VM acceptance report passes on both channels and
contains no false-success, routing, startup, secret-leak, or single-instance
finding.

### Phase 3 — Cut over all launch paths

Status: complete (2026-09-02); normal launch references and Hub packaging are
Tauri-only. Installed-image/user acceptance was waived for this cutover.

- [x] Route normal notification and recipe launches through
  `/usr/bin/kyth-welcome-launch`.
- [x] Remove the Python/Qt startup path from `Justfile` `run-hub`; local
  development now starts the React/Tauri shell.
- [x] Make `kyth-welcome-launch` a Tauri-only launcher (2026-09-02); it fails
  clearly if `/usr/bin/kyth-hub-shell` is not installed.
- [x] Remove the legacy `/usr/bin/kyth-welcome` Python UI/package from the
  normal image (2026-09-02). No silent runtime compatibility path remains.
- [x] Update notifications and the dual-boot recipe to target the Tauri
  launcher; existing autostart, Kickoff, and KRunner entries already invoke
  the wrapper and preserve the route contract.
- [x] Replace Python-generated Hub search/desktop metadata with the shared
  React route manifest and a packaging-only generator (2026-09-02).

Exit criteria: a clean image has one System Hub desktop entry and one launch
implementation; no normal user path starts `kyth-welcome`.

### Phase 4 — Retire the Python/Qt UI package

Status: complete (2026-09-02); the Python/Qt Hub UI source, entry point,
page registry, KRunner generator, UI-only tests, Qt smoke CI job, and the
standalone Python/Qt VPN UI are gone. Remaining Python service modules are
explicitly transitional and are not the Hub UI.

- [x] Stop installing the Python Hub UI package and its Python console entry
  point; retain only the Tauri launcher desktop metadata.
- [x] Delete the `src/kyth-welcome/kyth_welcome` UI pages, page registry,
  Qt-only Hub shell, and UI-only tests after replacement coverage was recorded.
- [x] Remove the CI-only PySide6 Hub smoke job and obsolete UI quality checks.
- [x] Keep only transitional Python service modules needed by independent
  helpers; strict Hub service ownership is complete and remaining helper
  cleanup is tracked in Phase 7.
- [x] Update support, developer, architecture, and parity documentation so
  the old UI is not presented as a supported fallback.

Exit criteria met: the image contains no Python/Qt System Hub UI, and repository
search finds no normal launch, packaging, or test path for the retired UI.

### Phase 5 — Replace transitional Python service authorities (strict mode)

Status: implementation complete for the listed P1 authorities (2026-09-02);
P2 removed their obsolete Python/build compatibility fixtures. Installed-image
acceptance is intentionally waived. The probe collector, extended Guardian
service CLI, update watcher, telemetry writer, privileged socket daemon, and
network-share executor are native Rust binaries, and the image no longer
installs their Python entry points.
Model investigation is bounded and optional: without a valid local model and
`llama-cli`, the native service reports an explicit degraded model state.
VPN/SAML, the privileged socket daemon, and network-share execution are
Rust-owned at their action boundaries; the former standalone Python/Qt VPN and
root-boundary fixtures were removed in P2.
The retained `src/kyth-welcome` service package is source-only compatibility
material and is not installed in the supported image; its active privilege
boundary is the native Rust service and Tauri command layer.

- [x] Port the live probe collector and cache writer behind `kyth-probe.service`
  to the shared Rust crate, with bounded commands, atomic writes, and null-on-
  failure section semantics.
- [x] Replace the installed Guardian entry point with the native Rust service
  CLI, preserving the JSON state/history contract, deterministic core probes,
  fixed recipe policy, bounded executors, and post-repair verification.
- [x] Port the extended Guardian probes and bounded model-assisted inference
  parity; keep unavailable model assets explicit rather than reporting false
  health. Native tests cover the probe/service contract (2026-09-02).
- [x] Replace the installed update-watcher entry point with the native Rust
  oneshot, preserving safety gates, rollout/quarantine checks, status-file
  persistence, bounded `bootc upgrade`, and notification routing to the Tauri
  launcher.
- [x] Complete extended update-watcher parity and service-level validation,
  including firmware staging, lock/retryable-status handling, `/sysroot`
  free-space protection, and network/session safety conditions (2026-09-02).
- [x] Port the VPN/SAML profile editor, openconnect worker, and browser flow to
  Rust/Tauri; keep credentials, cookies, and SAML responses out of frontend
  state and logs. The legacy standalone client and Python/Qt source were
  removed (2026-09-02).
- [x] Port the root network-share helper behind the existing fixed privileged
  socket protocol; preserve credential isolation, mount-unit ownership, and
  audit behavior. The installed helper is native Rust (2026-09-02).
- [x] Replace the Python root-owned privileged socket daemon with the native
  Rust `kyth-privileged` binary, preserving peer credentials, the fixed
  operation allowlist, bounded execution, BitLocker stdin handling, and audit
  behavior. The obsolete Python daemon fixture was removed in P2 (2026-09-02).
- [x] Enable the Rust telemetry writer for the image, prove schema/CSV-output
  parity with the former Python fixture, and remove the Python writer from the
  active path (2026-09-02). The obsolete fixture was removed in P2;
  installed-image acceptance is waived.
- [x] Reconcile the remaining Hub-facing Python workflows and update the
  parity, migration, architecture, checklist, and developer guidance with
  explicit ownership and priority entries. The Hub's Windows migration check
  now uses the native `migration_readiness` read; the standalone Python
  `kyth-windows-verify` tunable remains outside the Hub path (2026-09-02).
- [x] Run native service-level parity tests before removing each Python
  authority; shared Rust tests now cover telemetry ingestion, Guardian policy/
  probe contracts, firmware staging helpers, and update retry/status behavior
  (2026-09-02). Installed-image acceptance is tracked separately and waived.
- [x] Replace the installed post-update confidence, first-login app-status,
  and Steam launcher-export Python entry points with Rust binaries that reuse
  the shared diagnostic, firstboot, and desktop-shortcut contracts (2026-09-02).
- [x] Replace the packaging-time Python KRunner generator with the native Rust
  `kyth-hub-desktop-entries` binary (2026-09-02).

Exit criteria: every Hub-facing read/action has Rust-owned semantics and an
approved non-UI service boundary; no Hub workflow depends on a Python module
without an explicit, time-bounded exception.

### Phase 6 — Security, observability, and rollback hardening

Status: code-level hardening and local install-only acceptance complete
(2026-09-05); promoted-image security and rollback drills remain open.

- [x] Audit every exposed Tauri command for capability scope, argument
  validation, fixed allowlisting, bounded execution, and output redaction.
  The shell now declares an explicit minimal `core:default` capability, and
  shared bounded-output handling redacts common secret fields.
- [x] Verify BitLocker, VPN, and network-share secrets use stdin/local
  boundaries and receive defense-in-depth redaction before UI status or audit
  output. SAML redirects are restricted to validated HTTPS destinations without
  credentials or fragments.
- [x] Use telemetry-free local diagnostics for shell startup, command failure,
  service absence, and update lifecycle through the existing native probe,
  runtime-check, recovery, and update-health commands. Support-safe capture and
  rollback procedures are documented in
  [Kyth Hub Rust rollback runbook](kyth-hub-rust-rollback-runbook.md).
- [x] Define release-blocking signals and a removal/revert procedure for failed
  Tauri launch, broken deep link, false-success action, or leaked secret in the
  rollback runbook.
- [ ] Run the security review and rollback drill on the exact promoted image;
      the local install-only run does not cover this gate.

Code-level exit criteria are met: native tests cover secret redaction, bounded
execution, and SAML URL rejection. Full Phase 6 exit still requires an
installed-image drill on the promoted digest.

Validation evidence (2026-09-03): 501 shared Rust tests with the
telemetry-writer feature (498 default) and 6 Tauri tests pass;
the frontend command-contract test, Hub SSR smoke test, Tauri release build,
frontend production build, `git diff --check`, and fast repository validation
also pass. The installed-image security/rollback drill was not run.

### Phase 7 — Declare completion and monitor

Status: code cleanup in progress (2026-09-05); direct Python-backed Just recipe
parsing, the installed AI performance daemon, the listed diagnostic/game entry
points, 49 sysctl-backed tunable entries, and all 45 module-specific tunable
entries are now native Rust/non-Python. The compatibility dispatcher remains
only as a rollback fixture for older images.

- Publish the final parity matrix, VM qualification reports, and release
  notes.
- Promote only an image that passes the complete runtime and security gates.
- Monitor the first release window for startup, deep-link, update, and action
  failures before deleting compatibility artifacts permanently.
- After the observation window, remove dead migration code, stale docs, and
  compatibility aliases.
- [x] Replace the installed Python `kyth-ai-perfd` launcher with the native
  Rust daemon using the shared performance-policy, gaming-activity, and
  hardware-policy modules (2026-09-02).
- [x] Remove Python JSON parsing from the probe, OS update, JetBrains Toolbox,
  LSFG-VK, and runtime perf-gate Just recipes (2026-09-02); use the native
  probe/perf-gate binaries or `jq` for data-only JSON extraction.
- [x] Replace the installed Python `kyth-health-check`, `kyth-resume-check`,
  `kyth-nvidia-status`, `kyth-controller-check`, `kyth-game-boost`, and
  `kyth-doctor` entry points with bounded native Rust binaries (2026-09-03).
- [x] Port all 49 sysctl-backed and all 45 module-specific entries of the
      indirect recipe executor to
      native `kyth-tunable-rs`; package-time symlink selection is derived from the
      Rust registry and every registry entry selects the native dispatcher
      (2026-09-03).
- [x] Port `mimalloc`, `mimalloc-run`, `sccache`, `shader-cache-size`, and
      `wine-sync` module-specific writers to native `kyth-tunable-rs`
      (2026-09-03); generated environment and service files remain reversible.
- [x] Port `kwin-latency` module-specific writer to native `kyth-tunable-rs`
      (2026-09-03); KWin drop-in and environment projections are reversible.
- [x] Port `distrobox-cache`, `flatpak-prefetch`, `flatpak-trim`, `readahead`, and `trim-tune` writers to
      native `kyth-tunable-rs` (2026-09-03); generated unit/timer files remain
      reversible and service activation stays caller-owned.
- [x] Port the final module-specific tunable writer to native Rust and make all
      94 registry entries resolve to `kyth-tunable-rs`; retain the compatibility
      dispatcher only as a rollback fixture for older images.
- Continue the repository-wide runtime ownership work in the
  [Rust migration completion plan](rust-migration-completion-plan.md), which
  now includes non-Hub shell functions and requires Rust ownership for every
  runtime operation that can be implemented natively, including destructive
  and privileged paths. This plan remains the source of truth for Hub-specific
  acceptance, observation, and rollback gates.

Exit criteria: Tauri/Rust is the only supported System Hub implementation and
the old Python/Qt Hub is absent from the production image and supported code
paths.

## Open-item register

| Priority | Open item | Blocking phase |
|---|---|---|
| P0 | Complete installed-image Hub acceptance on testing and stable images. | 2 (local install-only pass; testing/stable and lifecycle coverage remain open) |
| P0 | Remove or isolate the `kyth-welcome-launch` Python fallback. | 3 (complete 2026-09-02) |
| P0 | Replace update-notification launches of `kyth-welcome --page updates` (normal path now uses the wrapper). | 3 (complete 2026-09-02; image verification waived) |
| P0 | Prove second-launch focus and page forwarding in a real session. | 2 (local pass; promoted-image repeat remains open) |
| P0 | Validate privileged actions, secret redaction, and bounded failures on the image. | 2, 6 (local pass; promoted-image repeat remains open) |
| P0 | Run the exact-image Phase 6 security review and rollback drill. | 6 (open) |
| P1 | Replace Python-generated runtime route/search metadata. | 3, 4 (complete 2026-09-02) |
| P1 | Remove `Justfile run-hub` Python/Qt startup behavior. | 3 (complete) |
| P1 | Add Tauri command-unit tests to CI instead of compile-only coverage. | 1 (complete) |
| P1 | Replace installed Python post-update, firstboot-status, and Steam-export helpers with native binaries. | 5 (complete 2026-09-02) |
| P1 | Replace the packaging-time Python Hub desktop-entry generator. | 3, 4 (complete 2026-09-02) |
| P0 | Replace the Python/PySide6 VPN/SAML app reached by `open_vpn_app`; expose the complete flow through Rust/Tauri. | 5 (complete 2026-09-02; standalone client and source fixture removed) |
| P0 | Replace the Python root network-share helper behind the typed privileged socket; preserve fixed operations and credential isolation. | 5 (complete 2026-09-02; native binary installed and Python fixture removed) |
| P1 | Enable and validate the Rust telemetry writer; remove the active Python `kyth-telem` daemon. | 5 (complete 2026-09-02; source fixture removed; image acceptance waived) |
| P1 | Complete extended Guardian/model parity and update-watcher lock, firmware, retry, and session/network gates. | 5 (complete 2026-09-02; image acceptance waived) |
| P1 | Reconcile any other Hub-facing Python authorities listed in `MIGRATION.md` before declaring strict mode complete. | 5 (complete 2026-09-02; native Rust authorities installed) |
| P2 | Remove remaining compatibility service fixtures and stale support references after strict-mode cutover. | 7 (complete 2026-09-02; native helper entry points, dead fixtures, and stale VPN launch references removed) |
| P2 | Port remaining indirect recipe executors (`kyth-tunable`) and remove its compatibility module. | 7 (all 49 sysctl and all 45 module-specific entries complete 2026-09-03; compatibility retained only as a rollback fixture) |
| P2 | Run a post-cutover observation window before deleting compatibility code. | 7 (open; begins after promoted-image acceptance) |

## Definition of done

The migration is final only when all of the following are true:

- The production image starts `/usr/bin/kyth-hub-shell` for every supported
  System Hub launch path.
- The Python/Qt Hub is not installed or reachable through a normal user path.
- Every Hub read and action begins as a registered typed Tauri/Rust command.
- No frontend path launches Python, Qt, or an arbitrary command string.
- All user-facing destinations, deep links, single-instance behavior, degraded states,
  destructive confirmations, secrets, updates, rollback, and representative
  actions pass installed-image acceptance.
- Transitional service exceptions, if any, are documented with owners and
  retirement dates; strict mode has no unresolved installed Python authority.
- CI runs build, contract, Rust unit, security, and installed-image acceptance
  gates for the release commit.
