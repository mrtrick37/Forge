//! Pure parsers for output produced by host/runtime commands.
//!
//! These mirror `kyth_shared.runtime_output`.  Keeping parsing separate from
//! process execution makes the Rust ports fixture-testable and prevents a
//! malformed command response from turning into a guessed healthy state.

use serde_json::Value;

pub fn parse_json_object(raw: &str) -> Option<Value> {
    let value: Value = serde_json::from_str(raw).ok()?;
    value.is_object().then_some(value)
}

pub fn parse_lsblk_devices(raw: &str) -> Vec<Value> {
    parse_json_object(raw)
        .and_then(|value| value.get("blockdevices").cloned())
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
}

pub fn parse_ntfs_devices(raw: &str) -> Vec<Value> {
    fn walk(devices: &[Value], found: &mut Vec<Value>) {
        for device in devices {
            if device
                .get("fstype")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_lowercase()
                .contains("ntfs")
            {
                found.push(device.clone());
            }
            if let Some(children) = device.get("children").and_then(Value::as_array) {
                walk(children, found);
            }
        }
    }

    let devices = parse_lsblk_devices(raw);
    let mut found = Vec::new();
    walk(&devices, &mut found);
    found
}

pub fn parse_findmnt_sources(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn parse_flatpak_apps(raw: &str) -> Vec<Value> {
    raw.lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split('\t').map(str::trim).collect();
            if fields.len() < 4 || fields[0].is_empty() {
                return None;
            }
            Some(serde_json::json!({
                "kind": "flatpak",
                "app_id": fields[0],
                "name": if fields[1].is_empty() { fields[0] } else { fields[1] },
                "origin": if fields[2].is_empty() { "unknown" } else { fields[2] },
                "installation": if fields[3].is_empty() { "system" } else { fields[3] },
            }))
        })
        .collect()
}

pub fn count_fwupd_updates(raw: &str) -> usize {
    raw.lines()
        .filter(|line| line.trim_start().starts_with("Device ID:"))
        .count()
}

pub fn parse_secure_boot_state(raw: &str) -> &'static str {
    let normalized = raw.trim().to_lowercase();
    if normalized.contains("secureboot enabled") {
        "enabled"
    } else if normalized.contains("secureboot disabled") {
        "disabled"
    } else {
        "unknown"
    }
}

pub fn parse_systemd_state(raw: &str) -> &'static str {
    match raw.trim().to_lowercase().as_str() {
        "active" => "active",
        "activating" => "activating",
        "inactive" => "inactive",
        "failed" => "failed",
        "deactivating" => "deactivating",
        _ => "unknown",
    }
}

pub fn parse_nvidia_smi(raw: &str) -> Option<(String, String)> {
    let first = raw.lines().map(str::trim).find(|line| !line.is_empty())?;
    let (name, version) = first.split_once(',')?;
    let name = name.trim();
    let version = version.trim();
    (!name.is_empty() && !version.is_empty()).then(|| (name.to_string(), version.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_ntfs_devices() {
        let raw = r#"{"blockdevices":[{"name":"nvme0n1","children":[{"name":"nvme0n1p4","fstype":"ntfs"}]}]}"#;
        let devices = parse_ntfs_devices(raw);
        assert_eq!(devices[0]["name"], "nvme0n1p4");
    }

    #[test]
    fn parses_runtime_fixture_shapes() {
        assert!(parse_json_object("{bad").is_none());
        assert_eq!(count_fwupd_updates("Device ID: one\n\n Device ID: two"), 2);
        assert_eq!(parse_secure_boot_state("SecureBoot enabled"), "enabled");
        assert_eq!(parse_systemd_state("FAILED"), "failed");
        assert_eq!(
            parse_nvidia_smi("GPU, 590.48.01"),
            Some(("GPU".into(), "590.48.01".into()))
        );
    }
}
