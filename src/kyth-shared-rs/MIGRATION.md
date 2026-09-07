# Migrating kyth_shared to Rust

`src/kyth_shared` (Python) is ~200 modules covering everything from GPU
switching and VPN connection management to installer disk partitioning,
SELinux policy, and systemd unit management. Porting all of it in one pass
isn't realistic, and doing it carelessly is actively dangerous — a lot of
it is exactly what CLAUDE.md already calls out as high-risk (installer,
GPU setup, anything privileged). This crate is not that port. It's the
starting point for one, done incrementally, module by module. Most probes
remain read-only; explicit user-requested Hub actions are added only where
their command policy and bounded execution are already covered.

## What's ported so far

The read-only bridges and pure helpers the Kyth Hub's Tauri shell
(`src/kyth-hub-web/src-tauri`) used to shell out to Python subprocesses
for — see `src/kyth-hub-web/src-tauri/src/main.rs`'s `probe_backend`,
`guardian_snapshot`, `hardware_snapshot`, `storage_snapshot` commands:

| This crate | Ports the read path of | Behavior deliberately NOT ported |
|---|---|---|
| `system::probe` | Cache reads/invalidation plus the native bounded collector and atomic cache writer used by the Rust `kyth-probe` binary | The legacy Python collector remains in the source tree only for compatibility tests; it is not installed or used by the image service path. |
| `guardian` | `kyth_shared.guardian` state, recipe policy, bounded repair executors, verification, native extended probes, and the native `kyth-guardian` service CLI's model-assisted decision path when a preinstalled model/runtime is available | Model download/removal remains outside the Hub command surface; unavailable model assets are reported explicitly and never treated as healthy. The legacy Python file is source-only for compatibility tests. |
| `system::runtime_output` | Pure parsers from `kyth_shared.system.runtime_output` | The commands that collect the raw output remain in their owning probes; this module only parses bounded output. |
| `system::bootc_query`, `system::registry`, `system::update_*` | Hub-facing bootc status, registry manifest, update summaries, and the native watcher status/write contract | Image mutation remains an explicit bounded caller action; the installed native Rust `kyth-update-watcher` owns scheduling and status persistence. |
| `system::snapshot` | Read-only Snapper/Btrfs snapshot and bootc deployment timeline | Snapshot creation, deletion, rollback, and cache/state writes remain outside this crate. |
| `system::safe_upgrade_policy` | Rollout-ring config decoding and fixed `/boot` remount/finalize argv projection | The native watcher owns root checks, registry comparison, `/sysroot` free-space gating, non-blocking upgrade locking, firmware staging, bounded `bootc upgrade`, retryable status persistence, and session/network gates. |
| `system::snapshot_autoclean` | Bounded Btrfs quota/Snapper timeline cleanup command planning and filesystem-status normalization | Quota mutation, Snapper cleanup, and deletion remain caller-owned. |
| `system::telemetry_ingest` | MangoHud CSV parsing, game-name derivation, launcher detection, and numeric normalization for `kyth-telem` | The legacy Python parser remains only as a compatibility fixture. |
| `system::telemetry_writer` (`telemetry-writer` feature) | Native `kyth-telem` MangoHud config ownership, SQLite WAL/schema creation, `/proc` game-name enrichment, stable CSV scanning, duplicate suppression, and session/frame ingestion | Modelled real-output fixtures are covered by Rust tests; installed-image acceptance remains explicitly waived for this cutover. |
| `system::gpu` | `kyth_shared.system.gpu` | `loaded_kernel_modules`, `rpm_package_installed`, `query_nvidia_smi` — only `lspci_gpu_lines` had a caller. |
| `system::hardware_policy` | Read-only `hardware_policy` inventory, TOML parsing, selector matching, and evaluation | Policy application, modprobe/scheduler writes, and persisted state/report writes remain Python-owned. |
| `system::storage` | (new — was inline Python in the retired `storage_bridge.py`, not really "kyth_shared") | — |
| `system::boot_health` | read/policy surface plus pure staged/failure/recovery/quarantine state transitions of `kyth_shared.boot_health` | Atomic persistence, boot verification, and rollback execution remain Python-owned. |
| `diagnostics_scrub` | `kyth_shared.diagnostics_scrub.scrub_logs` | Collection, upload, and report composition remain outside the crate. |
| `atomic_io` | `kyth_shared.atomic_io` crash-safe text/bytes/JSON replacement plus read-with-default JSON recovery | Callers still decide which state is safe to persist. |
| `config_loader` | Shared TOML defaults/section/candidate loading behavior from `kyth_shared.config` | Type-specific validation and writes remain owned by each setting module. |
| `health` | `kyth_shared.health` typed reports, severity aggregation, remediation text, JSON/text output | Smoke-check collection remains owned by the caller. |
| `system::battery` | `kyth_shared.battery` config defaults/clamping and sysfs health reads | Hardware charge-limit application remains outside this crate. |
| `system::boot_loader` | `kyth_shared.boot_loader` config/status reads and native boot-timeout writer | `/boot` loader mutation remains privileged and caller-owned. |
| `system::runtime_diagnostics` | Pure deployment, GPU-driver, GPU/Vulkan/session/rollback status, service-state, and live-image interpretation | Raw command collection remains with existing probes. |
| Native diagnostic entry points | `kyth-post-update-check`, `kyth-firstboot-app-status`, `kyth-steam-game-export`, `kyth-hub-desktop-entries`, `kyth-health-check`, `kyth-resume-check`, `kyth-nvidia-status`, `kyth-controller-check`, `kyth-game-boost`, and `kyth-doctor` are Rust binaries built with this crate; they replace the installed Python helpers and packaging generator | Runtime/user acceptance and provider-specific qualification remain separate release gates. |
| `system::controllers` | Controller USB/module/input inventory, variant classification, and bounded optional DualSense probe | Hardware command collection and device mutation remain caller-owned. |
| `system::disk_utils` | Safe integer and device-path normalization | Partition discovery and mutation remain installer-owned. |
| `system::installer_query` | Read-only installer planning queries: GPT/BIOS detection, guided-space calculation, BootCurrent parsing, and Windows NTFS resize-target selection | Command execution, storage probing, journal mutation, and partition/filesystem changes remain installer-owned. |
| `system::installer_source` | Image-reference normalization, source classification, registry-host/OCI-tag parsing, and kernel-specific target derivation | Source verification, network preflight, image downloads, and bootc installation remain installer-owned. |
| `system::sbom` | Offline SBOM diff and CVE severity summaries | No network fetch or vulnerability database update. |
| `system::exe_compat` | Bounded EXE hashing, offline compatibility lookup, filename normalization, and Steam launcher rewriting | Compatibility verdicts remain advisory; the Tauri handler presents warnings and requires explicit confirmation for known limitations. |
| `system::rollback_state` | `kyth_shared.rollback_single_source` staged/rollback state read | Update coordinator writes remain outside this crate. |
| `system::appstore_cache` | Offline AppStream cache status | Catalog refresh remains probe/service-owned. |
| `system::tuning_profile` | Common TOML profile normalization for small tuning modules | Privileged sysctl/cgroup application is not included. |
| `system::app_presets`, `backup_config`, `print_config` | Offline TOML config models and safe persistence | Service activation, backup execution, and printer setup remain external. |
| `system::windows_verify`, `attest`, `thirdparty` | Read-only migration parity, cached attestation metadata, and downloaded-asset discovery | No installer execution or online signature verification. |
| `system::windows_installer` | PE/MSI header inspection, immutable file identity/hash, compatibility assessment, bottle planning/JSON parsing, safe staging, and a fixed-argv Bottles workflow | The existing unprivileged Tauri/React Hub owns presentation, confirmation, progress, and error display; no generic process bridge is exposed. |
| `system::firstboot`, `devcontainers`, `search_config`, `session_config` | First-run markers, status-file serialization, Distrobox/search config, and browser credential-store transforms; the native `kyth-firstboot-app-status` CLI owns the installed app-status entry point | Desktop service application remains outside the shared crate. |
| `system::process` | Session helpers, ANSI/progress formatting, and bounded argv execution | Caller still owns command allowlists and operation-specific timeout policy. |
| `system::display`, `hdr` | KScreen output parsing, EDID HDR hints, and per-display HDR config | KScreen/KWin mutation remains guarded by existing action paths. |
| `system::gaming_master` | Gaming profile normalization, thermal/battery safety evaluation, and native gaming-master dispatch | Composed tuning application and snapshot/rollback remain outside Rust. |
| `system::gaming_snapshot` | Pre-gaming Snapper/Btrfs fallback command planning and result evaluation | Snapshot creation and filesystem mutation remain caller-owned. |
| `system::sysctl_profiles` and `kyth-tunable-rs` | Shared profile/config/drop-in behavior and the native dispatcher for all 49 sysctl-backed plus all 45 module-specific tuning modules | The native binary owns the bounded `sysctl --system` refresh, environment/service projections, the zswap modprobe drop-in, and the read-only Windows migration report; the legacy compatibility dispatcher remains only as a rollback fixture for older images. |
| `system::sysctl_compose` | Canonical base/gaming/network sysctl TOML loading, duplicate detection, deterministic rendering, and explicit tier-file writes | Legacy-file cleanup, sysctl application, and build CLI exit policy remain outside Rust. |
| `system::hdr_store`, `hdr_per_game` | HDR preserve preference and native hdr-store dispatch plus native per-game peak/ITM config lookup | KWin/display mutation and driver probing remain outside the shared crate. |
| `system::work_cache` | Work-cache config normalization, native dispatch, and reversible tmpfiles/systemd rendering | tmpfiles/systemd activation and bind mounts remain outside Rust. |
| `system::bluetooth` | Bluetooth LE Audio per-device TOML presets | BlueZ/device mutation remains outside Rust. |
| `system::ananicy` | Ananicy profile normalization and explicit gaming rule rendering | Service activation and process scheduling remain outside Rust. |
| `system::flatpak_trim` | Flatpak trim preference and service-presence status | Unit/timer generation and Flatpak execution remain service-owned. |
| `system::quicksettings` | Brightness/tile preference normalization and persistence | D-Bus brightness application remains outside the shared crate. |
| `system::perf_gate` | Performance gate config, native dispatch, and recent JSONL p95 regression comparison | Benchmark execution and ledger writes remain outside Rust. |
| `system::perf_audit` | Stable line-oriented performance-audit text projection, key ordering, and native gaming-audit dispatch | Live tunable collection, probe-cache writes, and systemd queries remain outside Rust. |
| `system::driver_config`, `gpu_power` | Graphics driver and GPU power preference normalization/persistence plus native gpu-power dispatch | Driver installation and `/sys` power-level writes remain outside Rust. |
| `system::readahead` | Readahead preference normalization and native tunable-dispatcher status/profile persistence | Filesystem fadvise application remains outside the shared crate. |
| `system::btrfs_autotune`, `overlay`, `podman_btrfs` | Btrfs autotune config/script and native Podman metacopy/storage-driver config/drop-in dispatch and rendering | Timer/service activation and filesystem/container runtime operations remain outside Rust. |
| `system::btrfs_perf` | Btrfs performance profile normalization, mount-option rendering, and explicit drop-in persistence | Remounts, systemd reloads, and filesystem operations remain caller-owned. |
| `system::io_tune` | I/O profile normalization, native tunable dispatch, and explicit udev rule rendering | udev reload and device mutation remain outside Rust. |
| `system::office`, `privacy`, `signing` | Office association, privacy, and signing preference models plus Git config projection | Desktop MIME activation, privacy policy application, and signing commands remain outside Rust. |
| `system::memory_tune` | RAM-tier selection and deterministic sysctl/zram configuration content | Boot-time zram setup and sysctl application remain outside Rust. |
| `system::performance`, `system_probe` | CPU topology, Gamescope argument shaping, and firewall/SELinux/Secure Boot/autologin parsing | Active performance writes and live command collection remain outside Rust. |
| `system::numa`, `flatpak_prefetch`, `distrobox_cache`, `quadlet` | NUMA config model and native numa dispatch, Flatpak prefetch, Distrobox cache, and Quadlet config models | CPU affinity, systemd/timer activation, mounts, and container execution remain outside Rust. |
| `system::update_coordinator` | Locked atomic boot-health/staged-update transactions and convenience wrappers for the shared pure transitions | Policy transition choice, boot verification, rollback execution, and upgrade execution remain caller/service-owned. |
| `system::zswap` | Zswap profile/compressor/zpool normalization and sysctl/modprobe rendering | Module loading and active swap policy remain outside Rust. |
| `system::telemetry_opt` | Telemetry enable/collector filtering, native dispatch, and auditable purge state | Collector execution and telemetry transport remain outside Rust. |
| `system::input_preset`, `rgb_preset`, `power_preset`, `steam_input`, `overlay_preset` | Offline device/game preset models, clamping, persistence, and overlay environment projection | libinput/OpenRGB/Steam/overlay runtime mutation remains outside Rust. |
| `system::preference_presets` | Fonts, locale, OOMD, immutable `/etc` overlay, and native Steam deadzone/SELinux/OOM gaming config dispatch | Desktop/system policy application and overlay activation remain outside Rust. |
| `system::shader_tmpfs` | Shader tmpfs config normalization and reversible tmpfiles/systemd rendering | Mount, bind, persistence, and systemd activation remain outside Rust. |
| `system::service_preferences` | Plymouth, shader-cache hashing/status, polkit rule rendering, and SCX preset models | Service activation, polkit installation, and cache preheating remain outside Rust. |
| `system::audio_network` | PipeWire latency and network DoT/firewall preference models plus deterministic drop-in projections | PipeWire reload, resolved/firewalld mutation, and TTL markers remain outside Rust. |
| `system::runtime_preferences` | Trim, UKSM, journald, IRQ, and FS-Cache config models plus reversible generated snippets and native trim-tune/uksmd/irq-tune/fscache/journal-tune dispatch | Service activation, CPU autodetection, and active filesystem/scheduler changes remain outside Rust. |
| `system::gaming_kargs` | Per-game HDR preferences and native kernel-argument config/drift dispatch | Gamescope latency setup, DMI-specific mutation, and rpm-ostree kargs changes remain outside Rust. |
| `system::display_policy` | VRR/night-colour config normalization, persistence, and KWin policy mapping | KWin/KScreen/D-Bus mutation remains outside Rust. |
| `system::plasma_hdr` | HDR/VRR preset settings, bounded KWin argv projection, and section-aware status parsing | KWin writes, output HDR application, and D-Bus reconfiguration remain caller-owned. |
| `system::display_live` | Bounded KScreen inspection plan, debounce policy, and mode readback evaluation | Live display mutation remains a guarded desktop action. |
| `system::vpn_saml` | VPN profile validation, fixed openconnect argv/stdin projection, bounded SAML URL/cookie parsing, same-origin ACS validation, and redacted log projection | The Tauri caller owns process signaling, worker lifecycle, and the embedded SAML webview; the former standalone Python/PySide6 app and launcher were removed in P2. |
| Hub VPN/SAML boundary | Typed `open_vpn_app`, profile summary, native connect/disconnect jobs, SAML webview, and cookie handoff in the Tauri shell | The standalone Python client and source fixture were removed in P2; real IdP/provider qualification remains an operational gate. |
| Hub privileged action boundary | Tauri validation plus the fixed `/run/kyth/privileged.sock` operation allowlist, native Rust `kyth-privileged` daemon, and native Rust `/usr/libexec/kyth-network-share` executor | The former Python socket daemon and network-share wrapper were removed in P2; the installed root boundary keeps peer credentials, fixed argv, credential isolation, mount-unit ownership, bounded execution, and audit behavior. |
| `system::network_services` | Cloud-drive and Tailscale offline preference models and persistence | rclone mounts, Tailscale control, and credential/network operations remain outside Rust. |
| `system::extended_preferences` | Fan curves, Fcitx/PipeWire gaming, PCIe/PSI, Wine sync, mimalloc, sccache, and shader-cache preference/rendering helpers; native tunable dispatch now owns the six corresponding writers | Hardware probing, service activation, preload application, and active device writes remain outside Rust. |
| `system::desktop_preferences` | Flatpak override arguments, Plasma drift section flattening, and window-snap preference models | Flatpak/KDE mutation and session reconfiguration remain outside Rust. |
| `system::desktop_shortcuts` | Application-id sanitizing and Steam/web-app/Kali desktop-file transformations; the native `kyth-steam-game-export` CLI owns the installed Steam export entry point and its bounded filesystem/cache work | No arbitrary launcher execution; all cache refresh commands remain fixed and bounded. |
| `system::desktop_plasma` | Launcher availability filtering and bounded Plasma/qdbus/kwriteconfig argv projections | Command discovery, command execution, and Plasma mutation remain caller-owned. |
| `kyth-user-polish` | Native first-login folder/MIME/KDE/desktop-polish service, including the versioned stamp/lock and Dolphin places XML contract | The Python `user_polish` and `user_polish_flatpak` files remain source-only parity fixtures until image rollback qualification closes. |
| `system::akmods_lock` | Single-flight bounded lock for NVIDIA module builds | The lock does not start or supervise an akmods build. |
| `system::qualification` | Acceptance sentinel parsing, qualification reports, and regression budgets | Probe, benchmark, VM, and deployment execution remain caller-owned. |
| `system::vm_acceptance` | Acceptance reference validation, bootc/ostree JSON decoding, state normalization, and event framing | Guest commands, power control, and smoke-check execution remain caller-owned. |
| `system::role_preset` | Offline role preset defaults, TOML loading, list overrides, and persistence | Package/container/extension installation remains an explicit action. |
| `system::wayland` | Wayland/software-compositor policy, DRM detection, greeter-session config, session classification, and argv projection | Session file writes and compositor startup remain caller-owned. |
| `system::explorer_preset` | Dolphin double-click/preview/drives-on-desktop preference model, plus the native `kwriteconfig5` apply step (the native `kyth-apply-explorer` binary owns this; dead on the shipped Kinoite 44 image since `kwriteconfig5` is not installed there) | Desktop-session reconfiguration beyond the fixed `kwriteconfig5` writes remains outside Rust. |
| `system::tunable_registry` | Declarative tunable catalog loading, name normalization, and safe lookup/listing used by the native dispatcher and its rollback fixture | All current registry entries resolve to native Rust; the compatibility fixture remains available for forward-compatible additions and older-image rollback. |
| `system::ai_plan` | Offline deterministic repair-action planning and tolerant serialized-plan parsing/order | Action execution, model calls, and network access remain caller/service-owned. |
| `system::ai_dev` | Environment-derived AI/developer config and Distrobox enter/create argv projection with GPU flags | Container creation, provisioning, model downloads, lifecycle commands, and Ollama remain caller-owned. |
| `containers` | Deterministic Distrobox tool-wrapper script generation | Wrapper execution, container creation, and provisioning remain caller-owned. |
| `cloud_idempotent` | Rclone sync-key, dry-run text, and explicit manifest serialization/persistence | Rclone execution and remote/network operations remain caller-owned. |
| `work_migration` | Idempotent single-file copy-if-newer behavior with atomic destination replacement | Migration discovery, directory policy, and workflow orchestration remain caller-owned. |
| `system::perf_policy` | Offline AI performance sample model, read-only pressure/battery/power-input parsing, deterministic SCX/sysctl/GPU policy selection, and p95 rollback gate | Native `kyth-ai-perfd` owns collection and best-effort policy application; optional model calls and privileged write failures remain explicitly degraded. |
| `system::scheduler_arbiter` | Scheduler arbiter configuration normalization, native dispatch, single-writer desired-state policy, and flag projection | Service/process detection, gamemode rewriting, and scheduler activation remain caller/service-owned. |
| `system::gaming_cgroup` | Declarative gaming cgroup configuration normalization and systemd slice drop-in rendering | Drop-in writes, systemd activation, and live process placement remain caller/service-owned. |
| `system::gaming_truth` | Offline compatibility payload parsing, Steam manifest discovery, normalized library lookup, and compatibility classification | Remote compatibility refresh, network access, and UI ownership remain caller/service-owned. |
| `system::gaming_versions` | Pinned UMU/Proton-CachyOS/Mesa gaming metadata loading, candidate-path/cache/env precedence, and OCI-label projection | Remote version resolution and runtime cache writes remain build/runtime-owned. |
| `system::gaming_activity`, `system::gaming_kargs` | Gaming-session trigger precedence, GameMode/process-name interpretation, and per-game launch environment projection | Login-session/D-Bus/`/proc` collection and game launching remain caller-owned. |
| `system::clip_quick` | Clipboard-history/tile config normalization and fixed Klipper argv projection | Klipper configuration application remains an explicit desktop action. |
| `system::kwin_latency` | KWin latency profile normalization and generated drop-in/environment projection | File installation, KWin reload, and session mutation remain caller-owned. |
| `system::app_suggestions` | Packaged EXE-handler application database loading, regex lookup, and embedded fallback behavior | The native `kyth-exe-handler` launcher forwards only a file path into the existing Tauri/React Hub dialog, which owns the UI. |
| `system::scaling` | Fractional per-output scaling TOML normalization, persistence, and KWin data projection | KScreen discovery, ICC deployment, and display mutation remain guarded desktop actions. |
| `system::system_audit` | Pure audit aggregation, compact summary formatting, and native system-audit dispatch | Perf/snapshot collection, cache writes, and live system probing remain outside Rust. |
| `system::save_cloud` | Offline per-game save-cloud TOML model, defaults, and atomic persistence | Save discovery, restic/rclone execution, network access, and credential handling remain caller-owned. |
| `system::maintenance` | Bounded Steam deduplication-target discovery, filesystem capability classification, and deterministic duperemove argv projection | Trash deletion, secure hash-database creation, and deduplication execution remain caller-owned. |
| `system::plymouth` | Plymouth policy constants, image discovery, fingerprints, and pure initramfs inspection | `dracut`, mount remounts, and initramfs writes remain Python-owned. |
| `system::perf_transaction` | Performance transaction plan and dry-run/apply rollback evaluation | Backup copying and privileged `sysctl` execution remain caller-owned. |
| `system::polish_manifest` | Declarative folders, metadata, and MIME-default manifest | Desktop filesystem creation and MIME database writes remain caller-owned. |
| `system::smoke_check` | Typed smoke-check rows, filesystem/content checks, command-presence projection, and exit-status aggregation | Live command/service probes, console/JSON output policy, and process exit remain caller-owned. |
| repos | Third-party repository JSON decoding and deterministic yum-repo rendering | Repository enablement, key import, and package-manager mutation remain explicit actions. |
| transfer | Shared size parsing, human-readable formatting, `/proc/net/dev` receive-byte parsing, and the rolling throughput/ETA tracker used by installer/welcome surfaces | Network polling orchestration, download execution, and UI state remain caller-owned. |
| url_encode | RFC 3986 query-value encoding used by native feedback/report surfaces | Destination URL construction and browser launching remain caller-owned. |
| secret_scan | Pure high-confidence private-key/token pattern matching and binary-file exclusion for CI secret checks | Git enumeration, report output, and CI failure policy remain script-owned. |
| setup_transfer | Setup-archive manifest schema, restore-path allowlist, and support-friendly preview summary | Archive extraction/restoration, Flatpak/network discovery, and credential handling remain caller-owned. |
| desktop_polish | Declarative first-login folder/MIME manifest and owned desktop-shortcut drift detection | KDE configuration writes, folder creation, and user-session commands remain caller-owned. |
| release_identity | Canonical ISO release identity validation and artifact-name projection, with a Rust CLI wrapper | Git HEAD lookup and GitHub output-file writing remain CLI orchestration; the existing Python workflow remains authoritative until runner-toolchain adoption is verified. |
| release_publish | Release channel presentation, asset/release URL projection, and static `gh release` argv planning | GitHub API calls, artifact uploads, notes-file writes, and release deletion remain release orchestration. |
| build_metadata | Typed image, release-artifact, and supply-chain metadata projections matching the build-script JSON contracts | File writes, artifact inspection, and workflow orchestration remain outside the shared crate. |
| build_checks | RPM manifest extraction, coverage-floor ratcheting, base-image label/digest decisions, exact OCI digest validation, and stable source hashing | Docker/GitHub inspection, report writes, and CI exit-code policy remain outside the shared crate. |
| build_metrics | Optimization-report static metric calculation, budget failure projection, and JSON report assembly including optional runtime metrics | Filesystem traversal, runtime probes, report-file writes, and CI exit policy remain build-script orchestration. |
| commands | Explicit argv validation, `ujust` recipe validation, environment filtering, and sensitive-argument redaction | Process execution, spawning, timeouts, and caller-specific failure policy remain outside the shared crate. |
| diagnostic_report | Typed diagnostic result rows, warning/failure aggregation, exit-status projection, and human-readable rendering | Live probes, notifications, report-file writes, and process exit remain caller-owned. |
| sarif | Changed-file SARIF finding filtering, source-suppression handling, and safe file-URI projection | Report collection, file I/O, and CI exit-code policy remain caller-owned. |
| doctor | Read-only health score calculation and local evidence collection for kernel, hardware, memory, filesystem, scheduler, and desktop-stack status | Repair execution and live notifications remain caller-owned. |

Most functions ported are pure reads against on-disk state, parsers, or a
single bounded subprocess call. A few explicit Hub actions are exceptions:
Guardian repairs, firmware update helpers, PipeWire configuration, and
software catalog import/install helpers are user-invoked and bounded; they
must not be expanded into generic command execution. The native Guardian
service now runs its extended multi-probe sweep, while installer partitioning
and other high-risk writers remain outside this crate. Keep those boundaries
explicit when adding another port.

## How more of it moves over

One module (or one function) at a time, in this order of preference:

1. **Read-only first.** A function that reads a file, a cache, or runs one
   cheap command and returns data is a good candidate. A function that
   writes state, executes a repair, or runs a probe sweep is not — do
   those later, once there's a real reason (a Rust caller that needs it)
   and real test coverage proving parity with the Python original.
   New host-tuning logic should land in the Rust shared crate and its native
   dispatcher/binary pattern; do not add new standalone Python modules under
   `kyth_shared` for installed runtime behavior.
2. **Port faithfully, not "improved."** Match the Python original's
   behavior exactly, including its quirks (see `system::gpu`'s doc comment
   for a real example — `lspci_gpu_lines`'s substring-match gotcha is
   preserved on purpose). Fix bugs as a separate, deliberate, reviewed
   change — not silently as part of a port, where it's easy to miss that
   the behavior changed at all.
3. **Test parity, not just "it compiles."** Every module here has
   `#[cfg(test)]` unit tests exercising the same scenarios the retired
   Python bridge tests (`tests/test_kyth_hub_shell_bridges.py`, since
   deleted — check git history for the shape) covered, using an explicit
   path/state parameter rather than mutating process-global env vars (see
   `system::probe::read_section_in` / `guardian::load_state_from`) — keeps
   tests parallel-safe and avoids flakiness from shared mutable env state.
4. **The Python module stays source-only after its Rust service port is proven.**
   Remaining Python callers (including independent helpers and ujust recipes)
   are not treated as migrated merely because the Tauri shell has a Rust read
   or launch wrapper. The installed probe, Guardian, update-watcher, telemetry,
   VPN, privileged-socket, and network-share service paths now use native Rust;
   remaining compatibility fixtures for other services are tracked separately
   and are not active Hub authorities.

## Why a separate crate instead of folding into kyth-hub-shell

Because the Tauri shell isn't going to be the only Rust consumer forever —
keeping this as its own crate (`kyth-shared`, a plain path dependency, no
workspace yet — see `src-tauri/Cargo.toml`) means the next Rust thing that
needs `kyth_shared` reads doesn't need to depend on a GUI shell binary to
get them.
