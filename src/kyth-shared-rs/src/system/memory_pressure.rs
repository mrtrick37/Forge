//! Port of kyth_shared.system.memory_pressure — psi / memavailable advisory.
use std::fs;
pub fn memory_pressure_status() -> (String, String) {
    // Mirrors Python's PSI check: /proc/pressure/memory + MemAvailable
    let psi = fs::read_to_string("/proc/pressure/memory").unwrap_or_default();
    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let avail_kb: u64 = meminfo
        .lines()
        .find(|l| l.starts_with("MemAvailable"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if psi.contains("avg10=") && avail_kb < 500_000 {
        (
            "warn".to_string(),
            "Memory pressure high — close heavy apps".to_string(),
        )
    } else if avail_kb == 0 {
        ("unknown".to_string(), "memory check pending".to_string())
    } else {
        ("ok".to_string(), "memory ok".to_string())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn returns_tuple() {
        let (s, _) = memory_pressure_status();
        assert!(["ok", "warn", "unknown"].contains(&s.as_str()));
    }
}
