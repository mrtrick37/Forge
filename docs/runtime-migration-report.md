# Runtime migration report

The source of truth for runtime ownership is the generated
[`runtime-migration-report.json`](../build_files/config/runtime-migration-report.json),
produced from the generated
[`runtime-migration-inventory.json`](../build_files/config/runtime-migration-inventory.json).

The report distinguishes source implementation from installed runtime
authority. In particular, a Python file can be one of three different things:

- an active compatibility/runtime package;
- a source counterpart whose installed entry point is already native Rust; or
- source-only compatibility material that is not installed in the supported
  image.

Regenerate and validate both files with:

```text
python3 build_files/scripts/check-runtime-migration-inventory.py --generate
```

The normal repository validation command runs the checker without `--generate`
and fails if the checked-in inventory or report is stale. It also checks the
React/Tauri frontend boundaries for direct Python/process APIs, unscoped shell,
filesystem, or process plugins, and generic command bridges.

P0 interpretation:

- The installer cutover is complete: no `python-installer` authority remains
  active and `p0_open_entries` is 0. The Python installer backend is
  source-only parity material.
- `python-shared-package` counts only reachable modules. Superseded rollback
  fixtures carry `superseded_by` and are inactive — they are not migration
  tasks until the Hub plan's observation window (Phase 3) retires them.
- Reachability has three channels: static imports (including transitive) from
  an active launcher/unit, a documented dynamic-dispatch table, and direct
  shell-harness invocation (e.g. `qualification.py` via `vm-acceptance.sh`).
- `data-or-config` entries are terminal (`not-applicable`), not queued work.
- `source-only` entries are not runtime migration tasks.
- Rust binaries, Rust services, and Rust dispatchers are counted separately
  from their retained Python source counterparts.

Phase 0 reclassification (2026-09-07, inventory schema 2 → 3):

| Metric | Before | After |
| --- | --- | --- |
| Active entries | 499 | 399 |
| Active Python entries | 294 | 202 (−92) |
| `python-shared-package` active | 255 | 163 (−92 superseded fixtures) |
| `python-runtime` active | 39 | 39 (unchanged — the real Phase 1/2 ledger) |
| `data-or-config` active | 8 | 0 (terminal `not-applicable`) |
| Superseded entries | n/a | 92 (`superseded_by: native::kyth-tunable-rs`) |
| `p0_open_entries` | 0 | 0 |

Reachability audit behind the 92: transitive static-import closure over the
39 queued launchers plus a `build_files/scripts` harness scan. Only
`sched_arbiter` (imported by `build_files/kyth-game-launch`) and `perf_gate`
(used by `build_files/scripts/check-perf-gate.py`) remain reachable and stay
active; all 94 tunable registry aliases verified present in
`tunable_registry.rs` with 0 missing.

User-polish cutover (2026-09-07, after the 38-port baseline):

| Metric | Before | After |
| --- | ---: | ---: |
| Active entries | 363 | 360 |
| Active Python entries | 166 | 163 |
| `python-runtime` active | 3 | 2 (the two deliberate `kyth-exe-handler` and `kyth-vm-acceptance-guest` deferrals) |
| `python-shared-package` active | 163 | 161 |
| `rust-service` active | 19 | 20 |
| Superseded entries | 92 | 94 (adds the two user-polish source fixtures) |

The native `kyth-user-polish` binary is built by the shared Rust crate and
installed at the same `/usr/bin/kyth-user-polish` path. Its Python modules are
retained only for parity tests and rollback qualification.
