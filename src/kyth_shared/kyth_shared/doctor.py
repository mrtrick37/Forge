"""kyth-doctor — health score like cachy-doctor, reuses probe + hardware_policy.

Scores: kernel (fedora vs cachy), v3 (CACHYOS_ARCH), zram, btrfs, scx,
desktop stack (portals / PipeWire when a user session is present).
Suggests just fixes; no daemon, same probe code.
"""
from __future__ import annotations

from pathlib import Path

from kyth_shared.system.desktop_stack import desktop_stack_checks
from kyth_shared.system.probe import read_section
from kyth_shared.system.hardware_view import get_hardware_view


def _score() -> tuple[int, list[str], list[str]]:
    suggestions: list[str] = []
    checks: list[str] = []
    score = 0

    # kernel
    has_cachy = any("cachy" in p.name for p in Path("/usr/lib/modules").glob("*"))
    if has_cachy:
        checks.append("kernel: cachy (opt-in)")
        score += 20
    else:
        checks.append("kernel: fedora (default)")
        score += 20
        suggestions.append("For v3: just build-base cachy")

    # v3
    _ = Path("/usr/lib/os-release").read_text(errors="ignore") if Path("/usr/lib/os-release").exists() else ""
    # Use probe hardware-summary if available
    hw = read_section("hardware-summary")
    if hw and isinstance(hw, dict) and hw.get("capabilities"):
        checks.append(f"v3: {hw.get('capabilities')[:2]}")
        score += 20
    else:
        try:
            view = get_hardware_view()
            checks.append(f"v3: {view.evaluation.get('capabilities', [])[:2]}")
            score += 20
        except (OSError, ValueError, RuntimeError, AttributeError, KeyError):  # noqa: BLE001 -- narrow: best-effort production path
            checks.append("v3: unknown")
            suggestions.append("Run kyth-probe --system")

    # zram
    if Path("/usr/lib/systemd/zram-generator.conf").exists() or Path("/etc/systemd/zram-generator.conf").exists():
        checks.append("zram: yes")
        score += 20
    else:
        checks.append("zram: no")
        suggestions.append("Enable zram: systemctl enable --now kyth-zram-swap.service")

    # btrfs
    try:
        fstype = Path("/proc/mounts").read_text()
        if "btrfs" in fstype:
            checks.append("btrfs: yes")
            score += 20
        else:
            checks.append("btrfs: no")
    except (OSError, ValueError, RuntimeError, AttributeError, KeyError):  # noqa: BLE001 -- narrow: best-effort production path
        checks.append("btrfs: unknown")

    # scx
    scx_active = Path("/sys/kernel/sched_ext/state").exists()
    checks.append(f"scx: {'active' if scx_active else 'inactive (opt-in)'}")
    score += 20
    if not scx_active:
        suggestions.append("Try scx: kyth-scx set lavd")

    # Desktop stack (portals / PipeWire) — report only; do not inflate the 100 scale.
    stack = desktop_stack_checks()
    hard_fails = [c for c in stack if not c.passed and not c.advisory]
    soft_fails = [c for c in stack if not c.passed and c.advisory]
    if hard_fails:
        score = max(0, score - 15)
        checks.append("desktop-stack: " + "; ".join(c.detail for c in hard_fails))
        suggestions.append("Ensure xdg-desktop-portal + xdg-desktop-portal-kde are on the image")
    else:
        checks.append("desktop-stack: packages ok")
    for fail in soft_fails:
        checks.append(f"desktop-stack warn: {fail.name}: {fail.detail}")
        if "portal" in fail.name.lower():
            suggestions.append(
                "Restart portals: systemctl --user restart xdg-desktop-portal xdg-desktop-portal-kde"
            )
        elif fail.name in ("PipeWire", "WirePlumber"):
            suggestions.append(
                "Restart audio: systemctl --user restart pipewire pipewire-pulse wireplumber"
            )

    return min(score, 100), checks, suggestions


def main() -> int:
    score, checks, suggestions = _score()
    print(f"KythOS health: {score}/100")
    for c in checks:
        print(f" - {c}")
    if suggestions:
        print("\nSuggestions (just):")
        for s in suggestions:
            print(f"  * {s}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
