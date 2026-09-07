# shellcheck shell=bash
# ── Read-only diagnostic/health-check scripts ──────────────────────────────────
# kyth-smoke-check is the native Rust binary copied from the hub-web-builder;
# the former Python entry point is retained only as source/test coverage.
 # kyth-resume-check is likewise supplied by the native Rust builder output.
# Native Rust compatibility-named utilities are copied into /usr/bin by the
# Dockerfile builder stage; no Python diagnostic entry points are installed.
install -m 0755 /ctx/kyth-creator-check /usr/bin/kyth-creator-check
install -m 0644 /ctx/config/qualification-budgets.json /usr/share/kyth/qualification-budgets.json
install -m 0755 /ctx/kyth-vm-acceptance-guest /usr/libexec/kyth-vm-acceptance-guest
install -m 0644 /ctx/kyth-vm-acceptance.service /usr/lib/systemd/system/kyth-vm-acceptance.service
systemctl enable kyth-vm-acceptance.service

install -m 0755 /ctx/kyth-boot-verify /usr/bin/kyth-boot-verify

# greenboot owns boot counting and rollback. These hooks add KythOS-specific
# required checks plus digest-aware success/failure and quarantine records.
install -Dm0755 /ctx/kyth-greenboot-required /etc/greenboot/check/required.d/40_kyth_core_health.sh
install -Dm0755 /ctx/kyth-greenboot-success /etc/greenboot/green.d/40_kyth_record_success.sh
install -Dm0755 /ctx/kyth-greenboot-failure /etc/greenboot/red.d/40_kyth_record_failure.sh

# The required check polls for graphical.target, the display manager, and a
# DRM device (see kyth_shared/system/boot_runtime.py) because greenboot runs
# long before the display stack settles. Pin the runner's start timeout well
# above that poll budget: inheriting a shorter default would kill the check
# mid-wait and score a healthy boot as red — turning the rollback machinery
# into a reboot loop.
install -d /usr/lib/systemd/system/greenboot-healthcheck.service.d
cat >/usr/lib/systemd/system/greenboot-healthcheck.service.d/40-kyth-timeout.conf <<'EOF'
[Unit]
# Poll after restorecon so a large /var/home cannot consume the
# display-manager deadline and mark a healthy first boot red.
After=kyth-selinux-relabel-home.service
# RemainAfterExit=yes below is what stops multi-user.target's Wants= from
# restarting this: a start job on an already-active oneshot no-ops.
# StartLimit only backstops repeated *failures*, so disable it outright
# instead of giving it a window — these keys belong in [Unit], not
# [Service]; stranded in [Service] they're silently ignored and the unit
# runs under systemd's compiled-in 10s/5 default instead.
StartLimitIntervalSec=0
[Service]
TimeoutStartSec=600
# Upstream is Type=oneshot without RemainAfterExit; Wants= from
# multi-user.target would otherwise restart it into start-limit.
RemainAfterExit=yes
EOF
