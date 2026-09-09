import "_vars.just"
import "build.just"
import "vm.just"
import "_internal.just"

alias build-vm := build-qcow2
alias run-vm := run-vm-qcow2

[private]
default:
    @just --list

# Check Just Syntax
[group('Just')]
check:
    #!/usr/bin/env bash
    set -euo pipefail
    find . -path './tmp' -prune -o -type f -name "*.just" -print | while read -r file; do
    	echo "Checking syntax: $file"
    	just --unstable --fmt --check -f "$file"
    done
    echo "Checking syntax: Justfile"
    just --unstable --fmt --check -f Justfile

# Check Dockerfile frontend/build rules without requiring the local kyth-base image.
[group('Build')]
check-dockerfile check_base_image=default_base_image:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! id -nG | grep -qw docker; then
        exec sg docker -c "just check-dockerfile '{{ check_base_image }}'"
    fi
    docker buildx build --check \
        --build-arg BASE_IMAGE={{ check_base_image }} \
        .

# Run Python unit tests (deprioritized + memory-capped on a live desktop).
[group('Quality')]
test *args:
    ./build_files/scripts/run-tests.sh {{ args }}

# Verify codecs/drivers are baked (Nobara-style one-click, no post-install dnf)
[group('Quality')]
verify-codecs image="localhost/kyth:latest":
    #!/usr/bin/env bash
    set -euo pipefail
    for pkg in gstreamer1-plugins-bad-freeworld gstreamer1-plugins-ugly gstreamer1-libav gstreamer1-vaapi; do
        podman run --rm {{ image }} rpm -q "$pkg" >/dev/null && echo "OK $pkg" || (echo "MISSING $pkg" >&2; exit 1)
    done
    echo "Codecs baked — no post-install dnf needed"

# Run Python unit tests with a statement coverage report.
[group('Quality')]
test-coverage:
    ./build_files/scripts/run-quality.sh
    echo ""
    echo "HTML report: coverage-html/index.html"

# Check maintainability/optimization budgets tracked in source control.
[group('Quality')]
check-optimization:
    python3 build_files/scripts/optimization-report.py --check

# Build/typecheck the React + Tauri (Rust) Kyth Hub shell (src/kyth-hub-web).
# Needs Node + a Rust toolchain + the Tauri Linux prerequisites (webkit2gtk,
# gtk3, dbus, libsoup3 -devel) — all provisioned in the kyth-ai-dev box by
# `ujust ai-dev-setup` (see kyth_shared/ai_dev.py's PROVISION_SCRIPT).
[group('Quality')]
check-hub-shell:
    ./build_files/scripts/check-hub-web-shell.sh

# Build/typecheck the React + Tauri (Rust) KythOS installer shell
# (src/kyth-installer-web). Same prerequisites as check-hub-shell. Not yet
# wired into the Dockerfile — this is the only build gate the crate has.
[group('Quality')]
check-installer-shell:
    ./build_files/scripts/check-installer-web-shell.sh

# Print source metrics; pass runtime=1 on a representative installed system.
[group('Quality')]
optimization-report runtime="0":
    #!/usr/bin/env bash
    set -euo pipefail
    args=()
    if [[ "{{ runtime }}" == "1" ]]; then args+=(--runtime); fi
    python3 build_files/scripts/optimization-report.py "${args[@]}"

# Create/update the local pinned quality-tool environment.
[group('Quality')]
setup-quality:
    python3 -m venv .venv-quality
    .venv-quality/bin/python -m pip install --disable-pip-version-check -r requirements-quality.txt
    .venv-quality/bin/coverage --version
    .venv-quality/bin/ruff --version

# Run the complete validation suite used by GitHub Actions and pre-push.
# Wrapped via the scripts' own systemd-run --scope deprioritization so direct
# `just validate` on a live desktop doesn't starve kwin/Plasma.
[group('Quality')]
validate:
    ./build_files/scripts/validate.sh

# Run Validation plus changed-file Codacy and pinned CodeQL security checks.
# Same deprioritization as validate — this is the heaviest local gate.
[group('Quality')]
ci-preflight:
    ./build_files/scripts/ci-preflight.sh

# Fix Just Syntax
[group('Just')]
fix:
    #!/usr/bin/env bash
    set -euo pipefail
    find . -path './tmp' -prune -o -type f -name "*.just" -print | while read -r file; do
    	echo "Checking syntax: $file"
    	just --unstable --fmt -f "$file"
    done
    echo "Checking syntax: Justfile"
    just --unstable --fmt -f Justfile || { exit 1; }

# Clean local build temp dirs and fix output/ ownership.
[group('Utility')]
clean:
    #!/usr/bin/env bash
    set -eoux pipefail
    rm -rf _build* *_build*
    rm -f previous.manifest.json
    if [[ -d output ]]; then
        sudo chown -R "$(id -u):$(id -g)" output/
    fi

# Sudo-clean: run 'clean' with sudo if needed
[group('Utility')]
[private]
sudo-clean:
    just sudoif just clean

# Show a disk-usage summary: Docker images, build cache, and output/ ISOs
[group('Utility')]
disk-usage:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "── Docker ────────────────────────────────────────────────────────────────"
    docker system df
    echo ""
    echo "── Output ISOs ───────────────────────────────────────────────────────────"
    just _list-output-images
    echo ""
    echo "── /var/tmp kyth-live build dirs ─────────────────────────────────────────"
    find /var/tmp -maxdepth 1 \( -name "kyth-live.*" -o -name "kyth-titanoboa.*" \) -exec du -sh {} \; 2>/dev/null || echo "(none)"

# Set up a Kali Linux security toolbox via the shipped KythOS ujust recipe.
[group('Utility')]
setup-kali-box tools="headless":
    #!/usr/bin/env bash
    set -euo pipefail
    exec just --justfile build_files/just/kyth.just setup-kali-box "{{ tools }}"

# Export Kali Linux GUI apps via the shipped KythOS ujust recipe.
[group('Utility')]
export-kali-apps:
    #!/usr/bin/env bash
    set -euo pipefail
    exec just --justfile build_files/just/kyth.just export-kali-apps

# Install tracked git hooks for validation and commit message helpers.
[group('Utility')]
install-git-hooks:
    #!/usr/bin/env bash
    set -euo pipefail
    git config core.hooksPath .githooks
    chmod +x .githooks/pre-commit .githooks/pre-push .githooks/prepare-commit-msg build_files/scripts/install-validation-tools.sh build_files/scripts/validate.sh build_files/scripts/run-quality.sh build_files/scripts/ci-preflight.sh
    echo "Git hooks installed via core.hooksPath=.githooks"

# Remove old output ISOs — keeps only the current live ISO and current BIB ISO.
[group('Utility')]
clean-output:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Cleaning stale output artefacts..."
    just _clean-output-artefacts
    echo "Remaining output files:"
    just _list-output-images

# Prune Docker build cache and dangling (unreferenced) image layers.
[group('Utility')]
clean-docker:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Pruning Docker build cache and dangling image layers..."
    just _prune-docker-cache
    echo ""
    docker system df

# Reclaim space specifically for live ISO dev loops.
[group('Utility')]
prune-live-dev:
    #!/usr/bin/env bash
    set -euo pipefail

    echo "── Removing stale kyth-live images ──────────────────────────────────────"
    docker images \
        | awk 'NR>1 {print $1":"$2}' \
        | grep '^kyth-live:' \
        | xargs -r docker rmi -f || true

    echo ""
    echo "── Pruning Docker cache/volumes ──────────────────────────────────────────"
    docker builder prune -af || true
    docker image prune -af || true
    docker volume prune -f || true

    echo ""
    echo "── Removing stale VM/build temp artefacts ───────────────────────────────"
    find /tmp -maxdepth 1 -type f -name 'kyth-live-test.qcow2' -delete || true
    find /var/tmp -maxdepth 2 -type f -name 'kyth-live-test.qcow2' -delete || true
    find /tmp -maxdepth 1 -type d -name 'kyth-vm-share-*' -exec rm -rf {} + || true
    just _clean-vartmp-builddirs

    echo ""
    echo "── Post-cleanup summary ───────────────────────────────────────────────────"
    df -h /tmp /var || true
    docker system df || true

# Remove only disposable ISO/VM acceptance state. Safe before a fresh run and
# after an interrupted run; refuses to act while QEMU/build work is active.
[group('Utility')]
clean-vm-acceptance:
    build_files/scripts/cleanup-vm-acceptance.sh

# Run the exact-image Rust migration install/update/rollback evidence flow.
# The image ref must be pinned to the promoted testing image under review.
rust-migration-acceptance iso image_ref artifacts="/tmp/kyth-rust-migration-acceptance":
    build_files/scripts/run-rust-migration-acceptance.sh --iso "{{ iso }}" --image-ref "{{ image_ref }}" --artifacts "{{ artifacts }}"

# Full local cleanup: build temps + stale outputs + Docker cache.
[group('Utility')]
clean-all: clean clean-output clean-docker

# Nuclear purge: reclaim maximum disk space.
[group('Utility')]
purge:
    #!/usr/bin/env bash
    set -euo pipefail

    echo "── Stale _build* temp dirs in project root ───────────────────────────────"
    shopt -s nullglob
    build_dirs=( _build* )
    if [[ ${#build_dirs[@]} -gt 0 ]]; then
        sudo rm -rf "${build_dirs[@]}"
        printf '  removed: %s\n' "${build_dirs[@]}"
    else
        echo "  (none)"
    fi

    echo ""
    echo "── /var/tmp kyth-live.* / kyth-titanoboa.* build dirs ───────────────────"
    just _clean-vartmp-builddirs
    echo "  Done"

    echo ""
    echo "── Old output artefacts (previous-built-iso, archive, manifest backups) ──"
    just _clean-output-artefacts
    echo "  Done"

    echo ""
    echo "── Docker build cache and dangling image layers ──────────────────────────"
    just _prune-docker-cache

    echo ""
    echo "── Podman dangling image layers ──────────────────────────────────────────"
    if command -v podman &>/dev/null; then
        podman image prune -f
    else
        echo "  (podman not found)"
    fi

    echo ""
    echo "── Result ────────────────────────────────────────────────────────────────"
    df -h "$(pwd)"

# Runs shell check on all Bash scripts
[group('Quality')]
lint:
    #!/usr/bin/env bash
    set -eoux pipefail
    if ! command -v shellcheck &> /dev/null; then
        echo "shellcheck could not be found. Please install it."
        exit 1
    fi
    /usr/bin/find . -iname "*.sh" -type f -exec shellcheck "{}" ';'

# Runs shfmt on all Bash scripts
[group('Quality')]
format:
    #!/usr/bin/env bash
    set -eoux pipefail
    if ! command -v shfmt &> /dev/null; then
        echo "shfmt could not be found. Please install it."
        exit 1
    fi
    /usr/bin/find . -iname "*.sh" -type f -exec shfmt --write "{}" ';'

# Format every tracked Rust project using its Cargo manifest.
[group('Quality')]
format-rust:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v cargo >/dev/null 2>&1 || { echo "cargo could not be found. Please install Rust." >&2; exit 1; }
    while IFS= read -r -d '' manifest; do
        cargo fmt --manifest-path "${manifest}" --all
    done < <(git ls-files -z '*Cargo.toml')

# Set up the React/Tauri Hub's frontend dependencies for local development.
[group('Utility')]
setup-hub:
    #!/usr/bin/env bash
    set -euo pipefail
    npm --prefix src/kyth-hub-web ci
    echo "React/Tauri Hub ready: run 'just run-hub'"

# Run the React/Tauri Hub locally from the checkout.
[group('Utility')]
run-hub *args:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ ! -d src/kyth-hub-web/node_modules ]]; then
        echo "Hub dependencies are missing. Run: just setup-hub" >&2
        exit 1
    fi
    exec npm --prefix src/kyth-hub-web run tauri:dev -- {{ args }}

# Health like cachy-doctor (probe + zram/btrfs/scx); no daemon.
[group('Utility')]
doctor:
    PYTHONPATH=build_files/kyth_shared python3 -m kyth_shared.doctor

# COPR/AUR-style opt-in (Endeavour-like vanilla base).
[group('Utility')]
enable-copr repo:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "This enables a COPR repo on an installed system (opt-in):"
    echo "  sudo dnf5 copr enable {{ repo }}"
    echo "Run above on the host; base stays vanilla."

# Mesa + Plasma cutting edge overlay gated (kinoite stable default) (N41)
[group('Utility')]
enable-mesa-git:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Cutting Mesa git overlay (stable default, kinoite rollback):"
    echo "  sudo dnf5 copr enable xxx/mesa-git -y  # dry-run: bootc container lint, then overlay"
    echo "  bootc rollback  # if latest Mesa bricks, stable Mesa stays"

[group('Utility')]
enable-plasma-next:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Cutting Plasma next overlay (stable default):"
    echo "  sudo dnf5 copr enable xxx/plasma-unstable -y  # dry-run + rollback"

# Cutting kernel/sched per-game (kinoite stable + Cachy/bore/scx cutting edge) (N42)
[group('Utility')]
enable-sched-next:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Cutting sched per-game (scx lavd/rusty, bore, kinoite stable default):"
    echo "  just enable-cachy-kernel  # kernel"
    echo "  sudo dnf5 copr enable xxx/scx-next -y  # sched-ext next, then per-game gaming_slice"

# Provenance + umu nightly opt-in (Bazzite stale) (N44)
[group('Utility')]
enable-proton-next:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Cutting Proton next (umu main / Proton-CachyOS slr nightly, stable baked):"
    echo "  mkdir -p ~/.local/share/Steam/compatibilitytools.d"
    echo "  curl -L https://github.com/Open-Wine-Components/umu-proton/releases/latest/download/umu-proton.tar.gz | tar -xz -C ~/.local/share/Steam/compatibilitytools.d"

# PSI-gated btrfs+zram+irq cutting edge (Cachy no gate) (N46)
[group('Utility')]
enable-psi-tuning:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "PSI-gated cutting tuning (btrfs+zram+irq, skip when PSI>80):"
    echo "  PSI>80 → skip btrfs balance/zram tuning, kinoite stable under pressure"

# Per-game MangoHud/Gamescope git cutting edge (Nobara global env) (N45)
[group('Utility')]
enable-mangohud-next:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Cutting MangoHud git per-game (MANGOHUD_CONFIG per-game, not global env):"
    echo "  sudo dnf5 copr enable xxx/mangohud-git -y  # then per-game MANGOHUD=1 %command% via N22 slice"

# Cachy-style v3/PGO opt-in (no default change, keep fedora generic).
[group('Utility')]
enable-cachy-kernel:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Opt-in CachyOS kernel / x86-64-v3 (Cachy wins perf, default stays fedora):"
    echo "  ENABLE_CACHY=1 just build-base    # builds base with CACHYOS_KERNEL_VER via resolve-versions.py"
    echo "  kyth-doctor  # shows kernel: cachy vs fedora"

# Brew/distrobox-style opt-in (Aurora-like, no base bloat).
[group('Utility')]
enable-brew:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Opt-in Homebrew (Aurora-like, not in base):"
    echo "  /bin/bash -c \"\$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\""
    echo "Then: eval \"\$(/home/linuxbrew/.linuxbrew/bin/brew shellenv)\""

# Power profile slider (Windows-like, PPD or TLP fallback) (N27)
[group('Utility')]
power-profile mode="balanced":
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Power profile opt-in (PPD/TLP, no base daemon):"
    if command -v powerprofilesctl >/dev/null 2>&1; then echo "  powerprofilesctl set {{ mode }}  # balanced/performance/power-saver"; else echo "  sudo dnf install -y tuned && sudo tuned-adm profile {{ mode }}  # fallback"; fi

# VPN/Tailscale one-click (Aurora-like, wait-online already enabled) (N28)
[group('Utility')]
vpn-up provider="tailscale":
    #!/usr/bin/env bash
    set -euo pipefail
    echo "VPN opt-in (no base service, uses NetworkManager-wait-online):"
    if [[ "{{ provider }}" == "tailscale" ]]; then echo "  sudo tailscale up"; else echo "  nmcli connection up {{ provider }}  # or: sudo wg-quick up wg0"; fi

# Per-game audio preset (Nobara-like, no global env) (N29)
[group('Utility')]
audio-preset profile="gaming":
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Audio preset {{ profile }} (pipewire/easyeffects, tmp→apply, no env.d):"
    echo "  # gaming: easyeffects --load-preset Gaming; work: easyeffects --load-preset Work"

# Latest Arch distrobox cutting edge (Endeavour AUR freshness, base lean) (N47)
# Flathub beta cutting edge (Aurora stable + beta opt-in) (N48)
[group('Utility')]
enable-flathub-beta:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Cutting Flathub beta (stable default, opt-in beta):"
    echo "  flatpak remote-add --if-not-exists flathub-beta https://flathub.org/beta-repo/flathub-beta.flatpakrepo"

# Reproducible perf audit + compare (Cachy no artifact) (N50)
[group('Utility')]
perf-compare:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Reproducible perf compare (hyperfine + systemd-analyze + probe json):"
    echo "  hyperfine 'just check-hub-shell' --warmup 1"
    echo "  systemd-analyze; cat /run/user/1000/kyth/probe-cache.json | jq ."

[group('Utility')]
create-arch-latest:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Cutting Arch latest via distrobox (base lean, Endeavour freshness):"
    echo "  distrobox create --image archlinux:latest --name arch-latest && distrobox enter arch-latest  # yay -Syu"

[group('Utility')]
create-devbox flavor="fedora":
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Opt-in distrobox (Aurora/Endeavour-like, no base bloat):"
    echo "  distrobox create --image {{ flavor }} --name dev-{{ flavor }} && distrobox enter dev-{{ flavor }}"
    echo "Run above on host; toolbox stays vanilla."

# Preview the installer UI in your browser (no disk changes — safe for dev)
[group('Utility')]
preview-installer:
    #!/usr/bin/env python3
    import sys, threading, time
    sys.path.insert(0, "build_files/kyth-installer")
    from kyth_installer.server import Handler, _Server
    server = _Server(("127.0.0.1", 7777), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    print("Installer UI → http://127.0.0.1:7777  (Ctrl-C to stop)")
    try:
        while True:
            time.sleep(60)
    except KeyboardInterrupt:
        pass
