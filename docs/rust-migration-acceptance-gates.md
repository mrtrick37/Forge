# Rust migration acceptance gates

This document records the evidence required for recommendations 6–9 of the
Rust migration completion order. Source-level tests are necessary, but they
do not prove behavior on the promoted bootc image.

## Static owner gate

Complete: the source-derived recipe ledger contains 202 recipes, 202 accounted
for, including 107 native dispatches, 92 native fallbacks, and 3 explicitly
retired optional vendor-asset recipes; it has zero missing Rust owners. This is not a substitute for the exact-image
behavioral gate below.

## Exact-image gate

The reproducible entry point is:

```text
just rust-migration-acceptance \
  iso=/absolute/path/to/kyth-live-testing.iso \
  image_ref=ghcr.io/kyth-os/kyth@sha256:<promoted-digest> \
  artifacts=/absolute/path/to/acceptance-evidence
```

`build_files/scripts/run-rust-migration-acceptance.sh` records the source
commit, exact image reference, source-derived ledgers, image metadata, serial
log, QEMU log, qualification report, and a per-gate status file. It leaves
the evidence directory in place for review; cleanup is a separate explicit
step.

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

### Reuse an existing exact ISO artifact

The build workflow's opt-in acceptance job rebuilds the ISO when it is enabled.
For a published artifact that already passed the build and checksum gates, use
the non-publishing artifact-reuse workflow instead. It binds the checkout to
`source_sha`, downloads `artifact_name` from `artifact_run_id`, verifies the
embedded checksum, and invokes the same `run-rust-migration-acceptance.sh`
wrapper. The wrapper records the ledgers, image metadata, install boot, and
update/rollback evidence without moving a release tag:

```text
gh workflow run rust-migration-acceptance.yml --ref testing \
  -f artifact_run_id=<iso-build-run-id> \
  -f artifact_name=<exact-iso-artifact-name> \
  -f source_sha=<source-commit> \
  -f image_ref=ghcr.io/kyth-os/kyth@sha256:<promoted-digest> \
  -f update_ref=ghcr.io/kyth-os/kyth@sha256:<previous-or-next-digest>
```

`update_ref` is optional for a basic install/boot gate. Supplying a distinct
image reference makes the update/rollback cycle exercise a real image change;
the wrapper records the selected reference in `run-metadata.txt`. The resulting
`rust-migration-acceptance-<run-id>` artifact is the evidence bundle to review
before starting the observation window.

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

Current source evidence already satisfies the classification portion: the
runtime report records zero active Python authorities and all 92 tunable
modules are rollback/parity-only fixtures. Physical deletion of those fixtures
and stale launch prose remains deferred until the observation window closes.

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
