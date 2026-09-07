# shellcheck shell=bash
# ── Right-click "New Document" templates for Dolphin ─────────────────────────
# Any file placed in ~/Templates appears in Dolphin's right-click → Create New
# → Document menu. Seeding /etc/skel ensures every new user gets the templates
# on first login.
mkdir -p /etc/skel/Templates
printf '' >"/etc/skel/Templates/Plain Text.txt"
printf '# Title\n\n' >"/etc/skel/Templates/Markdown.md"
printf '#!/usr/bin/env bash\nset -euo pipefail\n\n' >"/etc/skel/Templates/Shell Script.sh"
printf '#!/usr/bin/env python3\n\n\ndef main():\n    pass\n\n\nif __name__ == "__main__":\n    main()\n' \
	>"/etc/skel/Templates/Python Script.py"
chmod +x /etc/skel/Templates/"Shell Script.sh"
chmod +x /etc/skel/Templates/"Python Script.py"

install -m 0755 /ctx/kyth-rclone-update /usr/bin/kyth-rclone-update
# kyth-session-snapshot is the native Rust binary copied from the
# hub-web-builder stage; no Python launcher remains in the source tree.
# kyth-report-issue is the native Rust binary copied from the
# hub-web-builder stage; no Python launcher remains in the source tree.
install -m 0755 /ctx/kyth-proton-cachyos-update /usr/bin/kyth-proton-cachyos-update
# kyth-steam-game-export is copied into /usr/bin by the Rust builder stage.
install -m 0644 /ctx/kyth-proton-cachyos-update.service /usr/lib/systemd/system/kyth-proton-cachyos-update.service
install -m 0644 /ctx/kyth-proton-cachyos-update.timer /usr/lib/systemd/system/kyth-proton-cachyos-update.timer

install -m 0644 /ctx/kyth-flathub-setup.service /usr/lib/systemd/system/kyth-flathub-setup.service
install -m 0644 /ctx/kyth-default-flatpaks.service /usr/lib/systemd/system/kyth-default-flatpaks.service
install -m 0755 /ctx/kyth-hw-setup /usr/bin/kyth-hw-setup
install -m 0644 /ctx/kyth-hw-setup.service /usr/lib/systemd/system/kyth-hw-setup.service
# hw-setup's ProtectSystem=strict ReadWritePaths need these dirs to exist
# so namespace setup (and later atomic writes) cannot fail on AMD-only hosts.
install -d -m 0755 /etc/modprobe.d /etc/scx /var/lib/akmods /var/cache/akmods
install -Dm 0644 /ctx/config/hardware-profiles.toml /usr/share/kyth/hardware-profiles.toml
