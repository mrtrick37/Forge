"""Canonical hardware view — single Evaluation truth for Hub and boot policy.

`kyth-hardware-policy` is the only component that understands
`hardware-profiles.toml` matching. `services/hardware/*` previously re-parsed
`lspci`/`lsusb` for the same GPU/hybrid decision, so the Hub could disagree
with `kyth-hw-setup`.

This module is the single re-export the Hub should import for
“what hardware do we have” — it returns the typed `Evaluation` plus the
persisted applied state, both via the unified ProbeService cache so
`lspci`/`lsusb`/`dmidecode` probes are not re-spawned per page.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from kyth_shared.system.hardware_native import status as native_status
from kyth_shared.system.probe import probe_cached

_HARDWARE_VIEW_TTL = 30.0


@dataclass(frozen=True, slots=True)
class HardwareView:
    evaluation: Any
    applied: dict[str, Any]
    has_nvidia: bool
    is_hybrid: bool


def _has_nvidia_from_inventory(inv: dict[str, Any]) -> bool:
    return any(
        device.get("vendor") == "10de"
        and str(device.get("class_code", "")).startswith("03")
        for device in inv.get("pci", [])
        if isinstance(device, dict)
    )


def _is_hybrid_from_evaluation(eval_: dict[str, Any]) -> bool:
    caps = set(eval_.get("capabilities", []))
    return "gpu.hybrid" in caps or "gpu.offload" in caps


def get_hardware_view() -> HardwareView:
    def _fetch() -> HardwareView:
        payload = native_status()
        evaluation = payload["evaluation"]
        applied = payload.get("applied", {})
        has_nvidia = _has_nvidia_from_inventory(evaluation.get("inventory", {}))
        is_hybrid = _is_hybrid_from_evaluation(evaluation)
        return HardwareView(evaluation, applied, has_nvidia, is_hybrid)

    return probe_cached("hardware-view", _HARDWARE_VIEW_TTL, _fetch)


def invalidate_hardware_view() -> None:
    """Invalidate the cached hardware view so the next get_hardware_view() re-probes."""
    from kyth_shared.system.probe import invalidate_probe_caches

    invalidate_probe_caches(["hardware-view", "hardware-summary"])
