# shellcheck shell=bash
# ── Misc maintenance/utility tools ─────────────────────────────────────────────
install -m 0755 /ctx/kyth-davinci-install /usr/bin/kyth-davinci-install
install -m 0755 /ctx/kyth-duperemove /usr/bin/kyth-duperemove
install -m 0755 /ctx/kyth-distrobox-root-launch /usr/bin/kyth-distrobox-root-launch
# kyth-kali-desktop-fixup is the native Rust binary copied from the
# hub-web-builder stage; no Python launcher remains in the source tree.
install -m 0755 /ctx/kyth-local-bin-migrate /usr/bin/kyth-local-bin-migrate
install -m 0755 /ctx/kyth-nearby-share /usr/bin/kyth-nearby-share
# kyth-setup-transfer is the native Rust binary copied from the
# hub-web-builder stage; no Python launcher remains in the source tree.
install -m 0755 /ctx/kyth-dynamic-lock /usr/bin/kyth-dynamic-lock
# kyth-duperemove.service/.timer and kyth-local-bin-migrate.service are
# installed in branding/31-ujust-recipes.sh instead, right before the
# `systemctl enable` calls that need them to already exist.
install -m 0755 /ctx/kyth-full-update /usr/bin/kyth-full-update
install -m 0755 /ctx/kyth-scx-loader /usr/bin/scx_loader
# kyth-doctor is the native Rust binary copied from the hub-web-builder stage;
# retain the Python launcher in the source tree for parity only.
install -m 0755 /ctx/kyth-windows-import /usr/bin/kyth-windows-import
# kyth-vscode-wallet is the native Rust binary copied from the
# hub-web-builder stage; no Python launcher remains in the source tree.
mkdir -p /usr/lib/systemd/user /usr/lib/systemd/user/default.target.wants
install -m 0644 /ctx/kyth-dynamic-lock.service /usr/lib/systemd/user/kyth-dynamic-lock.service
install -m 0644 /ctx/kyth-browser-wallet-defaults.service /usr/lib/systemd/user/kyth-browser-wallet-defaults.service
ln -sf ../kyth-browser-wallet-defaults.service \
	/usr/lib/systemd/user/default.target.wants/kyth-browser-wallet-defaults.service
