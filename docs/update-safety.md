# Health-Aware Updates

KythOS treats an update as successful only after the booted image passes
required health checks. Downloading an image or reaching the bootloader is not
enough.

## Lifecycle

1. The update watcher resolves the remote image to an immutable digest.
2. It refuses a digest that is quarantined locally or belongs to a different
   explicitly configured rollout ring.
3. After `bootc upgrade` stages the deployment, KythOS records the pending
   digest in `/var/lib/kyth/boot-health.json`. A pull that times out
   (`bootc upgrade` up to 3600 s) is **retryable** — no pending digest or
   quarantine is written, and the next run retries the same digest.
   Only after three unhealthy boots (step 6) does a digest become
   quarantined; `retryable: bootc upgrade timed out` in
   `/var/lib/kyth/update-watcher-status.json` surfaces as "Retry available"
   in System Hub.

   The Hub's guarded manual-update helper uses a short independent `skopeo`
   manifest probe for early quarantine and exact-digest checks. A timeout,
   DNS failure, or malformed remote manifest is treated as a degraded
   preflight rather than a reason to block `bootc upgrade`, because bootc has
   its own registry client and is authoritative for the image it stages. When
   the probe succeeds, the staged digest must still exactly match it. When the
   probe is unavailable, the helper validates bootc's staged digest, checks it
   against locally recorded quarantine state, and reports that degraded mode
   explicitly.
4. On the next boot, greenboot runs KythOS checks from
   `/etc/greenboot/check/required.d`.
5. A healthy boot records the exact digest as known-good. A failed boot records
   the boot ID and reason, then greenboot retries the boot.
6. After three unhealthy boots, KythOS quarantines the failed digest. The
   automatic updater and guarded manual update paths will not download or
   stage that digest again.

### Rollback is KythOS's own code, not greenboot's boot counter

greenboot's usual automatic-rollback story relies on a GRUB2 `grubenv`
boot-counter: GRUB decrements a counter on every unsuccessful boot and falls
back to the previous deployment when it hits zero. This image boots via
systemd-boot/BLS entries with no `grubenv` and no boot-counter suffix, so that
mechanism does not exist here — a digest that keeps failing its required
checks would otherwise just reboot into itself forever (reproduced directly:
9 consecutive ~3-minute boots with no self-recovery).

Instead, the moment a digest crosses the quarantine threshold in step 6,
KythOS's own `red.d` hook (`kyth-boot-health record-failure`) calls
`bootc rollback` directly — see
`kyth_shared.boot_health.trigger_rollback_if_newly_quarantined`. This fires at
most once per digest (tracked as `rollback_attempted_for` in
`boot-health.json`) so a rollback target that turns out to be unhealthy itself
can bounce back to the digest that triggered it exactly once, never loop.
There is no lower-level bootloader fallback under this: if `bootc rollback`
itself fails (for example because the same problem that made the deployment
unhealthy also impairs `bootc`), the machine stays quarantined on the broken
digest but records `last_rollback_error`/`last_rollback_at` in
`boot-health.json`. Check `kyth-boot-health status --json`'s
`rollback_attempted_for` + `last_rollback_error` fields when diagnosing a
stuck deployment. A failed rollback can be retried manually after fixing the
underlying condition:

```bash
sudo kyth-boot-health retry-rollback --digest sha256:…  # retries bootc rollback now
sudo kyth-boot-health clear-quarantine --digest sha256:… # or allow the digest to be staged again
```

The required checks deliberately cover immutable deployment invariants: KythOS
identity, bootc deployment metadata, the desktop and networking components, and
the running kernel's module tree. They do not fail because a user disabled a
service, disconnected a network cable, or changed mutable `/etc` policy. Such a
failure would survive an OS rollback and could otherwise create a reboot loop.

## Release Rings

`/etc/kyth/auto-update.toml` supports these policies:

- `follow-image` follows the channel selected in System Hub.
- `stable` accepts only the `latest` image family.
- `testing` accepts only the `testing` image family.
- `canary` is reserved for explicitly published pre-testing images.

Explicit ring policy fails closed if the machine is accidentally switched to a
different image family. Ring selection does not bypass signature, digest, or
qualification checks.

## Inspect and Recover

Show bootc and health state:

```bash
ujust status
ujust update-health
kyth-boot-health status --json
```

System Hub also displays the current boot-health state, failed-boot count, and
number of quarantined builds on the Updates page. Support snapshots include the
same non-secret information.

A quarantined digest stays blocked even when a mutable registry tag still
points to it. After confirming that the image was repaired or the failure was
environmental, an administrator can explicitly retry it:

```bash
ujust retry-quarantined-update sha256:FULL_DIGEST
```

Clearing quarantine does not immediately update or reboot the machine. It only
allows the normal update watcher to consider that digest again.

System Hub, `ujust update` (and the `upgrade` alias), `ujust kyth-upgrade`,
the full updater, and the Hub-independent fallback updater all use
`kyth-safe-upgrade`. Direct `sudo bootc upgrade`
remains available as an expert escape hatch, but requires normal administrator
authentication and deliberately bypasses KythOS quarantine policy.

## Troubleshooting: `Bus owner changed` during `ujust update`

Symptom: `ujust update` prints `Running rpm-ostree update...`, pulls
`ostree-unverified-registry:ghcr.io/...`, then fails with:

```
error: Bus owner changed, aborting. This likely means the daemon crashed; check logs with `journalctl -xe`.
Completed rpm-ostree update
```

That is Universal Blue's leftover recipe, not Kyth's updater. It always
prints "Completed" in full mode, even when `rpm-ostreed` dies. A concurrent
`rpm-ostreed-automatic` stage (the "automatic updates (stage) are enabled"
note) is a common trigger during a multi-GB ostree chunk fetch.

On a machine that still has the old recipe:

```bash
sudo systemctl stop rpm-ostreed-automatic.timer rpm-ostreed-automatic.service
sudo systemctl restart rpm-ostreed
ujust kyth-upgrade
```

Current images make `ujust update` run `kyth-full-update` (bootc +
quarantine) and set `AutomaticUpdatePolicy=none` so the two pullers cannot
race.

## Troubleshooting: `opendir(boot): Operation not permitted`

Symptom:

```
$ sudo bootc status
error: Status: opendir(boot): Operation not permitted
```

Kyth `/boot` is a read-only bind of the root btrfs subvol (the same layout
that makes a plain `remount,rw` return EINVAL). `bootc status` still takes a
sysroot write-lock and opens the sysroot-relative `boot` directory; that
`opendir` returns EPERM until the bind is remounted and, when present,
`/sysroot/boot` is bound to the real `/boot`.

```bash
sudo kyth-finalize-staged prepare-boot
sudo bootc status
```

If `prepare-boot` is not on this image:

```bash
sudo mount -o remount,bind,rw /boot
sudo bootc status
```

`rpm-ostree status` does not need that open and still shows the booted and
rollback deployments. `ujust status` and `kyth-bootc-guard status` remount
`/boot` first and fall back to `rpm-ostree status` when bootc still fails.

## Troubleshooting: Staged Upgrade Not Taking Effect

Symptom: `bootc upgrade` (or `bootc switch`) reports success and "Queued for
next boot" with the correct digest, but after reboot `bootc status` still
shows the old digest — and repeating `bootc upgrade` any number of times
doesn't change the outcome.

Root cause observed on the ASUS TUF FA617NS host (2026-08-20): staging always
succeeded (the new deployment tree was fully written under
`/ostree/deploy/default/deploy/`), but the step that promotes a staged
deployment into a real bootloader entry — `ExecStop=/usr/bin/ostree admin
finalize-staged` on `ostree-finalize-staged.service`, which runs at shutdown
— failed with:

```
error: Remounting /boot read-write: Invalid argument
```

`/boot` on this layout is a bind mount of itself onto the same btrfs subvol as
root (`subvolid=5,subvol=/`), mounted read-only during normal operation;
finalize needs to remount it read-write briefly to write the new
kernel/initramfs/loader entry. When that remount fails at shutdown, the
staged deployment appears to be silently dropped rather than retried — no
error is surfaced to the user, and the next boot just reuses the previous
default. This reproduced even after a single clean `bootc upgrade` followed
by a single clean `systemctl reboot` (not just after re-running upgrade
mid-flight), so don't assume a "ran it twice" race is always the explanation.

Diagnose with:

```bash
journalctl -b -1 -u ostree-finalize-staged.service
```

Look for the `Invalid argument` line right after `Stopping
ostree-finalize-staged.service`.

`kyth-safe-upgrade` remounts `/boot` with `bind,rw` (plain `remount,rw` is
EINVAL on this layout) and runs `ostree admin finalize-staged` after
`bootc upgrade` returns. `ostree-finalize-staged.service` ExecStart only
prepares `/boot`; finalize on ExecStart deadlocks against bootc's sysroot
lock. ExecStop still finalizes at shutdown as a fallback.

A pull that is still running after 3600s is retryable (not quarantined).
Hub `bootc status` probes are skipped while an upgrade *or*
`ostree admin finalize-staged` is active so they cannot take the same
write-lock. `PrivilegedGateway` allows `kyth-safe-upgrade` 3600 s (not 300 s).

If you are already stuck with "Queued for next boot" on an older image:

```bash
sudo mount -o remount,bind,rw /boot
sudo kyth-finalize-staged                 # remounts bind,rw, then finalize
sudo ostree admin status                  # confirm it now shows "(pending)"
sudo systemctl reboot
```

Or, on an image that includes the helper: `sudo kyth-finalize-staged reboot`
or `ujust apply-staged`.

## State and Privacy

The state file contains image digests, timestamps, failure counts, rollout
ring, and the last health-check reason. It contains no hostname, account name,
network address, hardware serial number, or telemetry upload identifier. The
file is readable for diagnostics but writable only by privileged system
services.
