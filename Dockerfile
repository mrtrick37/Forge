
ARG BASE_IMAGE=localhost/kyth-base:stable
# CI pins this to a digest-qualified ref (ghcr.io/...@sha256:...) via
# --build-arg BASE_IMAGE="${STEPS_UPSTREAM_BASE_OUTPUTS_PINNED}" in
# .github/workflows/build.yml; local `localhost` alias is intentional for
# developer builds. validate.sh warns if a release build lacks a digest.
# Declared before ANY FROM (this line is it) so it stays in scope for the
# main stage's own FROM ${BASE_IMAGE} below, across the hub-web-builder
# stage in between — an ARG's global scope survives an unrelated FROM,
# it just can't be *used inside* that stage's own instructions without a
# bare re-declare (see the "Base Image" one right before FROM ${BASE_IMAGE}).

# ── Kyth Hub web shell (React + Tauri) builder stage ──────────────────────
# Separate stage so the Rust/Node toolchain and Tauri Linux build
# prerequisites (webkit2gtk-devel & co — see src/kyth-hub-web/README.md for
# the same list in the local dev workflow) never land in the final image —
# only the compiled binary does (COPY --from'd into the main stage further
# down). The frontend's dist/ is embedded into the binary at compile time
# (see tauri.conf.json's frontendDist), so nothing besides the one binary
# needs installing alongside it — but only because src-tauri/Cargo.toml
# makes `custom-protocol` a default feature. Without it Tauri generates a
# *dev* context that ignores frontendDist and points the webview at devUrl,
# and the plain `cargo build --release` below has no way to opt in the way
# `tauri build` does. See that Cargo.toml's [features] comment.
FROM registry.fedoraproject.org/fedora:44 AS hub-web-builder
RUN dnf5 install -y --setopt=install_weak_deps=False --skip-unavailable \
        cargo rust nodejs npm gcc gcc-c++ pkgconf-pkg-config \
        webkit2gtk4.1-devel javascriptcoregtk4.1-devel libsoup3-devel gtk3-devel dbus-devel && \
    dnf5 clean all
# kyth-shared-rs is a sibling of kyth-hub-web (src/kyth-shared-rs, not
# under src/kyth-hub-web) — src-tauri's Cargo.toml depends on it via a
# `../../kyth-shared-rs` path dependency, so it needs copying to the same
# relative position here, not folded into the kyth-hub-web COPY below.
COPY src/kyth-shared-rs /build/kyth-shared-rs
# system::app_suggestions embeds build_files/exe-handler-apps.json via
# include_str!("../../../../build_files/exe-handler-apps.json"), a path
# written relative to the crate's real position in the repo
# (src/kyth-shared-rs/src/system/…, four levels up from src/system lands at
# the repo root). Copied here one level shallower (/build/kyth-shared-rs
# instead of /build/src/kyth-shared-rs), that same four-level-up path
# resolves to /build_files, not /build/build_files — copy the file there to
# match rather than editing the crate (whose path must stay correct for the
# real-repo checkout `cargo test`/check-hub-web-shell.sh build against
# directly).
COPY build_files/exe-handler-apps.json /build_files/exe-handler-apps.json
# `main.rs` embeds the bundled compatibility catalog using a path relative to
# `/build/kyth-hub-web/src-tauri/src`: ../../../kyth-welcome/... resolves to
# `/build/kyth-welcome/...`.  Keep that source-relative layout in
# the builder stage so the release container compile sees the same catalog as
# the repository build.
COPY src/kyth-welcome /build/kyth-welcome
COPY src/kyth-hub-web /build/kyth-hub-web
WORKDIR /build/kyth-hub-web
RUN --mount=type=cache,id=kyth-hub-web-npm,target=/root/.npm \
    npm ci && npm run build
WORKDIR /build/kyth-hub-web/src-tauri
RUN --mount=type=cache,id=kyth-hub-shell-cargo-registry,target=/root/.cargo/registry \
    --mount=type=cache,id=kyth-hub-shell-target,target=/build/kyth-hub-web/src-tauri/target \
    cargo build --release --locked && \
    cp target/release/kyth-hub-shell /build/kyth-hub-shell && \
    (cd /build/kyth-shared-rs && cargo build --release --locked --features telemetry-writer --bin kyth-probe --bin kyth-guardian --bin kyth-update-watcher --bin kyth-network-share --bin kyth-telem --bin kyth-privileged --bin kyth-post-update-check --bin kyth-firstboot-app-status --bin kyth-steam-game-export --bin kyth-hub-desktop-entries --bin kyth-safe-upgrade --bin kyth-bootc-guard --bin kyth-finalize-staged --bin kyth-btrfs-maint --bin kyth-ai-perfd --bin kyth-perf-gate-rs --bin kyth-doctor --bin kyth-health-check --bin kyth-smoke-check --bin kyth-resume-check --bin kyth-nvidia-status --bin kyth-controller-check --bin kyth-creator-check --bin kyth-exe-compat --bin kyth-snapshot-timeline --bin kyth-print-check --bin kyth-windows-verify --bin kyth-vm-acceptance-guest --bin kyth-tunable --bin kyth-tunable-rs --bin kyth-game-boost --bin kyth-configure-session --bin kyth-set-resolution --bin kyth-set-kickoff-icon --bin kyth-greeter-compositor --bin kyth-config-apply --bin kyth-apply-scx-preset --bin kyth-apply-explorer --bin kyth-apply-desktop-layout) && \
    cp /build/kyth-shared-rs/target/release/kyth-probe /build/kyth-probe && \
    cp /build/kyth-shared-rs/target/release/kyth-guardian /build/kyth-guardian && \
    cp /build/kyth-shared-rs/target/release/kyth-update-watcher /build/kyth-update-watcher && \
    cp /build/kyth-shared-rs/target/release/kyth-network-share /build/kyth-network-share && \
    cp /build/kyth-shared-rs/target/release/kyth-telem /build/kyth-telem && \
    cp /build/kyth-shared-rs/target/release/kyth-privileged /build/kyth-privileged && \
    cp /build/kyth-shared-rs/target/release/kyth-post-update-check /build/kyth-post-update-check && \
    cp /build/kyth-shared-rs/target/release/kyth-firstboot-app-status /build/kyth-firstboot-app-status && \
    cp /build/kyth-shared-rs/target/release/kyth-steam-game-export /build/kyth-steam-game-export && \
    cp /build/kyth-shared-rs/target/release/kyth-hub-desktop-entries /build/kyth-hub-desktop-entries && \
    cp /build/kyth-shared-rs/target/release/kyth-safe-upgrade /build/kyth-safe-upgrade && \
    cp /build/kyth-shared-rs/target/release/kyth-bootc-guard /build/kyth-bootc-guard && \
    cp /build/kyth-shared-rs/target/release/kyth-finalize-staged /build/kyth-finalize-staged && \
    cp /build/kyth-shared-rs/target/release/kyth-btrfs-maint /build/kyth-btrfs-maint && \
    cp /build/kyth-shared-rs/target/release/kyth-configure-session /build/kyth-configure-session && \
    cp /build/kyth-shared-rs/target/release/kyth-set-resolution /build/kyth-set-resolution && \
    cp /build/kyth-shared-rs/target/release/kyth-set-kickoff-icon /build/kyth-set-kickoff-icon && \
    cp /build/kyth-shared-rs/target/release/kyth-greeter-compositor /build/kyth-greeter-compositor && \
    cp /build/kyth-shared-rs/target/release/kyth-config-apply /build/kyth-config-apply && \
    cp /build/kyth-shared-rs/target/release/kyth-apply-scx-preset /build/kyth-apply-scx-preset && \
    cp /build/kyth-shared-rs/target/release/kyth-apply-explorer /build/kyth-apply-explorer && \
    cp /build/kyth-shared-rs/target/release/kyth-apply-desktop-layout /build/kyth-apply-desktop-layout && \
    cp /build/kyth-shared-rs/target/release/kyth-ai-perfd /build/kyth-ai-perfd && \
    cp /build/kyth-shared-rs/target/release/kyth-perf-gate-rs /build/kyth-perf-gate-rs && \
    cp /build/kyth-shared-rs/target/release/kyth-doctor /build/kyth-doctor && \
    cp /build/kyth-shared-rs/target/release/kyth-health-check /build/kyth-health-check && \
    cp /build/kyth-shared-rs/target/release/kyth-smoke-check /build/kyth-smoke-check && \
    cp /build/kyth-shared-rs/target/release/kyth-resume-check /build/kyth-resume-check && \
    cp /build/kyth-shared-rs/target/release/kyth-nvidia-status /build/kyth-nvidia-status && \
    cp /build/kyth-shared-rs/target/release/kyth-controller-check /build/kyth-controller-check && \
    cp /build/kyth-shared-rs/target/release/kyth-creator-check /build/kyth-creator-check && \
    cp /build/kyth-shared-rs/target/release/kyth-exe-compat /build/kyth-exe-compat && \
    cp /build/kyth-shared-rs/target/release/kyth-snapshot-timeline /build/kyth-snapshot-timeline && \
    cp /build/kyth-shared-rs/target/release/kyth-print-check /build/kyth-print-check && \
    cp /build/kyth-shared-rs/target/release/kyth-windows-verify /build/kyth-windows-verify && \
    cp /build/kyth-shared-rs/target/release/kyth-vm-acceptance-guest /build/kyth-vm-acceptance-guest && \
    cp /build/kyth-shared-rs/target/release/kyth-tunable /build/kyth-tunable && \
    cp /build/kyth-shared-rs/target/release/kyth-tunable-rs /build/kyth-tunable-rs && \
    cp /build/kyth-shared-rs/target/release/kyth-game-boost /build/kyth-game-boost

# Base Image
ARG BASE_IMAGE
FROM ${BASE_IMAGE}
SHELL ["/bin/bash", "-o", "pipefail", "-c"]
# Override upstream OCI labels so downstream tooling (lorax/bootc) sees KythOS product metadata
LABEL org.opencontainers.image.title="KythOS"
LABEL org.opencontainers.image.version="44"
LABEL org.opencontainers.image.description="KythOS — atomic gaming and dev workstation built on Fedora Kinoite"
LABEL org.opencontainers.image.licenses="Apache-2.0"
LABEL org.opencontainers.image.source="https://github.com/kyth-os/kyth"
LABEL org.opencontainers.image.documentation="https://github.com/kyth-os/kyth"
LABEL org.osbuild.product="KythOS"
LABEL org.osbuild.version="44"
LABEL org.osbuild.branding.release="KythOS 44"

### MODIFICATIONS
# Fedora 44 ships scx_rusty 0.5.4, whose pre-upstream sched_ext BPF ABI is
# incompatible with the kernel 7.1 interface. Keep SCX opt-in until KythOS
# ships a scheduler build coordinated with its CachyOS kernel.
ARG ENABLE_SCX=0
ARG ENABLE_MESA_GIT=0
ARG ENABLE_GAMING_PERIPHERALS=0
ARG ENABLE_VIRTUALIZATION_HOST=0
ARG ENABLE_KSM=0
ARG GAMING_VERSIONS_HASH=unset
LABEL org.kyth.profile.gaming-peripherals="${ENABLE_GAMING_PERIPHERALS}"
LABEL org.kyth.profile.virtualization-host="${ENABLE_VIRTUALIZATION_HOST}"
LABEL org.kyth.profile.ksm="${ENABLE_KSM}"
LABEL org.kyth.gaming-versions="${GAMING_VERSIONS_HASH}"

# Build cache boundary: all RPM package installs (~2-3 GB). This layer selects
# the package set and is source-hash/base-image cached. The date-busted upgrade
# layer later refreshes every installed RPM plus the coordinated kernel stack.
ARG RPM_SET_HASH=unset
# Published layer boundaries are defined later by legacy-rechunk metadata.
RUN --mount=type=bind,source=build_files/kyth_shared,target=/ctx/kyth_shared \
    --mount=type=bind,source=build_files/config,target=/ctx/config \
    --mount=type=bind,source=build_files/scripts/packages-static.sh,target=/ctx/packages-static.sh \
    --mount=type=bind,source=build_files/scripts/packages,target=/ctx/packages \
    --mount=type=bind,source=build_files/scripts/lib,target=/ctx/lib \
    --mount=type=bind,source=build_files/RPM-GPG-KEY-microsoft,target=/ctx/RPM-GPG-KEY-microsoft \
    --mount=type=bind,source=build_files/RPM-GPG-KEY-google-antigravity,target=/ctx/RPM-GPG-KEY-google-antigravity \
    --mount=type=cache,id=kyth-var-cache,target=/var/cache \
    --mount=type=cache,id=kyth-var-log,target=/var/log \
    --mount=type=tmpfs,dst=/tmp \
    : "cache-bust:rpm=${RPM_SET_HASH}" && \
    PYTHONPATH="/ctx/kyth_shared" \
    ENABLE_GAMING_PERIPHERALS="${ENABLE_GAMING_PERIPHERALS}" \
    ENABLE_VIRTUALIZATION_HOST="${ENABLE_VIRTUALIZATION_HOST}" \
    ENABLE_KSM="${ENABLE_KSM}" \
    ENABLE_SCX="${ENABLE_SCX}" \
    bash /ctx/packages-static.sh

# Proton-CachyOS is an offline fallback for fresh installs. The build must use
# the exact release tag resolved by CI; the mutable user-side updater may fetch
# newer versions later while retaining a rollback copy.
ARG PROTON_CACHYOS_VER
RUN --mount=type=bind,source=build_files/scripts/proton-cachyos.sh,target=/ctx/proton-cachyos.sh \
    --mount=type=bind,source=build_files/scripts/lib,target=/ctx/lib \
    --mount=type=secret,id=github_token \
    test -n "${PROTON_CACHYOS_VER}" && \
    PROTON_CACHYOS_VER="${PROTON_CACHYOS_VER}" bash /ctx/proton-cachyos.sh

# Third-party binary — umu launcher. Exact tags are resolved once by CI and
# used for both cache identity and downloads; installers never re-resolve
# "latest" inside the build.
ARG THIRDPARTY_VERSIONS_HASH=unset
ARG GAMING_VERSIONS_HASH=unset
ARG UMU_VERSION
RUN --mount=type=bind,source=build_files/scripts/thirdparty.sh,target=/ctx/thirdparty.sh \
    --mount=type=bind,source=build_files/scripts/thirdparty,target=/ctx/thirdparty \
    --mount=type=bind,source=build_files/scripts/lib,target=/ctx/lib \
    --mount=type=tmpfs,dst=/tmp \
    --mount=type=secret,id=github_token \
    : "cache-bust=${THIRDPARTY_VERSIONS_HASH}" && \
    UMU_VERSION="${UMU_VERSION}" \
    bash /ctx/thirdparty.sh

# Plymouth boot splash + initramfs rebuild.
# COPY (not bind-mount) is intentional: COPY includes file content hashes in the
# cache key, so the expensive dracut rebuild only reruns when the splash assets
# actually change — not on every daily dnf upgrade. Bind mounts do NOT contribute
# to the BuildKit cache key and would silently ship a stale cached splash.
# Kernel packages are excluded from ordinary dnf upgrades and updated as one
# coordinated stack during package assembly; the later kernel-repair layer
# validates the resulting latest kernel and initramfs. Sits after the large Proton-CachyOS/thirdparty download layers
# (which it does not depend on) so splash tweaks don't re-pull them, and before
# the BUILD_DATE cache-bust layer.
ARG PLYMOUTH_HASH=unset
COPY build_files/plymouth/kyth.plymouth             /tmp/kyth-plymouth/kyth.plymouth
COPY build_files/plymouth/kyth.script               /tmp/kyth-plymouth/kyth.script
COPY build_files/branding/kyth-logo-transparent.svg /tmp/kyth-branding/kyth-logo-transparent.svg
COPY build_files/branding/transparent-watermark.svg /tmp/kyth-branding/transparent-watermark.svg
COPY build_files/scripts/plymouth-setup.sh          /tmp/plymouth-setup.sh
COPY build_base/plymouth/kyth-plymouth-configure    /tmp/kyth-plymouth-configure
COPY build_files/scripts/plymouth-branding-guard.sh /tmp/plymouth-branding-guard.sh
RUN : "cache-bust:plymouth=${PLYMOUTH_HASH}" && \
    bash /tmp/plymouth-setup.sh && \
    rm -rf /tmp/kyth-plymouth /tmp/kyth-branding /tmp/plymouth-setup.sh /tmp/kyth-plymouth-configure /tmp/plymouth-branding-guard.sh

# kyth-vscode-wallet and the other helpers below are needed by both
# sysconfig-static and sysconfig layers. COPY once so neither layer needs a
# redundant bind-mount. sysconfig.sh removes these from /ctx once installed
# (see its tail) so they don't linger as duplicate content in the final image.
COPY build_files/kyth-vscode-wallet build_files/game-performance build_files/kyth-ntfs-repair build_files/kyth-shader-preheat build_files/kyth-sched-arbiter build_files/kyth-power-arbiter build_files/kyth-power-arbiter.service build_files/kyth-storage-gate build_files/kyth-readahead-hint build_files/kyth-game-launch build_files/kyth-shader-prune build_files/kyth-tunable /ctx/

# Install the shared Python distribution for runtime scripts.
COPY build_files/kyth_shared /tmp/kyth-shared-package
RUN python3 -m pip install \
        --no-cache-dir \
        --no-deps \
        --no-build-isolation \
        --prefix=/usr \
        /tmp/kyth-shared-package && \
    rm -rf /tmp/kyth-shared-package


# Static system configuration — sysctl, kernel modules, PipeWire, Proton env
# vars, gamemode, MangoHud, vkBasalt, bluetooth, and kyth-* service units.
# Hash-gated — only re-runs when sysconfig-static.sh or sysconfig/ or data/
# change. Keeps the post-upgrade layer chain short and avoids users pulling
# a new sysconfig layer when only packages changed.
COPY --from=hub-web-builder --chmod=0755 /build/kyth-finalize-staged /usr/libexec/kyth-finalize-staged
COPY --from=hub-web-builder --chmod=0755 /build/kyth-tunable-rs /usr/bin/kyth-tunable-rs
COPY --from=hub-web-builder --chmod=0755 /build/kyth-game-boost /usr/bin/kyth-game-boost
ARG SYSCONFIG_HASH=unset
RUN --mount=type=bind,source=build_files/scripts/sysconfig-static.sh,target=/ctx/sysconfig-static.sh \
    --mount=type=bind,source=build_files/scripts/sysconfig,target=/ctx/sysconfig \
    --mount=type=bind,source=build_files/scripts/lib,target=/ctx/lib \
    --mount=type=bind,source=build_files/data,target=/ctx/data \
    --mount=type=bind,source=build_files/config,target=/ctx/config \
    --mount=type=bind,source=build_files/kyth-tunable,target=/ctx/kyth-tunable \
    --mount=type=tmpfs,dst=/tmp \
    : "cache-bust:sysconfig=${SYSCONFIG_HASH}" && \
    bash /ctx/sysconfig-static.sh

# BUILD_DATE busts the upgrade layer and everything after it on every daily
# build. Package selection remains cached, but installed packages and the full
# Fedora kernel stack are refreshed against current repositories here.
ARG BUILD_DATE=unset

# Build cache boundary: upstream RPM upgrades and optional Mesa-git drivers.
# Mesa-git is folded into this layer instead of a standalone RUN so the no-op
# ENABLE_MESA_GIT=0 case does not add a separate empty layer to the manifest chain.
# Layers after this one are re-run on every daily build; layers before it are
# cached until their scripts or the base image change.
RUN --mount=type=bind,source=build_files/scripts/mesa-git.sh,target=/ctx/mesa-git.sh \
    --mount=type=bind,source=build_files/scripts/kernel-repair.sh,target=/ctx/kernel-repair.sh \
    --mount=type=bind,source=build_files/scripts/lib/fedora-kernel.sh,target=/ctx/lib/fedora-kernel.sh \
    --mount=type=bind,source=build_files/scripts/lib/find-kver.sh,target=/ctx/lib/find-kver.sh \
    --mount=type=bind,source=build_files/scripts/lib/dracut-retry.sh,target=/ctx/lib/dracut-retry.sh \
    --mount=type=bind,source=build_files/scripts/lib/check-multilib.sh,target=/ctx/lib/check-multilib.sh \
    --mount=type=cache,id=kyth-var-cache,target=/var/cache \
    --mount=type=cache,id=dnf-cache,sharing=locked,target=/var/cache/libdnf5 \
    --mount=type=cache,id=dnf-log,sharing=locked,target=/var/log \
    --mount=type=tmpfs,dst=/tmp \
    : "cache-bust=${BUILD_DATE}" && \
    set -euo pipefail; \
    dnf5 upgrade -y --refresh --setopt=retries=10 --setopt=timeout=120 --setopt=zchunk=False --setopt=max_parallel_downloads=10 --setopt=keepcache=1 \
        --exclude='gstreamer1-plugins-bad' \
        --exclude='gstreamer1-plugins-bad.i686' && \
    source /ctx/lib/fedora-kernel.sh && \
    if [[ "$(cat /usr/share/kyth/kernel-flavor 2>/dev/null || echo fedora)" == fedora ]]; then update_fedora_kernel; fi && \
    bash /ctx/kernel-repair.sh && \
    ENABLE_MESA_GIT=${ENABLE_MESA_GIT} bash /ctx/mesa-git.sh && \
    . /ctx/lib/check-multilib.sh && \
    check_multilib_pairs "${KYTH_MULTILIB_PAIRS[@]}" && \
    scan_multilib_orphans

# Build cache boundary: post-upgrade service wiring and account repair.
# Re-enforces display-manager symlinks that dnf5 upgrade can reset, and enables/
# disables runtime services after the upgrade has settled the unit file set.
RUN --mount=type=bind,source=build_files/scripts/sysconfig.sh,target=/ctx/sysconfig.sh \
    --mount=type=bind,source=build_files/scripts/sysconfig,target=/ctx/sysconfig \
    --mount=type=tmpfs,dst=/tmp \
    bash /ctx/sysconfig.sh

# Build cache boundary: Secure Boot signing, branding, helper app, and Plymouth.
# These operations share one raw BuildKit layer; legacy-rechunk repartitions the
# finished filesystem into update-efficient published OCI layers.
# Skipped gracefully when MOK_KEY is not set (local builds without a signing key).
# Pass the private key via: --secret id=mok_key,env=MOK_KEY

# The primary React+Tauri Hub's compiled binary — see the hub-web-builder
# stage declared near the top of this file (before BASE_IMAGE's own FROM,
# so it doesn't disturb that ARG's global scope). Ships on every channel;
# kyth-welcome-launch (installed below via 23-kyth-helper-ctx-installs.sh)
# is the single normal launch wrapper; it requires the Tauri shell and never
# falls back to the classic kyth-welcome UI.
COPY --from=hub-web-builder --chmod=0755 /build/kyth-hub-shell /usr/bin/kyth-hub-shell
COPY --from=hub-web-builder --chmod=0755 /build/kyth-probe /usr/bin/kyth-probe
COPY --from=hub-web-builder --chmod=0755 /build/kyth-guardian /usr/bin/kyth-guardian
COPY --from=hub-web-builder --chmod=0755 /build/kyth-update-watcher /usr/bin/kyth-update-watcher
COPY --from=hub-web-builder --chmod=0755 /build/kyth-network-share /usr/bin/kyth-network-share
COPY --from=hub-web-builder --chmod=0755 /build/kyth-telem /usr/bin/kyth-telem
COPY --from=hub-web-builder --chmod=0755 /build/kyth-privileged /usr/bin/kyth-privileged
COPY --from=hub-web-builder --chmod=0755 /build/kyth-post-update-check /usr/bin/kyth-post-update-check
COPY --from=hub-web-builder --chmod=0755 /build/kyth-firstboot-app-status /usr/bin/kyth-firstboot-app-status
COPY --from=hub-web-builder --chmod=0755 /build/kyth-steam-game-export /usr/bin/kyth-steam-game-export
COPY --from=hub-web-builder --chmod=0755 /build/kyth-hub-desktop-entries /usr/bin/kyth-hub-desktop-entries
COPY --from=hub-web-builder --chmod=0755 /build/kyth-safe-upgrade /usr/bin/kyth-safe-upgrade
COPY --from=hub-web-builder --chmod=0755 /build/kyth-bootc-guard /usr/bin/kyth-bootc-guard
COPY --from=hub-web-builder --chmod=0755 /build/kyth-btrfs-maint /usr/bin/kyth-btrfs-maint
COPY --from=hub-web-builder --chmod=0755 /build/kyth-ai-perfd /usr/bin/kyth-ai-perfd
COPY --from=hub-web-builder --chmod=0755 /build/kyth-perf-gate-rs /usr/bin/kyth-perf-gate-rs
COPY --from=hub-web-builder --chmod=0755 /build/kyth-doctor /usr/bin/kyth-doctor
COPY --from=hub-web-builder --chmod=0755 /build/kyth-health-check /usr/bin/kyth-health-check
COPY --from=hub-web-builder --chmod=0755 /build/kyth-smoke-check /usr/bin/kyth-smoke-check
COPY --from=hub-web-builder --chmod=0755 /build/kyth-resume-check /usr/bin/kyth-resume-check
COPY --from=hub-web-builder --chmod=0755 /build/kyth-nvidia-status /usr/bin/kyth-nvidia-status
COPY --from=hub-web-builder --chmod=0755 /build/kyth-controller-check /usr/bin/kyth-controller-check
COPY --from=hub-web-builder --chmod=0755 /build/kyth-creator-check /usr/bin/kyth-creator-check
COPY --from=hub-web-builder --chmod=0755 /build/kyth-exe-compat /usr/bin/kyth-exe-compat
COPY --from=hub-web-builder --chmod=0755 /build/kyth-snapshot-timeline /usr/bin/kyth-snapshot-timeline
COPY --from=hub-web-builder --chmod=0755 /build/kyth-print-check /usr/bin/kyth-print-check
COPY --from=hub-web-builder --chmod=0755 /build/kyth-windows-verify /usr/bin/kyth-windows-verify
COPY --from=hub-web-builder --chmod=0755 /build/kyth-vm-acceptance-guest /usr/bin/kyth-vm-acceptance-guest
COPY --from=hub-web-builder --chmod=0755 /build/kyth-tunable /usr/bin/kyth-tunable
COPY --from=hub-web-builder --chmod=0755 /build/kyth-configure-session /usr/bin/kyth-configure-session
COPY --from=hub-web-builder --chmod=0755 /build/kyth-set-resolution /usr/bin/kyth-set-resolution
COPY --from=hub-web-builder --chmod=0755 /build/kyth-set-kickoff-icon /usr/bin/kyth-set-kickoff-icon
COPY --from=hub-web-builder --chmod=0755 /build/kyth-greeter-compositor /usr/bin/kyth-greeter-compositor
COPY --from=hub-web-builder --chmod=0755 /build/kyth-config-apply /usr/bin/kyth-config-apply
COPY --from=hub-web-builder --chmod=0755 /build/kyth-apply-scx-preset /usr/bin/kyth-apply-scx-preset
COPY --from=hub-web-builder --chmod=0755 /build/kyth-apply-explorer /usr/bin/kyth-apply-explorer
COPY --from=hub-web-builder --chmod=0755 /build/kyth-apply-desktop-layout /usr/bin/kyth-apply-desktop-layout

ARG SECUREBOOT_SIGNING_REQUESTED=0
# Branding fragments retain the legacy fixtures for rollback; re-run the
# native dispatcher last so every tunable alias points at Rust in the
# installed image and cannot be overwritten by a later fragment.
RUN --mount=type=bind,source=build_files,target=/ctx \
    --mount=type=bind,source=src/kyth-welcome,target=/ctx/kyth-welcome \
    --mount=type=bind,source=src/kyth_shared,target=/ctx/kyth_shared \
    --mount=type=bind,source=src,target=/src \
    --mount=type=bind,source=src/kyth-welcome,target=/src/kyth-welcome \
    --mount=type=bind,source=src/kyth_shared,target=/src/kyth_shared \
    --mount=type=tmpfs,dst=/tmp \
    --mount=type=secret,id=mok_key \
    if [ -d /usr/share/factory/var/cache/libdnf5 ]; then \
        find /usr/share/factory/var/cache/libdnf5 -mindepth 1 -delete; \
    fi && \
    SECUREBOOT_SIGNING_REQUESTED=${SECUREBOOT_SIGNING_REQUESTED} bash /ctx/scripts/secureboot.sh && \
    bash /ctx/scripts/branding.sh && \
    bash /ctx/scripts/sysconfig/tunable/01-tunable-dispatcher.sh && \
    bash /ctx/scripts/plymouth-initramfs.sh
