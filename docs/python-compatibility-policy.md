# Final-image Python compatibility policy

Status: temporary compatibility layer; not a supported runtime authority.

The Rust migration has removed Python from the supported policy/execution
paths, but the final image still installs `build_files/kyth_shared` as a
transitional package. This is intentional until the promoted-image gates in
the Rust migration completion plan close.

## Allowed uses while the gate is open

The installed Python material may be used only for:

* rollback and parity fixtures for a previously deployed image;
* the compatibility `kyth-tunable` dispatcher while its Rust symlink/native
  registry rollout is observed across stable and testing images; and
* build-time or test-time tooling that is not an installed runtime authority.

The package's historical console-script names are not evidence of Python
ownership. The corresponding supported entry points are native binaries such
as `kyth-ai-dev`, `kyth-boot-health`, `kyth-hardware-policy`, `kyth-guardian`,
`kyth-qualify`, `kyth-smoke-check`, and `kyth-setup-transfer`.

## Prohibited uses

No supported systemd unit, desktop entry, `ujust` recipe, Tauri command, or
runtime service may invoke a Python console script for policy, validation,
mutation, privilege transition, or success/failure semantics. New runtime
behavior must be implemented behind a Rust owner and added to the recipe
ledger before it is exposed.

## Removal gate

Remove the final-image package installation from `Dockerfile` only after all
of the following are evidenced on a promoted image:

1. the native recipe ledger has no open owner assessments, or each remaining
   name has an explicit reviewed retirement decision;
2. the Rust tunable dispatcher owns every installed tunable symlink and the
   compatibility dispatcher is not selected for a supported name;
3. exact-image tests show that units, desktop launchers, `ujust`, and Tauri
   commands resolve to native owners without Python imports;
4. rollback/parity fixtures have been archived or moved out of the supported
   image; and
5. the post-cutover observation window records no supported-path regression.

Until that gate closes, deleting the package would make rollback evidence and
older-image compatibility less recoverable. Retaining it does not reopen the
Python migration queue: the reachability-derived runtime inventory remains
the authority for active Python behavior.
