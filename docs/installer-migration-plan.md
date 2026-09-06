# Installer Migration Plan

**Status:** Native Rust installer backend ownership is complete for the
supported runtime; full VM/live-media acceptance remains intentionally deferred
**Scope:** Migrate the KythOS installer to the React/Tauri style used by the System Hub.

## Context

At the migration baseline, the installer had a production React/Tauri client,
a native Slint recovery client, a root-owned Rust transport daemon, and a
typed Rust execution helper. The supported runtime has now completed the
backend cutover: Rust owns discovery, partitioning, filesystem resize and
mount orchestration, bootc deployment, configuration, account creation,
Secure Boot setup, progress, cancellation, transaction recovery, and Rescue.
The former Python implementation remains only as source fixture material for
parity tests and is not installed in the supported image.

The migration must preserve those safety properties. A UI rewrite and a
Python-to-Rust rewrite should not happen simultaneously: that would make disk
and boot failures difficult to distinguish from frontend regressions.

## Decision

Migrate in layers:

1. Keep the frozen logical HTTP/SSE contract while the React client is hosted
   in an unprivileged Tauri shell.
2. Keep the root-owned Rust Unix-socket daemon as the only user-visible
   privileged boundary; it invokes only fixed typed Rust helper operations.
3. Port backend components selectively to Rust after behavioral parity is
   established, with storage and recovery gates before every destructive
   cutover.
4. Remove the Python installer package and root Python launcher from the
   supported image; destructive VM/live-media acceptance remains a separate
   release gate and is not part of this code-level cutover.

The Tauri process must not run as root and must not gain a generic command,
filesystem, or disk-writing bridge. The privileged service remains the trust
boundary.

```text
Live-session user
    |
    v
React + Tauri installer shell
    | typed IPC over Unix socket
    v
kyth-installerd (root-owned)
    |
    +-- partition journal and transaction state
    +-- bootc / filesystem / mount operations
    +-- Python parity fixtures (source-only, not packaged)
```

## Migration phases

### 0. Freeze the API contract

Document the current routes and events from `src/kyth-installer/kyth_installer/server.py`,
`post_routes.py`, `context.py`, and `webui/install-flow.js`:

- request and response schemas
- lifecycle states and valid transitions
- event types, ordering, and reconnect behavior
- cancellation semantics
- secret handling
- read-only versus destructive operations
- transaction recovery rules

This contract is the compatibility target for both implementations.

### 1. React compatibility frontend with the existing backend (historical)

Create `src/kyth-installer-web/` with typed services and page components:

- Welcome
- Disk and installation mode
- Kernel
- Configuration
- Review
- Install/progress
- Rescue

Keep the Python HTTP server and SSE stream unchanged initially. The milestone
is identical installer behavior with a typed React state model and component
tests. This client is compatibility-only; the Rust/Slint client is the
production UI.

### 2. Rust/Slint production UI and Tauri compatibility shell (complete)

Create `src/kyth-installer-web/src-tauri/`, following the System Hub shell's
build and single-instance patterns, with these differences:

- run as the live-session user, never root
- do not use `--no-sandbox`
- embed production assets
- expose no unrestricted command or filesystem bridge
- preserve startup routing and one-instance behavior

The original milestone used the loopback Python service as a development
fixture. The completed image uses the Rust shell and root-owned Rust daemon;
the loopback service and Chromium path remain source-only fixture material.

### 3. Unix-socket privileged service

Replace loopback HTTP with a root-owned Unix socket once the Tauri frontend is
stable. Use socket ownership/permissions and peer credentials, retaining a
one-time session token as defense in depth.

Read-only commands:

- disks, partitions, free space, and filesystem options
- locale, timezone, and keymap lists
- source-image status
- transaction and rescue state

Mutating commands:

- create, delete, resize, format, and mount-point operations
- partition journal commit/rollback
- start and cancel installation
- reboot
- copy rescue logs

All validation remains server-side; the UI is never trusted.

### 4. Selective Rust backend migration

Port in this order:

1. Pure request and install-plan validation
2. Disk and partition discovery
3. Partition journal model and serialization
4. Transaction state and recovery guidance
5. Streaming command runner and cancellation
6. Mount lifecycle management
7. bootc installation and target configuration
8. Secure Boot/MOK handling

Retain the Python implementation behind a compatibility adapter until each
subsystem has equivalent tests.

Highest-risk source areas are:

- `partition_ops_journal.py`
- `storage_guard.py`
- `plan_validate.py`
- `recovery.py`
- `phases/storage.py`
- `phases/finalize.py`

## Historical safety gates and deferred acceptance

Before the native cutover, the original plan required:

- React build, typecheck, and embedded-asset checks pass.
- All existing installer tests remain green.
- The Tauri shell starts in a live ISO without a development server.
- No unrestricted command execution is available from the UI.
- Passwords never appear in URLs, logs, process arguments, or persistent state.
- Cancellation works during every long-running phase.
- Recovery is tested after partition changes, filesystem resize, image deploy,
  and final configuration.
- Wipe, alongside, resize, free-space, and manual modes pass VM tests.
- Rescue mode remains read-only and diagnoses interrupted installs.

## Suggested work breakdown

1. API contract and frontend state model — 1–2 days
2. React WebUI rewrite — 3–5 days
3. Tauri shell and live-image packaging — 2–4 days
4. Unix-socket service — 3–5 days
5. Rust logic ports and parity tests — 1–2 weeks
6. VM destructive-path acceptance testing — 3–5 days
7. Remove Chromium/Python UI launcher — 1–2 days after parity

## Decisions

- The native Rust daemon owns the Unix socket and all packaged installer
  operations. It does not proxy to the Python implementation; Python remains
  available only for source-level parity fixtures.
- Calamares remains an optional, separately scoped build path until a release
  owner decides whether to retire it; it is not part of the 15-item Rust
  backend migration ledger.
- The logical SSE contract is preserved over the Unix-socket HTTP framing so
  reconnect, event IDs, cancellation, and terminal state remain compatible.

## Current progress

- P0 — Migration control plane: complete (2026-09-05). The generated
  [runtime migration report](../build_files/config/runtime-migration-report.json)
  distinguishes installed authorities from source-only fixtures, and the
  repository validation gate rejects untyped frontend process/filesystem
  bridges and unclassified active Python paths.
- Phase 0 — API contract: complete. See
  [`installer-api-contract.md`](installer-api-contract.md).
- Phase 1 — React frontend: complete as the embedded Tauri client. Typecheck,
  production build, API decoding, request guards, manual-error handling, and
  contract smoke tests pass. The Slint client remains an explicit recovery
  fallback, not a second backend authority.
- Phase 2/3 — Tauri shell and Rust Unix-socket boundary: implemented and
  packaged. The Rust daemon owns the packaged socket and invokes only native
  Rust phase/helper operations.
- Local acceptance — the live ISO, install-only path, reboot into the installed
  system, and installed Hub checks have not been run in KVM/QEMU/SPICE and
  remain release-gate work. All installer modes, cancellation, power-loss
  recovery, and update/rollback lifecycle acceptance remain open as well.
- Host-side acceptance harness checks — the unittest-based VM acceptance
  contract tests pass (68 tests), and the fast repository validation gate
  passes. A destructive VM run is still pending because this host has neither
  a built live ISO nor `/dev/kvm`; the acceptance harness must not silently
  fall back to software emulation.

## Review findings (2026-09-05)

The code-level migration is complete for the React/Tauri client, native Rust
Unix-socket boundary, and native backend execution path. The Python installer
tree is no longer an installed authority; it remains source-only fixture
material for parity tests. Live-media qualification and the broader
disposable-VM destructive-path matrix remain release work.

## Deferred release continuation

The code-level migration is complete. The following work remains deliberately
outside this loop and is release/acceptance work:

1. Build the live ISO with the native client packaged.
2. Exercise all install modes in disposable VMs.
3. Test cancellation and power-loss recovery at every durable phase.
4. Reconfirm the packaged image contains only the native launcher and service
   in the built artifact.

### Historical implementation scaffolding (superseded)

The following section records the original scaffold plan for historical
traceability. It is complete and must not be read as an open implementation
item. The `src/kyth-installer-web/src-tauri/` shell now provides:

- an unprivileged, production-asset-only Tauri configuration;
- the minimum capabilities needed to host the application (no shell, generic
  filesystem, process, or disk APIs);
- a narrow bootstrap transport to the existing loopback service, preserving
  the one-use bootstrap and HttpOnly-cookie authentication flow;
- single-instance behavior and clean backend-child shutdown, without copying
  the Hub's system-action commands;
- unit tests for startup argument parsing and an embedded-asset smoke check;
- image packaging additions that build the shell but do not switch the live
  launcher until live-ISO validation succeeds.

Phase 3 and the selective backend ports are also complete; the daemon now owns
the socket protocol and native installer operations.

## Remaining plan

### Acceptance scope decision

Per the current migration execution request, the live-media and destructive VM
acceptance stages are deferred. This means the code-level cutover is validated
by host-independent checks only; it does not claim destructive install or
hardware acceptance. The supported image now uses the native Rust authority,
while deferred acceptance remains a release gate.

### Phase 2 — Tauri installer shell (complete)

- Add an unprivileged Tauri shell around the React build. **Done:** `src/kyth-installer-web/src-tauri/` embeds the production assets and exposes only the fixed backend connection/token handoff.
- Embed production assets; do not use a development server or `--no-sandbox`.
- Preserve single-instance/startup routing behavior from System Hub. **Done:** the shell uses the single-instance plugin and the launcher passes bootstrap/session tokens.
- Expose no unrestricted command, filesystem, or disk-writing bridge. **Done:** the shell has one typed connection command and no OS command/file APIs.
- Add WebKitGTK/runtime dependencies to the installer image and use the native
  launcher. **Done:** Chromium and the Python UI launcher are not installed.

### Phase 3 — Unix-socket privileged service

- **Done:** Add a root-owned native Rust service entrypoint and activate the socket transport in the installer launcher.
- **Done:** Replace loopback HTTP access with a root-owned Unix-socket service in the live-image configuration; development keeps the loopback fallback.
- **Done:** Use socket ownership/permissions and peer credentials, retaining the per-run session token as defense in depth.
- Validate the activated service and Tauri client in a built live ISO as a
  deferred release gate; no compatibility backend is required by the image.
- Preserve the frozen logical API, SSE/event semantics, validation, journal, and recovery behavior.
- **Done:** The Rust service validates the token, configured socket peer,
  request size, route allowlist, and every native request before dispatch.

### Phase 4 — Selective Rust migration

The native Rust transport daemon performs request normalization and install-plan
projection, then the native executor repeats all storage-dependent checks
immediately before mutation. Shared Rust/Python parity fixtures cover all five
modes and representative rejection branches; the Python side is used only to
verify fixture compatibility.
The Rust shell also parses explicit `lsblk` snapshots into typed disk and
partition records; the same fixture is exercised through the Python discovery
functions to pin safety-relevant output.

Port components only after behavioral parity and focused tests exist:

- **Done as a transport preflight:** request and install-plan normalization
  (native Rust daemon; Python validation is retained only for fixture parity).
  Shared parity cases live in
  `src/kyth-installer-web/src-tauri/testdata/installer_plan_cases.json`.
- **Done as a runtime query:** the root-owned Rust daemon now performs fixed,
  read-only `lsblk`, `findmnt`, and `blockdev` probes for disk inventory,
  partition inventory, and free-space regions. The Rust parser applies the
  protected-disk policy before returning API-compatible records and the native
  executor repeats validation immediately before destructive operations.
- **Done as metadata/validation plus a typed execution boundary:** the
  partition journal model, serialization, and safety checks remain covered,
  while GPT backup/restore, table creation, partition create/delete/flag
  operations, and supported filesystem formatting now go through the
  root-only Rust `kyth-installer-exec` helper. Rust now also validates
  partition targets and owns journal locking, backup/restore, operation
  ordering, filesystem shrinking, and commit failure rollback. The helper
  synchronizes completed partition-table backups and their parent directory
  before they can be used as recovery snapshots. The former Python fallback is
  retained only in source fixtures for parity tests. Both manual-journal and guided
  NTFS partition-boundary resize use the same typed Rust operation, including
  its fixed interactive confirmation and cancellation-safe child handling.
- **Done as a typed state and persistence boundary:** Rust owns transaction
  state encoding, atomic replacement, file/directory fsync, and Rescue
  classification. The Python writer is retained only as source fixture
  material; the packaged daemon reads and writes native transaction state.
- **Done as a pure model:** streaming command output framing, bounded failure
  tails, independent I/O/network/absolute timeout decisions, and cooperative
  cancellation; shared Rust/Python fixtures cover framing and failure tails,
  while process execution and privilege boundaries are owned by the Rust daemon
  and fixed root-only helper.
- **Done as a state model and typed executor:** mount registration, release,
  LIFO cleanup ordering, cleanup-state clearing, filesystem mounts,
  subvolume options, bind mounts, and recursive/lazy unmounts now use the
  root-only Rust disk helper. The fixed-argv compatibility implementation is
  retained only as source fixture material.
- **Bootc image-write handoff complete at the process boundary:** the typed
  bootc operation is validated and projected by Rust, then
  `kyth-installer-exec` pins and `exec`s `/usr/bin/bootc`. Rust owns
  phase-specific filesystem work, recovery reporting, and target
  configuration, as well as orchestration decisions, cancellation
  classification, transaction-status ordering, and the power-supply probe.
  The compatibility command builder remains until the full
  storage/configuration executor is ported.
- **Non-secret target configuration now uses the typed Rust executor:**
  hostname, locale, keyboard layout, and timezone-link writes are validated,
  synced, and applied by the native daemon/helper; Rust also owns account
  creation and phase sequencing.
- **First storage/configuration execution slice now uses the typed Rust
  executor:** Btrfs formatting, creation of the fixed `@`/`@home` subvolumes,
  default-subvolume selection, installer staging-directory creation, and
  append-only `/etc/fstab` updates all cross the root-only helper. Rust also
  owns phase orchestration and read-only UUID/EFI probes.
- **First streaming lifecycle slice now runs in Rust:** typed bootc image writes
  and filesystem-resize streams are validated, spawned, waited, and protected
  with parent-death cancellation by `kyth-installer-exec`. Rust owns output
  framing, bounded failure tails, timeout policy, and the user cancellation
  request.
- **First recovery-action slice now runs in Rust:** the support-log export
  validates the USB destination and configured artifact paths, skips symlink
  sources, copies only bounded installer artifacts, and syncs the exported
  files. Rust also owns removable-media discovery and the bounded copy path;
  the Python copy path remains only as source fixture material.
- **Done as a native execution boundary:** Rust performs Secure Boot/MOK
  probing and import-result classification, and the installed helper supplies
  the fixed firmware operation; `mokutil`, passwords, and firmware
  interactions never cross into Python.

### Fifteen-item implementation ledger

All implementation items below are complete and locally committed. Full
live-media, VM, hardware, and reboot acceptance is deliberately excluded from
this ledger and remains a release gate.

#### P0 — authority and durable safety

1. Native daemon owns the typed job/state machine, routes, and event stream.
2. Native transaction, failure-summary, and installer-log paths are unified
   and atomically persisted.
3. Native source-image inventory validates embedded OCI layout, digest, and
   metadata before destructive work.

#### P1 — destructive execution

4. Native storage execution covers wipe, alongside, free-space, manual, and
   NTFS-resize plan paths with protected-disk and geometry checks.
5. Native partition journal, mount registry, EFI reuse, cleanup, and typed disk
   helper boundary own storage mutations.
6. Native streaming process lifecycle owns fixed bootc/filesystem-resize
   commands, bounded output, cancellation, child reaping, and parent-death
   handling.

#### P2 — target configuration and identity

7. Native hostname, locale, keymap, timezone, and fstab configuration is
   validated, synced, and rolled back on later failure.
8. Native account creation runs through the typed helper; passwords and hashes
   remain stdin-only and out of plans, logs, events, and reports.
9. Native post-configuration assurance verifies identity files, account,
   fstab, boot metadata, and the installed deployment before success.

#### P3 — firmware and recovery

10. Native Secure Boot/MOK probing and staging uses fixed paths, stdin-only
    passwords, bounded waits, cancellation, and explicit state classification.
11. Native Rescue owns transaction classification, read-only diagnostics,
    bootc summary, report/log reads, and bounded USB support export.
12. Native completion, cancellation, failure-summary, and recovery state
    transitions preserve the frozen API/SSE contract.

#### P4 — packaging and authority retirement

13. Native launcher creates the root-owned session token, starts the Rust
    service, launches the unprivileged Rust shell, and cleans up deterministically.
14. The live image no longer installs Python installer packages, Chromium, or
    the legacy Python launcher/backend.

#### P5 — evidence and inventory

15. Runtime inventory, generated reports, source comments, and host-independent
    Rust/Python parity checks classify the Rust installer as the installed
    authority and Python installer sources as fixtures only.

### Phase 5 — VM destructive-path acceptance

- Validate wipe, alongside/resize, free-space, and manual modes in disposable VMs.
- Test cancellation during every long-running phase.
- Test interrupted partitioning, filesystem resize, image deployment, final configuration, and reboot recovery.
- Verify rescue mode stays read-only and produces support-safe exports.

### Phase 6 — Live-image integration and parity gate

- Build and typecheck the React/Tauri application in the image pipeline.
- Start the shell from a built live ISO without a development server.
- Verify authentication, asset embedding, single-instance behavior, and all installer workflows on live media.
- Confirm passwords never appear in URLs, logs, process arguments, persistent state, telemetry, or rescue exports.

### Phase 7 — Launcher retirement (code complete; acceptance deferred)

- **Done:** Switch the live image to the native launcher and Rust daemon.
- **Done:** Remove the Python backend package and Chromium/Python UI launcher
  from the supported image after code-level parity coverage.
- Reassess whether Calamares remains an optional fallback or can be retired.

## Historical implementation starting point (complete)

The original implementation sequence was:

1. Make the Rust daemon own the typed installer job/state machine and event
   stream while preserving the logical API.
2. Complete native storage execution and all five install-mode paths.
3. Move target configuration, account creation, and secret handling into the
   Rust executor.
4. Move Secure Boot/MOK execution and final reboot classification.
5. Run the complete destructive-path acceptance matrix, then remove the
   Python installer runtime and compatibility launcher.

Items 1–4 are complete in the native code-level migration. Item 5 remains the
deferred full VM/live-media acceptance gate; Python installer sources remain in
the repository only as fixtures and are not installed by the supported image.
