# Rust migration acceptance gates

This document records the evidence required for recommendations 6–9 of the
Rust migration completion order. Source-level tests are necessary, but they
do not prove behavior on the promoted bootc image.

## Static owner gate

Complete: the source-derived recipe ledger contains 202 recipes, 202 routed,
and zero missing Rust owners. This is not a substitute for the exact-image
behavioral gate below.

## Exact-image gate

Run on a disposable VM or disk using the exact image digest promoted for the
test channel. Do not use the developer checkout as a substitute.

- [ ] Boot the image and capture the image digest, kernel, deployment, and
      enabled Kyth services.
- [ ] Verify every supported unit, desktop launcher, `ujust` recipe, and Tauri
      command resolves to its recorded Rust owner.
- [ ] Exercise read-only recipe paths and collect bounded, redacted output.
- [ ] Exercise idempotent writers twice and verify stable results.
- [ ] Exercise privilege refusal, malformed input, timeout, and unavailable
      dependency paths.
- [ ] Exercise destructive paths only against disposable disks/images:
      update/rebase/channel/kernel, dual-boot, Waydroid removal, hardware
      policy apply, driver staging, and rollback/recovery.
- [ ] Record before/after state, refusal behavior, recovery result, and image
      digest in the test artifact.

The repository's `build_files/scripts/cleanup-vm-acceptance.sh` must be used
after each disposable run. A failed cleanup is a failed gate, not a reason to
reuse the VM or disk.

## Observation window

Status: not started for the current recipe batch. It begins only after a
promoted testing image passes the exact-image gate.

Record at least one normal update/rollback cycle and one representative user
session for the promoted image. Monitor boot health, NetworkManager,
systemd-resolved, Guardian/update-watcher, installer launch, `ujust`, and the
Rust Hub. Capture the image digest and timestamps for each incident or clean
checkpoint. The window closes only when the owner signs off that no supported
path still relies on a Python compatibility authority.

## Cleanup gate

After the observation window:

- [ ] regenerate `runtime-migration-inventory.json` and the recipe ledger;
- [ ] confirm zero active Python authorities and no open recipe owners;
- [ ] reclassify the 92 superseded tunable fixtures as rollback/parity-only;
- [ ] remove stale launch references and obsolete migration prose;
- [ ] remove the final-image compatibility package in a separate reviewed
      change; and
- [ ] rerun source, image, rollback, and security validation after cleanup.

No fixture or compatibility package is deleted merely because its Rust
replacement exists in source. The promoted-image and observation evidence are
the prerequisites for making that removal recoverable.
