# System Hub Rust release checklist

Use this checklist before shipping a KythOS image that makes the Rust/Tauri
System Hub the default launcher. The checklist is intentionally separate from
the migration roadmap: a green Rust build does not by itself demonstrate
runtime parity on an installed image.

## Status snapshot (2026-09-05)

Current target: `testing` at `192ee5ba` with the Hub migration cutover changes;
the local worktree also contains the P0 closed-action-allowlist fix.
The prior pre-build gates are complete. The local shared-crate gate now passes
501 Rust tests across 26 suites with the telemetry-writer feature (498 in the
default feature set),
including native telemetry, extended Guardian, firmware staging, watcher
retry/lock coverage, the privileged socket policy, secret redaction, and SAML
redirect validation. The native telemetry writer and privileged daemon release
builds also pass.
The native post-update, first-login app-status, Steam launcher-export,
KRunner desktop-entry, diagnostic, game-boost, and doctor utilities are
included in the shared Rust build. No Python/Qt update notifier is installed
or autostarted.
The native `kyth-tunable-rs` dispatcher now owns all 94 tunable entries;
the compatibility dispatcher remains only as a rollback fixture for older images.

GitHub Actions Validation run
[`33665906864`](https://github.com/kyth-os/kyth/actions/runs/33665906864)
is green for this commit, including the Hub web shell and coverage/lint jobs.
The locally built ISO has now completed the install-only KVM/QEMU/SPICE
acceptance path. The guest booted the installed image, found the native Hub
binary, exercised the manifest-derived route matrix, verified second-launch
page forwarding, checked degraded dashboard behavior, exercised the Hub update
read, and verified a privileged allowlist failure. The qualification artifacts
are under `output/live-iso/vm-acceptance-final/`.

This is local install-only evidence. Update staging/rollback, all destructive
installer modes, and the exact promoted-image security/rollback drill remain
open and are not implied by the acceptance run.

## Pre-build gates

- [x] Dashboard and Updates command ledger is present.
- [x] Frontend/Rust contract tests pass.
- [x] Rust command modules compile and their unit tests pass (the canonical
      script compiles the Tauri crate; its eight unit tests were run separately).
- [x] Privileged operations are allowlisted and validate inputs centrally.
- [x] Frontend confirms privileged and destructive actions without displaying
      BitLocker secrets.
- [x] CI `hub-shell` job is green on the current testing commit
      (`1d2f7f07`; Validation run `33538517283`).
- [x] `npm ci` succeeds from the lockfile in a clean build environment.
- [x] `cargo build --locked` succeeds for the Tauri shell and shared crate.
- [x] The asset-embed assertion finds every built JS/CSS asset in the binary.
- [x] Every static Hub action is represented by the closed Rust `HubAction`
      allowlist, including Secure Boot enrollment; unknown action names are
      rejected during Tauri deserialization.

## Image and runtime gates

- [x] The locally built installed image contains `/usr/bin/kyth-hub-shell` and
      the launcher selects it.
- [x] `kyth-welcome-launch --page` routes to every manifest-derived destination
      and section in the local installed image.
- [x] A second launch focuses the existing shell and forwards its page in the
      local installed image.
- [x] Dashboard renders an honest degraded state with probe data absent in the
      local installed image.
- [ ] Updates check, stage, rollback, and restart guidance are all truthful;
      the local run covers the update read only, not staging and rollback.
- [ ] Guardian, Hardware, App Store, and Gaming actions complete or report
      bounded failures on a real installed image.
- [x] Tauri system-changing controls have an explicit confirmation gate;
      secret-bearing and argument-bearing workflows remain withheld from the
      bridge until dedicated controls exist.
- [x] The local installed image rejects a non-allowlisted privileged action.
- [ ] Complete the promoted-image authorization and secret-redaction drill;
      the local install-only run is not the final security sign-off.

## Python retirement gate

The Python Hub UI package is no longer installed or copied into the Hub build.
The native Rust launcher, Tauri bridge, React frontend, desktop metadata, and
embedded compatibility catalog are the supported Hub implementation. The
locally built installed image has been exercised by the VM guest; promoted
image acceptance remains a release gate. Remaining Python pieces are outside
the Hub runtime and are transitional authorities or source artifacts for other
parts of the OS:

- `kyth-guardian` now owns the extended deterministic sweep and a bounded,
  schema-validated local-model investigation path; missing model assets are
  explicit degraded state;
- the native Rust update watcher owns the installed scheduling/status path,
  including firmware staging, free-space/lock gates, retryable status, and
  session/network safety conditions;
- the Tauri VPN command now owns the profile editor, openconnect worker, and
  SAML webview; the legacy `kyth-vpn-connect` package and Python/Qt source were
  removed in P2;
- network-share mutations use the typed privileged socket and native Rust root
  helper; and
- the fixed privileged socket daemon is the native Rust `kyth-privileged`
  binary; its former Python fixture was removed in P2; and
- `kyth-telem` is the native Rust telemetry writer built with the
  `telemetry-writer` feature; its former Python fixture was removed in P2 and
  is not installed or enabled;
- retired Python Hub UI source and UI-only tests, removed in Phase 4; the
  launcher is native Rust, the compatibility catalog and desktop metadata are
  Hub-owned assets, and the route metadata generator is now the native Rust
  `kyth-hub-desktop-entries` build utility; and
- any workflow whose Rust command is not yet listed in the command ledger.

## Strict service-ownership gate

These remain unchecked until the migration is genuinely off the remaining
Python authority for Hub-facing reads and actions:

- [x] Replace the Python/PySide6 VPN profile editor, openconnect worker, and
      SAML browser with a Rust/Tauri workflow; the legacy package is not
      installed.
- [x] Replace the Python root network-share helper behind the fixed privileged
      socket with the native Rust binary, retaining credential isolation and
      audit behavior.
- [x] Replace the Python root-owned privileged socket daemon with the native
      Rust `kyth-privileged` binary, retaining peer-credential checks, fixed
      argv allowlisting, BitLocker stdin handling, bounded execution, and audit
      behavior (native unit tests pass; image acceptance waived).
- [x] Enable and validate the Rust telemetry writer, then remove the active
      Python `kyth-telem` daemon (native fixture tests pass; image acceptance
      waived).
- [x] Complete extended Guardian/model probe parity and service-level tests;
      unavailable model assets remain an explicit degraded state.
- [x] Complete update-watcher lock, firmware staging, retryable-status, and
      network/session safety parity (native fixture tests pass; image
      acceptance waived).

## P2 compatibility-retirement gate

- [x] Replace the installed Python `kyth-ai-perfd` launcher with the native
      Rust daemon; its policy loop uses the shared Rust performance modules and
      retains the bounded 30-second TTL behavior.
- [x] Remove Python JSON parsing from the probe, OS update, JetBrains Toolbox,
      LSFG-VK, and runtime perf-gate Just recipes; data-only extraction uses
      native commands or `jq`.
- [x] Port all 49 sysctl-backed and all 45 module-specific entries of the
      indirect tunable dispatcher to
      native `kyth-tunable-rs`; the installer derives native symlinks from the
      Rust binary for all registry entries.
- [x] Port the final module-specific tunable entry to native `kyth-tunable-rs`;
      all 94 registry entries now resolve to the native dispatcher, while the
      compatibility fixture remains available for rollback.
- [x] Remove the obsolete Python/build fixtures for the native update watcher,
      telemetry writer, privileged socket boundary, network-share executor,
      and standalone VPN client.
- [x] Remove stale Python/Qt VPN launch references and fixture-only tests.
- [ ] Run a post-cutover observation window. This is waived/not started for
      the YOLO cutover because installed-image/user acceptance was skipped;
      treat runtime qualification as an explicit operational risk.

## Phase 6 security and rollback gate

- [x] Tauri declares an explicit minimal capability for the main shell and
      Rust-managed VPN sign-in window.
- [x] Bounded helper output is centrally redacted before it reaches UI job
      status, diagnostics, or privileged audit output.
- [x] BitLocker, VPN, and network-share credentials remain off process
      arguments; privileged responses redact any echoed request secrets.
- [x] SAML redirect and callback URLs are size-bounded and reject insecure,
      credential-bearing, fragmented, or malformed destinations.
- [x] Telemetry-free local startup, command-failure, service-absence, and
      update-health diagnostics are mapped in the
      [Kyth Hub Rust rollback runbook](kyth-hub-rust-rollback-runbook.md).
- [x] Release-blocking signals and the revert/removal procedure are defined in
      the rollback runbook.
- [ ] Execute the security review and rollback drill on the exact installed
      image. This is waived/not started for the YOLO cutover; source/build
      gates passing is not installed-image acceptance.

## Rollback triggers

Keep the previous image available for rollback and revert the Rust default if
any of these occur after an image build:

- the shell fails to start or does not render its embedded frontend;
- a deep link opens the wrong page or a second launch opens another window;
- a privileged action bypasses confirmation, leaks a secret, or executes an
  operation outside the allowlist; or
- an update, Guardian, hardware, application, or gaming workflow reports
  success before the underlying action has completed.

## Validation commands

The canonical CI/image gate is:

```text
build_files/scripts/check-hub-web-shell.sh
```

It runs the clean frontend install, production build, frontend/Rust contract
tests, a headless SSR construction smoke test of every Hub section component
(`tests/hub-shell-smoke.test.mjs`), shared Rust tests, Tauri build,
and embedded-asset assertion.

The script uses both `cargo test --locked` and `cargo build --locked` for the
Tauri shell. Run the focused unit-test gate explicitly when iterating with:

```text
(cd src/kyth-hub-web/src-tauri && cargo test --locked)
```

Phase 6 source validation on 2026-09-02 also passed:

```text
(cd src/kyth-shared-rs && cargo test --locked)       # 498 passed
(cd src/kyth-hub-web/src-tauri && cargo test --locked) # 8 passed
(cd src/kyth-hub-web && npm run test:contracts)      # pass
(cd src/kyth-hub-web && npm run test:smoke)          # pass
(cd src/kyth-hub-web && npm run build)               # pass
./build_files/scripts/validate.sh --fast             # pass
git diff --check                                      # pass
```

The environment does not provide `cargo fmt`/`rustfmt`; formatting was
therefore not independently run. The exact-image security review and rollback
drill remain unchecked until the new installed-image acceptance run has passed
on the testing ISO.

`npm run tauri:build` compiled the optimized `kyth-hub-shell` application and
loaded the capability configuration, but final AppImage bundling was blocked by
the environment's read-only filesystem. This is an environment limitation, not
a passing installed-image package result.
