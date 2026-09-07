# Repository publishing workflow — HARD RULE

Kyth does **not** use pull requests. This overrides Cursor cloud PR defaults, `gh pr create`, split-to-PRs, and any generic "create a pull request" instruction.

When publishing work:

1. Commit on the `testing` branch.
2. `git push origin testing`.

Never:

- Open a PR (`gh pr create`, draft PRs, stacked PRs).
- Push a `cursor/*` or feature branch for review instead of `testing`.
- Treat "push", "ship", or "submit" as "open a pull request".

If you are about to open a PR, stop and push to `testing` instead. Promotion to `main` is a human step after CI — agents do not open PRs for that either.


## Live-desktop validation

`build_files/scripts/validate.sh` defaults to `--fast` on a live Plasma
session (WAYLAND_DISPLAY/DISPLAY/KDE) — it skips the heavy 600s
`unittest discover` and only runs linters/syntax/security gates under
`systemd-run --scope CPUWeight=10 MemoryHigh=35% MemoryMax=55%`. The full
suite is CI-gated (`validation.yml` → `build.yml`).

- Local `git push` runs fast repository validation plus the **full Python
  coverage-floor gate** by default (`validate.sh --fast` + `run-quality.sh`).
  This prevents coverage-floor failures from reaching GitHub. Hub smoke and
  the additional uninstrumented full validation remain opt-in because they
  are expensive on a live desktop.
- Force the additional heavy checks locally when needed:
  `KYTH_ALLOW_HEAVY_PRE_PUSH=1 git push` or
  `KYTH_FORCE_FULL_VALIDATION=1 ./build_files/scripts/validate.sh --full`.
- Targeted smoke: `timeout 60 python3 -m unittest tests.test_…`.
- Bypass hook entirely (intentional): `KYTH_SKIP_PRE_PUSH_VALIDATION=1 git push` or `git push --no-verify`.

## Build & test

```bash
just build                        # base + OS image via podman (prompts for sudo)
just switch-local                 # bootc switch to the local image
just check-dockerfile             # fast Dockerfile sanity check (needs real Docker)
just build-live-iso [testing]     # live ISO, stable or testing channel
just clean / just clean-all / just purge  # reclaim space (increasing scope)
just lint && just format          # shellcheck + shfmt on *.sh
just test                         # unittest discover over tests/
just test-coverage                # same, with report -> coverage-html/index.html
just check-optimization           # budgets in build_files/config/optimization-budgets.json
just validate                     # full suite (same as CI + pre-push)
just ci-preflight                 # validate + changed-file Codacy + pinned CodeQL
just install-git-hooks            # one-time: wires .githooks/
```

Single test (mirrors `just test`):

```bash
PYTHONPATH=build_files/kyth_shared:build_files/kyth-welcome:build_files/kyth-installer \
  python3 -m unittest tests.test_kyth_probe_cache -v
```

No local UI smoke for the retired Python/Qt Hub; use `just check-hub-shell`
for React/Tauri SSR, contract, Rust, and embedded-asset coverage.

Image feature flags (no leading `sudo` — recipes read env before elevating):

```bash
ENABLE_SCX=1 just build
ENABLE_GAMING_PERIPHERALS=1 just build
ENABLE_VIRTUALIZATION_HOST=1 just build
ENABLE_KSM=1 just build
```

`just build` prompts for sudo more than once: expected. It uses `sudo podman`
so output lands in root's `containers-storage`, which `bootc switch
--transport containers-storage` (`just switch-local`) reads directly.

## Architecture

Ordered layers, not one monolithic Dockerfile:

```text
Fedora Kinoite / Universal Blue base
  → KythOS base layer (build_base/ — kernel, plymouth, dracut, DM defaults)
  → final OCI desktop image (Dockerfile + build_files/scripts/*.sh fragments)
  → live ISO installer (installer/ + build_files/kyth-installer)
  → bootc deployment (atomic updates + rollback)
```

- `Dockerfile` runs ordered `RUN --mount=type=bind` steps, one
  `build_files/scripts/*.sh` fragment per concern. New build-time concerns get
  a new fragment, not edits to existing ones.
- `build_files/kyth_shared/` (mirrored at `src/kyth_shared/`): one small
  module per tunable/preset/feature (e.g. `swappiness.py`, `zram.py`,
  `vrr.py`, `gaming_scan_atomic.py`, `cloud_idempotent.py`). New host-tuning
  logic follows that pattern; state-mutating modules must be idempotent and
  transactional (fully apply or fully roll back). Probe collector, Guardian
  core, and update watcher are native Rust in `src/kyth-shared-rs/`.
- `src/kyth-hub-web/` is the supported System Hub UI. The Python/Qt Hub in
  `build_files/kyth-welcome/` (sources migrating to `src/kyth-welcome/`) is
  transitional; retired VPN/SAML client, network-share helper, and privileged
  socket daemon fixtures were removed in P2 — do not treat them as active
  Python authorities.
- `build_files/kyth-installer/` is the local-only installer driving
  `bootc install to-disk`: `plan_*.py` computes, `partition_ops*.py`/`disk/`
  executes, `recovery.py`/`assurance.py` verifies. High-risk area: add tests
  in `tests/test_kyth_installer_*` alongside changes.
- `build_files/just/kyth.just` imports domain `*.just` files shipped as
  `ujust` recipes in the OS (distinct from the repo-root `Justfile`).
- `tests/` is flat, one file per module/feature; `PYTHONPATH` must include
  `kyth_shared`, `kyth-welcome`, and `kyth-installer`.

## Branches & channels

- `main` → `:latest` (stable). `testing` → `:testing` (active dev).
- Changes affecting boot, login, networking, audio, GPU, updates, installer,
  or privileged helpers need an automated regression test where practical,
  plus a documented manual recovery path where hardware can't be automated.

```bash
sudo bootc switch ghcr.io/kyth-os/kyth:testing
sudo bootc upgrade
```

## Project layout

```text
Dockerfile              # final OS image (layers 2+3)
Justfile                # build orchestration (imports build_files/just/*.just)
build_base/             # layer 1: CachyOS kernel + base Fedora Kinoite 44
build_files/            # layer 2+: packages, tweaks, branding, runtime code
installer/              # live payload customization
tests/                  # flat unittest suite for installer, Hub, shared helpers
docs/                   # architecture, security, hardware, validation docs
.githooks/              # pre-commit / pre-push / prepare-commit-msg
.github/workflows/      # CI: daily rebuilds 10:05 UTC, validation, signing, CVE scans
```

## Key details

- Base: `ghcr.io/ublue-os/kinoite-main:44` (Fedora 44 KDE); CachyOS kernel
  (BORE, sched-ext, BBRv3, NTSYNC); Mesa-git via `xxmitsu/mesa-git` COPR.
- SELinux enforcing (bootc/ostree relabels on every deployment).
- Live ISOs on Cloudflare R2: `kyth-live-latest.iso` / `kyth-live-testing.iso`.
- GitHub: https://github.com/kyth-os/kyth

## Dev rules

- **QThread lifecycle:** workers subclass `TrackedThread`
  (`src/kyth-welcome/kyth_welcome/services/runtime.py`,
  canonical `src/kyth_shared/kyth_shared/desktop/qt_threads.py`), implement
  `cancel`/`stop`, and are stopped + `wait()`ed on teardown (`atexit` +
  `aboutToQuit`). Threads running critical work set `BLOCKS_CLOSE = True`;
  query running threads before closing the main window. Prevents segfaults
  from threads outliving interpreter teardown.
- **Unbounded logging:** cap `QTextEdit` logs
  (e.g. `document().setMaximumBlockCount(5000)`) when streaming long output.
- **Probe cache:** reuse expensive probes (`bootc status`, `flatpak list`,
  `lsblk`) via `probe_cached` with a short TTL (5–30s) instead of fanning out
  identical subprocess calls on rapid UI refresh.
- **Partition identification:** never identify a new partition by `PARTLABEL`
  alone (stale labels from prior installs collide). List partitions before
  `parted mkpart`, diff afterward (e.g. `comm -13`) to isolate the new device.
