#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# ── VS Code Host Wrapper ──────────────────────────────────────────────────────
# VS Code is provided via the kyth-ai-dev container to keep the immutable core slim.
# Host wrapper transparently delegates to the container or prompts setup on first run.

/usr/bin/kyth-build-support container-wrapper \
	--tool code --description "Visual Studio Code" --output /usr/bin/code


# No GUI launcher is installed here: setup (triggered by the CLI wrapper above,
# or `ujust ai-dev-setup`) can take several minutes on first run with no visible
# progress in a menu-launched GUI app, which reads as a broken/hung icon.
# distrobox-export (kyth-ai-dev setup) installs the real "Visual Studio Code (on
# kyth-ai-dev)" launcher once the container actually has VS Code installed.
