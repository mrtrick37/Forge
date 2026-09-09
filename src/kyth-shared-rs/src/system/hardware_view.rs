//! Port of `kyth_shared.system.hardware_view` — canonical hardware view.
//! Prefer the ProbeService cache so normal Hub navigation remains cheap; when
//! the cache is unavailable, fall back to the Rust read-only hardware policy
//! evaluator. Policy application and modprobe writes remain Python-owned.

use std::collections::HashMap;

use serde_json::Value;

#[derive(Debug, Clone, serde::Serialize)]
pub struct HardwareViewSummary {
    pub has_nvidia: bool,
    pub is_hybrid: bool,
    pub capabilities: Vec<String>,
    pub applied: HashMap<String, Value>,
}

pub fn get_hardware_view_summary() -> Option<HardwareViewSummary> {
    // Read via existing probe helper — reuses DISK_TTL 30s and cache_read_paths()
    if let Some(raw) = crate::system::probe::read_section("hardware-summary") {
        let obj = raw.as_object()?;
        let has_nvidia = obj
            .get("has_nvidia")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let is_hybrid = obj
            .get("is_hybrid")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let capabilities = obj
            .get("capabilities")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| e.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        return Some(HardwareViewSummary {
            has_nvidia,
            is_hybrid,
            capabilities,
            applied: HashMap::new(),
        });
    }

    let evaluation = crate::system::hardware_policy::evaluate_system().ok()?;
    let has_nvidia = evaluation
        .inventory
        .pci
        .iter()
        .any(|device| device.vendor == "10de" && device.class_code.starts_with("03"));
    let is_hybrid = evaluation
        .capabilities
        .iter()
        .any(|cap| cap == "gpu.hybrid" || cap == "gpu.offload");
    Some(HardwareViewSummary {
        has_nvidia,
        is_hybrid,
        capabilities: evaluation.capabilities,
        applied: HashMap::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn returns_option() {
        let _ = get_hardware_view_summary();
    }
}
