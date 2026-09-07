<div align="center">

<img src="build_files/branding/kyth-logo-transparent.svg" alt="KythOS" width="190">

# KythOS

**A calm Linux desktop for people coming from Windows—built for games, work, creativity, and a way back when something goes wrong.**

[Download stable](https://pub-9a3cc72972ea44c4ae7504ee7cda1fa6.r2.dev/kyth-live-latest.iso) · [Try testing](https://pub-9a3cc72972ea44c4ae7504ee7cda1fa6.r2.dev/kyth-live-testing.iso) · [Get help](https://github.com/kyth-os/kyth/discussions) · [Report a bug](https://github.com/kyth-os/kyth/issues)

[![Build](https://github.com/kyth-os/kyth/actions/workflows/build.yml/badge.svg?branch=main)](https://github.com/kyth-os/kyth/actions/workflows/build.yml)
[![Live ISO](https://github.com/kyth-os/kyth/actions/workflows/build-live-iso.yml/badge.svg)](https://github.com/kyth-os/kyth/actions/workflows/build-live-iso.yml)
[![CVE Scan](https://github.com/kyth-os/kyth/actions/workflows/cve-scan.yml/badge.svg)](https://github.com/kyth-os/kyth/actions/workflows/cve-scan.yml)

</div>

## Linux without the scavenger hunt

KythOS is a ready-to-install Linux desktop for an x86-64 PC. It pairs KDE Plasma with thoughtful defaults for gaming, everyday work, creative tools, and recovery—without asking you to turn system administration into a hobby.

If you are moving from Windows, the biggest difference is simple: KythOS guides the things that need guidance and keeps the complicated parts available when you actually need them. **Kyth Hub** is the place to start. It helps you set up your computer, find apps, bring over games and files, understand updates, and get help when something feels off.

KythOS is not a promise that every Windows program, anti-cheat system, driver, or vendor utility will work on Linux. It is an opinionated daily driver that makes those boundaries clear before they become surprises.

## Start with Kyth Hub

<div align="center">
<img src="docs/system-hub-home.png" alt="The Kyth Hub home screen" width="100%">
</div>

Kyth Hub is the guided control center for KythOS. Instead of sending you through settings panels, terminal commands, and web searches, it gives you one clear next step and keeps the useful controls close by.

| In Kyth Hub | What it helps you do |
| --- | --- |
| **Home** | See your PC’s health, what needs attention, and where to go next. |
| **Play** | Set up game launchers, check compatibility context, connect controllers, and use performance tools when a game needs them. |
| **Apps** | Find Linux apps, familiar alternatives, and a work-ready setup. |
| **This PC** | Install updates, check hardware, run diagnostics, and use repair tools. |
| **Move In** | Bring across files and saves; connect cloud storage, network shares, and VPNs. |

Kyth Hub is designed to explain an action before it takes it. If there is a meaningful choice—such as an update, a repair path, or a Windows installer—it should be understandable and reversible where possible.

## What changes when you leave Windows

Most day-to-day applications on KythOS come from the App Store and Flathub rather than from downloading installers from the web. That means updates and permissions are handled in one place, and the base system stays focused on being a reliable desktop.

You can still encounter a Windows `.exe` or `.msi`. Kyth Hub can point you toward a Linux equivalent, explain the compatibility trade-off, or guide an appropriate Windows-app workflow through Bottles. It does not pretend every installer is safe or suitable to run.

Before switching, check the things that are personal to your setup:

- Some multiplayer games with Windows-only or kernel-level anti-cheat do not run on Linux.
- Some Adobe, enterprise, hardware-vendor, and driver-dependent applications need a Linux alternative, a web version, a virtual machine, or a Windows PC.
- Your own games deserve a quick compatibility check before you erase or repurpose a Windows installation.

The [gaming compatibility guide](docs/gaming-validation-matrix.md), [recorded game results](docs/gaming-results/README.md), and [everyday-use notes](docs/works-better-here.md) are there when you want the detail.

## Bring your games, files, and saves

KythOS is built to make a move practical, not ceremonial. Steam libraries on Windows drives can be found and copied, common launchers can be installed from Hub, and the migration area covers cloud storage, shared folders, VPNs, and personal files.

For game saves, let cloud sync finish on Windows first and keep an external backup. Launch the game once on KythOS to create its Linux or Proton environment, restore the save, then confirm it in-game before deleting anything. The [game-save migration guide](docs/game-save-migration.md) walks through the safe version of that process.

## Updates on your schedule, with a way back

An operating-system update is prepared as a complete next version of KythOS. You restart when you are ready; if the new version is not right for you, the previous one remains available from the boot menu.

That recovery path is part of the product, not a last-minute repair trick. Kyth Hub shows what is happening and offers the relevant checks and repair tools instead of making you memorize a recovery procedure. Read about [how update safety works](docs/update-safety.md) when you want the full picture.

## Built for the desktop, backed by Rust

Kyth Hub uses a responsive Tauri and React interface backed by focused Rust services for system actions, updates, checks, and installer handling. In plain terms: the interface can stay approachable while the work underneath remains deliberate, bounded, and easy to inspect.

Rust is not the point of using KythOS; dependable desktop behavior is. It is one of the ways the project keeps important work close to the operating system instead of turning Hub into a terminal-first control panel.

## Install KythOS

| Channel | Choose it when | Download |
| --- | --- | --- |
| **Stable** | You want the current daily-driver release. | [Download stable ISO](https://pub-9a3cc72972ea44c4ae7504ee7cda1fa6.r2.dev/kyth-live-latest.iso) |
| **Testing** | You want to help try new work before it reaches stable. | [Download testing ISO](https://pub-9a3cc72972ea44c4ae7504ee7cda1fa6.r2.dev/kyth-live-testing.iso) |

You need an x86-64 PC, a USB drive, and at least 8 GB of RAM for the live environment. **Back up anything important before changing partitions or selecting an install disk.**

1. Download the ISO for the channel you chose.
2. Write it to a USB drive with [Fedora Media Writer](https://fedoraproject.org/workstation/download/), Balena Etcher, Ventoy, or another raw-image writer.
3. Boot from that USB drive and choose **Install KythOS**.
4. Read the disk-selection screen carefully, choose the installation layout, and create your local user.
5. After the first restart, open **Kyth Hub** and follow its short setup checklist.

If you are installing alongside Windows, make a recovery drive and a current backup first. The installer can guide an installation, but it cannot make an unsafe disk choice safe. Stable and testing release records are also available on [GitHub](https://github.com/kyth-os/kyth/releases).

## Is KythOS for you?

KythOS is a good fit if you want a desktop that is ready for Steam and modern Linux apps, prefer guided maintenance to manual tuning, and value updates with a recovery path.

Take a closer look before switching full-time if your work depends on a particular Windows-only application, a company-managed device policy, a specialized driver, or a game with uncertain anti-cheat support. You can boot the USB drive first and explore without installing; keeping Windows available during a gradual move is completely reasonable.

## Support, privacy, and project status

Ask questions in [GitHub Discussions](https://github.com/kyth-os/kyth/discussions) and file reproducible problems in [GitHub Issues](https://github.com/kyth-os/kyth/issues). A support snapshot from Kyth Hub excludes stored passwords, browser sessions, SMB credentials, and cloud OAuth tokens.

KythOS enables Fedora’s anonymous DNF CountMe mechanism: Fedora receives an age bucket during a weekly repository request. KythOS does not create an account, send a per-machine identifier, or claim an install count from Fedora’s aggregate.

KythOS is licensed under [Apache License 2.0](LICENSE) and is not affiliated with Fedora, Universal Blue, KDE, Valve, CachyOS, or any game publisher.

<details>
<summary><strong>Technical documentation and contributor resources</strong></summary>

This section is for people who want the implementation, validation, and release detail behind the desktop experience.

### Design, safety, and support

- [Architecture](docs/architecture.md)
- [Stability principles](docs/stability-principles.md)
- [Security model](docs/security-model.md) and [security reporting policy](SECURITY.md)
- [Update safety and recovery lifecycle](docs/update-safety.md)
- [Guardian design and repair-policy boundaries](docs/guardian.md)
- [Hardware policy](docs/hardware-policy.md) and [hardware support matrix](docs/hardware-support-matrix.md)
- [Release support policy](docs/release-support.md)
- [Daily-driver validation](docs/daily-driver-validation.md)

### Gaming and migration evidence

- [Gaming validation matrix](docs/gaming-validation-matrix.md)
- [Recorded gaming results](docs/gaming-results/README.md)
- [Windows game-save migration](docs/game-save-migration.md)
- [Modding on KythOS](docs/modding-on-kythos.md)

### Hub, Rust, installer, and migration work

- [Runtime migration report](docs/runtime-migration-report.md)
- [Rust migration completion plan](docs/rust-migration-completion-plan.md)
- [Kyth Hub migration finalization plan](docs/kyth-hub-migration-finalization-plan.md)
- [Kyth Hub Rust release checklist](docs/system-hub-rust-release-checklist.md)
- [Installer migration plan](docs/installer-migration-plan.md)
- [Installer API contract](docs/installer-api-contract.md) and [feature-parity record](docs/installer-feature-parity.md)

### Local development and release verification

The development path assumes Linux, Git, Docker, Python 3, and [`just`](https://github.com/casey/just). QEMU and SPICE are used for native live-ISO checks. The project’s tracked commands cover the normal local workflow:

```bash
just test                 # unit tests
just validate             # GitHub validation parity
just ci-preflight         # validation, quality, and security checks
just build                # assemble a local image
just build-live-iso       # create an ISO from the stable channel image
just run-live-iso-native-local  # boot a fresh local ISO in QEMU
```

See the [dependency guide](docs/dependency-management.md), [optimization measurements](docs/optimization-budgets.md), and the repository’s `AGENTS.md` for contributor conventions and publishing rules.

</details>
