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

Phase 0 reclassification (2026-09-07, inventory schema 4):

| Metric | Before | After |
| --- | --- | --- |
| Active entries | 499 | 230 |
| Active Python entries | 294 | 45 |
| `python-shared-package` active | 255 | 45 (reachability-derived) |
| `python-runtime` active | 39 | 0 (all installed launcher entries are native) |
| `data-or-config` active | 8 | 0 (terminal `not-applicable`) |
| Superseded entries | n/a | 99 (native rollback fixtures) |
| `p0_open_entries` | 0 | 0 |

Reachability audit: the checker now computes a transitive AST import closure
from the three surviving Python console-script roots (`boot_health`,
`hardware_policy`, and `qualification`), scans direct `build_files/scripts`
imports, preserves explicit shell-harness channels, and expands the
documented `hardware_quirks` importlib catalog. The resulting 45 active
package modules remain queued; 111 unreachable shared-package modules are
classified as source-only compatibility fixtures. The first reachable module,
`ai_dev`, is now owned by the packaged `kyth-ai-dev` Rust binary; its Python
source is retained as an inactive rollback/parity fixture. All 94 tunable
registry aliases remain verified against `tunable_registry.rs` with 0 missing,
and the 99 native rollback fixtures remain inactive.

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

Executable-handler cutover (2026-09-07):

| Metric | Before | After |
| --- | ---: | ---: |
| Active Python entries | 163 | 158 |
| `python-runtime` active | 2 | 1 (`kyth-vm-acceptance-guest`, deferred to its acceptance gate) |
| `python-shared-package` active | 161 | 157 |
| Superseded entries | 94 | 98 |

`kyth-exe-handler` is now a native Rust launcher that forwards a MIME-file
launch into the existing Tauri/React Hub dialog. The Rust shared crate owns
inspection, compatibility assessment, application suggestions, preference
persistence, and the bounded Bottles workflow; the webview has typed commands
only and no generic process bridge. The retired Python handler and its four
support modules are source-only rollback/parity fixtures, not installed runtime
paths.

Reachability closure (2026-09-07):

| Metric | Result |
| --- | ---: |
| Inventory entries | 859 |
| Active entries | 230 |
| Active Python package entries | 45 |
| Unreachable shared-package fixtures | 111 |
| Active shell entries | 104 |
| Open priority-0 entries | 0 |

Reachable-package cutover (2026-09-07):

| Metric | Before | After |
| --- | ---: | ---: |
| Active Python package entries | 46 | 45 |
| Native shared-package owners | 0 | 1 (`kyth-ai-dev`) |
| Superseded native rollback fixtures | 98 | 99 |

`kyth-ai-dev` now owns setup, status, enter, start, stop, model pull, and
destructive box removal. Its Rust controller retains bounded execution,
validated model input, GPU-aware creation flags, atomic host-volume policy,
and redacted child output; the Python module remains only for parity and
rollback qualification.

The package is still installed for compatibility and rollback tooling, but a
package path alone is no longer evidence of runtime authority. The checked-in
inventory records `source-only` plus an explicit reason for each unreachable
module, while active package modules retain their fixture owner and retirement
condition until their Rust cutover is complete.
