# Rust-owned ujust compatibility manifest.
#
# The installed image imports this file instead of executable shell recipe
# bodies. Each recipe name is retained for compatibility, while policy,
# argument validation, execution, and status are owned by kyth-runtime.

set shell := ["/bin/bash", "-euo", "pipefail", "-c"]

[group('KythOS')]
_install-flatpak *args:
    exec /usr/bin/kyth-runtime recipe "_install-flatpak" {{ args }}

[group('KythOS')]
setup-kali-box *args:
    exec /usr/bin/kyth-runtime recipe "setup-kali-box" {{ args }}

[group('KythOS')]
export-kali-apps *args:
    exec /usr/bin/kyth-runtime recipe "export-kali-apps" {{ args }}

[group('KythOS')]
setup-waydroid *args:
    exec /usr/bin/kyth-runtime recipe "setup-waydroid" {{ args }}

[group('KythOS')]
remove-waydroid *args:
    exec /usr/bin/kyth-runtime recipe "remove-waydroid" {{ args }}

[group('KythOS')]
install-boxbuddy *args:
    exec /usr/bin/kyth-runtime recipe "install-boxbuddy" {{ args }}

[group('KythOS')]
_ai-dev *args:
    exec /usr/bin/kyth-runtime recipe "_ai-dev" {{ args }}

[group('KythOS')]
ai-dev-status *args:
    exec /usr/bin/kyth-runtime recipe "ai-dev-status" {{ args }}

[group('KythOS')]
ai-dev-setup *args:
    exec /usr/bin/kyth-runtime recipe "ai-dev-setup" {{ args }}

[group('KythOS')]
ai-dev-enter *args:
    exec /usr/bin/kyth-runtime recipe "ai-dev-enter" {{ args }}

[group('KythOS')]
ai-dev-start *args:
    exec /usr/bin/kyth-runtime recipe "ai-dev-start" {{ args }}

[group('KythOS')]
ai-dev-stop *args:
    exec /usr/bin/kyth-runtime recipe "ai-dev-stop" {{ args }}

[group('KythOS')]
ai-dev-remove *args:
    exec /usr/bin/kyth-runtime recipe "ai-dev-remove" {{ args }}

[group('KythOS')]
podman-btrfs *args:
    exec /usr/bin/kyth-runtime recipe "podman-btrfs" {{ args }}

[group('KythOS')]
distrobox-cache *args:
    exec /usr/bin/kyth-runtime recipe "distrobox-cache" {{ args }}

[group('KythOS')]
startup-apps *args:
    exec /usr/bin/kyth-runtime recipe "startup-apps" {{ args }}

[group('KythOS')]
install-ms-fonts *args:
    exec /usr/bin/kyth-runtime recipe "install-ms-fonts" {{ args }}

[group('KythOS')]
setup-printer *args:
    exec /usr/bin/kyth-runtime recipe "setup-printer" {{ args }}

[group('KythOS')]
firmware-update *args:
    exec /usr/bin/kyth-runtime recipe "firmware-update" {{ args }}

[group('KythOS')]
windows-verify *args:
    exec /usr/bin/kyth-runtime recipe "windows-verify" {{ args }}

[group('KythOS')]
setup-kyth-dev-box *args:
    exec /usr/bin/kyth-runtime recipe "setup-kyth-dev-box" {{ args }}

[group('KythOS')]
install-vscode *args:
    exec /usr/bin/kyth-runtime recipe "install-vscode" {{ args }}

[group('KythOS')]
sccache *args:
    exec /usr/bin/kyth-runtime recipe "sccache" {{ args }}

[group('KythOS')]
flatpak-prefetch *args:
    exec /usr/bin/kyth-runtime recipe "flatpak-prefetch" {{ args }}

[group('KythOS')]
work-cache *args:
    exec /usr/bin/kyth-runtime recipe "work-cache" {{ args }}

[group('KythOS')]
install-jetbrains-toolbox *args:
    exec /usr/bin/kyth-runtime recipe "install-jetbrains-toolbox" {{ args }}

[group('KythOS')]
kerver *args:
    exec /usr/bin/kyth-runtime recipe "kerver" {{ args }}

[group('KythOS')]
device-info *args:
    exec /usr/bin/kyth-runtime recipe "device-info" {{ args }}

[group('KythOS')]
smoke-check *args:
    exec /usr/bin/kyth-runtime recipe "smoke-check" {{ args }}

[group('KythOS')]
post-update-check *args:
    exec /usr/bin/kyth-runtime recipe "post-update-check" {{ args }}

[group('KythOS')]
resume-check *args:
    exec /usr/bin/kyth-runtime recipe "resume-check" {{ args }}

[group('KythOS')]
snappy-bench *args:
    exec /usr/bin/kyth-runtime recipe "snappy-bench" {{ args }}

[group('KythOS')]
perf-gate *args:
    exec /usr/bin/kyth-runtime recipe "perf-gate" {{ args }}

[group('KythOS')]
telemetry-opt *args:
    exec /usr/bin/kyth-runtime recipe "telemetry-opt" {{ args }}

[group('KythOS')]
setup-boot-windows-steam *args:
    exec /usr/bin/kyth-runtime recipe "setup-boot-windows-steam" {{ args }}

[group('KythOS')]
dualboot-status *args:
    exec /usr/bin/kyth-runtime recipe "dualboot-status" {{ args }}

[group('KythOS')]
reclaim-windows *args:
    exec /usr/bin/kyth-runtime recipe "reclaim-windows" {{ args }}

[group('KythOS')]
fix-dualboot-clock *args:
    exec /usr/bin/kyth-runtime recipe "fix-dualboot-clock" {{ args }}

[group('KythOS')]
install-battlenet *args:
    exec /usr/bin/kyth-runtime recipe "install-battlenet" {{ args }}

[group('KythOS')]
install-epic-launcher *args:
    exec /usr/bin/kyth-runtime recipe "install-epic-launcher" {{ args }}

[group('KythOS')]
install-ea-app *args:
    exec /usr/bin/kyth-runtime recipe "install-ea-app" {{ args }}

[group('KythOS')]
install-ubisoft-connect *args:
    exec /usr/bin/kyth-runtime recipe "install-ubisoft-connect" {{ args }}

[group('KythOS')]
install-steam *args:
    exec /usr/bin/kyth-runtime recipe "install-steam" {{ args }}

[group('KythOS')]
install-lutris *args:
    exec /usr/bin/kyth-runtime recipe "install-lutris" {{ args }}

[group('KythOS')]
install-heroic *args:
    exec /usr/bin/kyth-runtime recipe "install-heroic" {{ args }}

[group('KythOS')]
install-bottles *args:
    exec /usr/bin/kyth-runtime recipe "install-bottles" {{ args }}

[group('KythOS')]
install-prismlauncher *args:
    exec /usr/bin/kyth-runtime recipe "install-prismlauncher" {{ args }}

[group('KythOS')]
install-itch *args:
    exec /usr/bin/kyth-runtime recipe "install-itch" {{ args }}

[group('KythOS')]
install-retroarch *args:
    exec /usr/bin/kyth-runtime recipe "install-retroarch" {{ args }}

[group('KythOS')]
install-ludusavi *args:
    exec /usr/bin/kyth-runtime recipe "install-ludusavi" {{ args }}

[group('KythOS')]
hardware-policy *args:
    exec /usr/bin/kyth-runtime recipe "hardware-policy" {{ args }}

[group('KythOS')]
hardware-inventory *args:
    exec /usr/bin/kyth-runtime recipe "hardware-inventory" {{ args }}

[group('KythOS')]
hardware-policy-apply *args:
    exec /usr/bin/kyth-runtime recipe "hardware-policy-apply" {{ args }}

[group('KythOS')]
export-steam-games *args:
    exec /usr/bin/kyth-runtime recipe "export-steam-games" {{ args }}

[group('KythOS')]
controller-check *args:
    exec /usr/bin/kyth-runtime recipe "controller-check" {{ args }}

[group('KythOS')]
install-lact *args:
    exec /usr/bin/kyth-runtime recipe "install-lact" {{ args }}

[group('KythOS')]
corectrl *args:
    exec /usr/bin/kyth-runtime recipe "corectrl" {{ args }}

[group('KythOS')]
install-coolercontrol *args:
    exec /usr/bin/kyth-runtime recipe "install-coolercontrol" {{ args }}

[group('KythOS')]
install-piper *args:
    exec /usr/bin/kyth-runtime recipe "install-piper" {{ args }}

[group('KythOS')]
install-openrgb *args:
    exec /usr/bin/kyth-runtime recipe "install-openrgb" {{ args }}

[group('KythOS')]
install-solaar *args:
    exec /usr/bin/kyth-runtime recipe "install-solaar" {{ args }}

[group('KythOS')]
install-racing-wheel-drivers *args:
    exec /usr/bin/kyth-runtime recipe "install-racing-wheel-drivers" {{ args }}

[group('KythOS')]
install-oversteer *args:
    exec /usr/bin/kyth-runtime recipe "install-oversteer" {{ args }}

[group('KythOS')]
install-asus-tools *args:
    exec /usr/bin/kyth-runtime recipe "install-asus-tools" {{ args }}

[group('KythOS')]
install-vesktop *args:
    exec /usr/bin/kyth-runtime recipe "install-vesktop" {{ args }}

[group('KythOS')]
fix-ntfs-drives *args:
    exec /usr/bin/kyth-runtime recipe "fix-ntfs-drives" {{ args }}

[group('KythOS')]
install-gpu-screen-recorder *args:
    exec /usr/bin/kyth-runtime recipe "install-gpu-screen-recorder" {{ args }}

[group('KythOS')]
install-goverlay *args:
    exec /usr/bin/kyth-runtime recipe "install-goverlay" {{ args }}

[group('KythOS')]
install-mangojuice *args:
    exec /usr/bin/kyth-runtime recipe "install-mangojuice" {{ args }}

[group('KythOS')]
install-obs *args:
    exec /usr/bin/kyth-runtime recipe "install-obs" {{ args }}

[group('KythOS')]
enable-obs-capture *args:
    exec /usr/bin/kyth-runtime recipe "enable-obs-capture" {{ args }}

[group('KythOS')]
install-lsfg-vk *args:
    exec /usr/bin/kyth-runtime recipe "install-lsfg-vk" {{ args }}

[group('KythOS')]
deploy-opticscaler *args:
    exec /usr/bin/kyth-runtime recipe "deploy-opticscaler" {{ args }}

[group('KythOS')]
game-boost *args:
    exec /usr/bin/kyth-runtime recipe "game-boost" {{ args }}

[group('KythOS')]
preheat-shaders *args:
    exec /usr/bin/kyth-runtime recipe "preheat-shaders" {{ args }}

[group('KythOS')]
health-check *args:
    exec /usr/bin/kyth-runtime recipe "health-check" {{ args }}

[group('KythOS')]
update-proton-cachyos *args:
    exec /usr/bin/kyth-runtime recipe "update-proton-cachyos" {{ args }}

[group('KythOS')]
install-umu *args:
    exec /usr/bin/kyth-runtime recipe "install-umu" {{ args }}

[group('KythOS')]
toggle-fsr4 *args:
    exec /usr/bin/kyth-runtime recipe "toggle-fsr4" {{ args }}

[group('KythOS')]
toggle-nvapi *args:
    exec /usr/bin/kyth-runtime recipe "toggle-nvapi" {{ args }}

[group('KythOS')]
gaming-stack-status *args:
    exec /usr/bin/kyth-runtime recipe "gaming-stack-status" {{ args }}

[group('KythOS')]
nvidia-status *args:
    exec /usr/bin/kyth-runtime recipe "nvidia-status" {{ args }}

[group('KythOS')]
install-nvidia-driver *args:
    exec /usr/bin/kyth-runtime recipe "install-nvidia-driver" {{ args }}

[group('KythOS')]
install-displaylink *args:
    exec /usr/bin/kyth-runtime recipe "install-displaylink" {{ args }}

[group('KythOS')]
game-performance *args:
    exec /usr/bin/kyth-runtime recipe "game-performance" {{ args }}

[group('KythOS')]
performance-mode *args:
    exec /usr/bin/kyth-runtime recipe "performance-mode" {{ args }}

[group('KythOS')]
gaming-mode *args:
    exec /usr/bin/kyth-runtime recipe "gaming-mode" {{ args }}

[group('KythOS')]
balanced-mode *args:
    exec /usr/bin/kyth-runtime recipe "balanced-mode" {{ args }}

[group('KythOS')]
game-performance-profile *args:
    exec /usr/bin/kyth-runtime recipe "game-performance-profile" {{ args }}

[group('KythOS')]
gamescope *args:
    exec /usr/bin/kyth-runtime recipe "gamescope" {{ args }}

[group('KythOS')]
game-hdr *args:
    exec /usr/bin/kyth-runtime recipe "game-hdr" {{ args }}

[group('KythOS')]
game-scope *args:
    exec /usr/bin/kyth-runtime recipe "game-scope" {{ args }}

[group('KythOS')]
scx *args:
    exec /usr/bin/kyth-runtime recipe "scx" {{ args }}

[group('KythOS')]
nvme-tuning *args:
    exec /usr/bin/kyth-runtime recipe "nvme-tuning" {{ args }}

[group('KythOS')]
io-tune *args:
    exec /usr/bin/kyth-runtime recipe "io-tune" {{ args }}

[group('KythOS')]
kargs-apply *args:
    exec /usr/bin/kyth-runtime recipe "kargs-apply" {{ args }}

[group('KythOS')]
net-tune *args:
    exec /usr/bin/kyth-runtime recipe "net-tune" {{ args }}

[group('KythOS')]
uksmd *args:
    exec /usr/bin/kyth-runtime recipe "uksmd" {{ args }}

[group('KythOS')]
journal-tune *args:
    exec /usr/bin/kyth-runtime recipe "journal-tune" {{ args }}

[group('KythOS')]
thp-tune *args:
    exec /usr/bin/kyth-runtime recipe "thp-tune" {{ args }}

[group('KythOS')]
mimalloc *args:
    exec /usr/bin/kyth-runtime recipe "mimalloc" {{ args }}

[group('KythOS')]
mimalloc-run *args:
    exec /usr/bin/kyth-runtime recipe "mimalloc-run" {{ args }}

[group('KythOS')]
irq-tune *args:
    exec /usr/bin/kyth-runtime recipe "irq-tune" {{ args }}

[group('KythOS')]
btrfs-tune *args:
    exec /usr/bin/kyth-runtime recipe "btrfs-tune" {{ args }}

[group('KythOS')]
trim-tune *args:
    exec /usr/bin/kyth-runtime recipe "trim-tune" {{ args }}

[group('KythOS')]
ananicy *args:
    exec /usr/bin/kyth-runtime recipe "ananicy" {{ args }}

[group('KythOS')]
zswap *args:
    exec /usr/bin/kyth-runtime recipe "zswap" {{ args }}

[group('KythOS')]
gpu-power *args:
    exec /usr/bin/kyth-runtime recipe "gpu-power" {{ args }}

[group('KythOS')]
sched-latency *args:
    exec /usr/bin/kyth-runtime recipe "sched-latency" {{ args }}

[group('KythOS')]
readahead *args:
    exec /usr/bin/kyth-runtime recipe "readahead" {{ args }}

[group('KythOS')]
readahead-run *args:
    exec /usr/bin/kyth-runtime recipe "readahead-run" {{ args }}

[group('KythOS')]
gaming-master *args:
    exec /usr/bin/kyth-runtime recipe "gaming-master" {{ args }}

[group('KythOS')]
wine-sync *args:
    exec /usr/bin/kyth-runtime recipe "wine-sync" {{ args }}

[group('KythOS')]
kwin-latency *args:
    exec /usr/bin/kyth-runtime recipe "kwin-latency" {{ args }}

[group('KythOS')]
pipewire-gaming *args:
    exec /usr/bin/kyth-runtime recipe "pipewire-gaming" {{ args }}

[group('KythOS')]
btrfs-autotune *args:
    exec /usr/bin/kyth-runtime recipe "btrfs-autotune" {{ args }}

[group('KythOS')]
boot-timeout *args:
    exec /usr/bin/kyth-runtime recipe "boot-timeout" {{ args }}

[group('KythOS')]
oom-gaming *args:
    exec /usr/bin/kyth-runtime recipe "oom-gaming" {{ args }}

[group('KythOS')]
shader-tmpfs *args:
    exec /usr/bin/kyth-runtime recipe "shader-tmpfs" {{ args }}

[group('KythOS')]
gaming-cfs *args:
    exec /usr/bin/kyth-runtime recipe "gaming-cfs" {{ args }}

[group('KythOS')]
gaming-audit *args:
    exec /usr/bin/kyth-runtime recipe "gaming-audit" {{ args }}

[group('KythOS')]
steam-deadzone *args:
    exec /usr/bin/kyth-runtime recipe "steam-deadzone" {{ args }}

[group('KythOS')]
selinux-gaming *args:
    exec /usr/bin/kyth-runtime recipe "selinux-gaming" {{ args }}

[group('KythOS')]
fcitx-latency *args:
    exec /usr/bin/kyth-runtime recipe "fcitx-latency" {{ args }}

[group('KythOS')]
hdr-store *args:
    exec /usr/bin/kyth-runtime recipe "hdr-store" {{ args }}

[group('KythOS')]
system-audit *args:
    exec /usr/bin/kyth-runtime recipe "system-audit" {{ args }}

[group('KythOS')]
psi-gaming *args:
    exec /usr/bin/kyth-runtime recipe "psi-gaming" {{ args }}

[group('KythOS')]
numa *args:
    exec /usr/bin/kyth-runtime recipe "numa" {{ args }}

[group('KythOS')]
pcie *args:
    exec /usr/bin/kyth-runtime recipe "pcie" {{ args }}

[group('KythOS')]
thp-collapse *args:
    exec /usr/bin/kyth-runtime recipe "thp-collapse" {{ args }}

[group('KythOS')]
psi-poll *args:
    exec /usr/bin/kyth-runtime recipe "psi-poll" {{ args }}

[group('KythOS')]
compaction *args:
    exec /usr/bin/kyth-runtime recipe "compaction" {{ args }}

[group('KythOS')]
fscache *args:
    exec /usr/bin/kyth-runtime recipe "fscache" {{ args }}

[group('KythOS')]
hdr-per-game *args:
    exec /usr/bin/kyth-runtime recipe "hdr-per-game" {{ args }}

[group('KythOS')]
bore *args:
    exec /usr/bin/kyth-runtime recipe "bore" {{ args }}

[group('KythOS')]
shader-cache-size *args:
    exec /usr/bin/kyth-runtime recipe "shader-cache-size" {{ args }}

[group('KythOS')]
podman-overlay *args:
    exec /usr/bin/kyth-runtime recipe "podman-overlay" {{ args }}

[group('KythOS')]
net-backlog *args:
    exec /usr/bin/kyth-runtime recipe "net-backlog" {{ args }}

[group('KythOS')]
vm-watermark *args:
    exec /usr/bin/kyth-runtime recipe "vm-watermark" {{ args }}

[group('KythOS')]
tcp-notsent *args:
    exec /usr/bin/kyth-runtime recipe "tcp-notsent" {{ args }}

[group('KythOS')]
max-map-count *args:
    exec /usr/bin/kyth-runtime recipe "max-map-count" {{ args }}

[group('KythOS')]
dirty-ratio *args:
    exec /usr/bin/kyth-runtime recipe "dirty-ratio" {{ args }}

[group('KythOS')]
vfs-cache *args:
    exec /usr/bin/kyth-runtime recipe "vfs-cache" {{ args }}

[group('KythOS')]
tcp-ecn *args:
    exec /usr/bin/kyth-runtime recipe "tcp-ecn" {{ args }}

[group('KythOS')]
tcp-slow-start *args:
    exec /usr/bin/kyth-runtime recipe "tcp-slow-start" {{ args }}

[group('KythOS')]
sched-autogroup *args:
    exec /usr/bin/kyth-runtime recipe "sched-autogroup" {{ args }}

[group('KythOS')]
sched-nr-migrate *args:
    exec /usr/bin/kyth-runtime recipe "sched-nr-migrate" {{ args }}

[group('KythOS')]
page-cluster *args:
    exec /usr/bin/kyth-runtime recipe "page-cluster" {{ args }}

[group('KythOS')]
tcp-retries2 *args:
    exec /usr/bin/kyth-runtime recipe "tcp-retries2" {{ args }}

[group('KythOS')]
tcp-keepalive *args:
    exec /usr/bin/kyth-runtime recipe "tcp-keepalive" {{ args }}

[group('KythOS')]
sched-child *args:
    exec /usr/bin/kyth-runtime recipe "sched-child" {{ args }}

[group('KythOS')]
vm-stat *args:
    exec /usr/bin/kyth-runtime recipe "vm-stat" {{ args }}

[group('KythOS')]
numa-balancing *args:
    exec /usr/bin/kyth-runtime recipe "numa-balancing" {{ args }}

[group('KythOS')]
tcp-fastopen *args:
    exec /usr/bin/kyth-runtime recipe "tcp-fastopen" {{ args }}

[group('KythOS')]
tcp-mtu-probing *args:
    exec /usr/bin/kyth-runtime recipe "tcp-mtu-probing" {{ args }}

[group('KythOS')]
dirty-expire *args:
    exec /usr/bin/kyth-runtime recipe "dirty-expire" {{ args }}

[group('KythOS')]
file-max *args:
    exec /usr/bin/kyth-runtime recipe "file-max" {{ args }}

[group('KythOS')]
perf-cpu *args:
    exec /usr/bin/kyth-runtime recipe "perf-cpu" {{ args }}

[group('KythOS')]
swappiness *args:
    exec /usr/bin/kyth-runtime recipe "swappiness" {{ args }}

[group('KythOS')]
tcp-fin-timeout *args:
    exec /usr/bin/kyth-runtime recipe "tcp-fin-timeout" {{ args }}

[group('KythOS')]
somaxconn *args:
    exec /usr/bin/kyth-runtime recipe "somaxconn" {{ args }}

[group('KythOS')]
inotify-watches *args:
    exec /usr/bin/kyth-runtime recipe "inotify-watches" {{ args }}

[group('KythOS')]
min-free-kbytes *args:
    exec /usr/bin/kyth-runtime recipe "min-free-kbytes" {{ args }}

[group('KythOS')]
rmem-max *args:
    exec /usr/bin/kyth-runtime recipe "rmem-max" {{ args }}

[group('KythOS')]
wmem-max *args:
    exec /usr/bin/kyth-runtime recipe "wmem-max" {{ args }}

[group('KythOS')]
aio-max *args:
    exec /usr/bin/kyth-runtime recipe "aio-max" {{ args }}

[group('KythOS')]
overcommit-memory *args:
    exec /usr/bin/kyth-runtime recipe "overcommit-memory" {{ args }}

[group('KythOS')]
netdev-budget *args:
    exec /usr/bin/kyth-runtime recipe "netdev-budget" {{ args }}

[group('KythOS')]
rmem-default *args:
    exec /usr/bin/kyth-runtime recipe "rmem-default" {{ args }}

[group('KythOS')]
wmem-default *args:
    exec /usr/bin/kyth-runtime recipe "wmem-default" {{ args }}

[group('KythOS')]
tcp-window-scaling *args:
    exec /usr/bin/kyth-runtime recipe "tcp-window-scaling" {{ args }}

[group('KythOS')]
tcp-sack *args:
    exec /usr/bin/kyth-runtime recipe "tcp-sack" {{ args }}

[group('KythOS')]
tcp-timestamps *args:
    exec /usr/bin/kyth-runtime recipe "tcp-timestamps" {{ args }}

[group('KythOS')]
busy-read *args:
    exec /usr/bin/kyth-runtime recipe "busy-read" {{ args }}

[group('KythOS')]
busy-poll *args:
    exec /usr/bin/kyth-runtime recipe "busy-poll" {{ args }}

[group('KythOS')]
tcp-no-metrics-save *args:
    exec /usr/bin/kyth-runtime recipe "tcp-no-metrics-save" {{ args }}

[group('KythOS')]
tcp-retries1 *args:
    exec /usr/bin/kyth-runtime recipe "tcp-retries1" {{ args }}

[group('KythOS')]
tcp-orphan-retries *args:
    exec /usr/bin/kyth-runtime recipe "tcp-orphan-retries" {{ args }}

[group('KythOS')]
epp-ac *args:
    exec /usr/bin/kyth-runtime recipe "epp-ac" {{ args }}

[group('KythOS')]
zink-run *args:
    exec /usr/bin/kyth-runtime recipe "zink-run" {{ args }}

[group('KythOS')]
low-latency *args:
    exec /usr/bin/kyth-runtime recipe "low-latency" {{ args }}

[group('KythOS')]
enable-bpftune *args:
    exec /usr/bin/kyth-runtime recipe "enable-bpftune" {{ args }}

[group('KythOS')]
disable-bpftune *args:
    exec /usr/bin/kyth-runtime recipe "disable-bpftune" {{ args }}

[group('KythOS')]
probe-json *args:
    exec /usr/bin/kyth-runtime recipe "probe-json" {{ args }}

[group('KythOS')]
apply-preset *args:
    exec /usr/bin/kyth-runtime recipe "apply-preset" {{ args }}

[group('KythOS')]
list-presets *args:
    exec /usr/bin/kyth-runtime recipe "list-presets" {{ args }}

[group('KythOS')]
secureboot-status *args:
    exec /usr/bin/kyth-runtime recipe "secureboot-status" {{ args }}

[group('KythOS')]
enroll-secureboot *args:
    exec /usr/bin/kyth-runtime recipe "enroll-secureboot" {{ args }}

[group('KythOS')]
setup-sunshine *args:
    exec /usr/bin/kyth-runtime recipe "setup-sunshine" {{ args }}

[group('KythOS')]
setup-vr *args:
    exec /usr/bin/kyth-runtime recipe "setup-vr" {{ args }}

[group('KythOS')]
setup-tailscale *args:
    exec /usr/bin/kyth-runtime recipe "setup-tailscale" {{ args }}

[group('KythOS')]
update *args:
    exec /usr/bin/kyth-runtime recipe "update" {{ args }}

[group('KythOS')]
kyth-upgrade *args:
    exec /usr/bin/kyth-runtime recipe "kyth-upgrade" {{ args }}

[group('KythOS')]
apply-staged *args:
    exec /usr/bin/kyth-runtime recipe "apply-staged" {{ args }}

[group('KythOS')]
status *args:
    exec /usr/bin/kyth-runtime recipe "status" {{ args }}

[group('KythOS')]
update-health *args:
    exec /usr/bin/kyth-runtime recipe "update-health" {{ args }}

[group('KythOS')]
retry-quarantined-update *args:
    exec /usr/bin/kyth-runtime recipe "retry-quarantined-update" {{ args }}

[group('KythOS')]
rebase *args:
    exec /usr/bin/kyth-runtime recipe "rebase" {{ args }}

[group('KythOS')]
switch-channel *args:
    exec /usr/bin/kyth-runtime recipe "switch-channel" {{ args }}

[group('KythOS')]
switch-channel-impl *args:
    exec /usr/bin/kyth-runtime recipe "switch-channel-impl" {{ args }}

[group('KythOS')]
switch-kernel *args:
    exec /usr/bin/kyth-runtime recipe "switch-kernel" {{ args }}

[group('KythOS')]
flatpak-trim *args:
    exec /usr/bin/kyth-runtime recipe "flatpak-trim" {{ args }}

