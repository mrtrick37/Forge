//! Read-only hardware inventory and policy evaluation.
//!
//! This is the safe half of `kyth_shared.hardware_policy`: it reads sysfs,
//! DMI, and `/proc/cpuinfo`, parses the data-only TOML policy, and evaluates
//! selectors.  Applying modprobe/scheduler configuration remains Python-owned
//! and is deliberately not represented here.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_POLICY_PATH: &str = "/usr/share/kyth/hardware-profiles.toml";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Device {
    pub bus: String,
    pub vendor: String,
    pub device: String,
    #[serde(default)]
    pub class_code: String,
    #[serde(default)]
    pub driver: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Inventory {
    pub cpu_vendor: String,
    pub dmi_vendor: String,
    pub dmi_product: String,
    pub dmi_board: String,
    pub pci: Vec<Device>,
    pub usb: Vec<Device>,
}

impl Inventory {
    pub fn digest(&self) -> String {
        let payload = serde_json::to_vec(self).unwrap_or_default();
        format!("{:x}", Sha256::digest(payload))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Policy {
    pub schema_version: i64,
    pub policy_revision: String,
    #[serde(default)]
    pub variants: Vec<Variant>,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub quirks: Vec<Quirk>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Variant {
    pub id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub qualification: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Profile {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub tier: String,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub image_variant: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(rename = "match", default)]
    pub selector: Match,
    #[serde(default)]
    pub policy: ProfilePolicy,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProfilePolicy {
    #[serde(default)]
    pub scheduler_candidates: Vec<String>,
    #[serde(default)]
    pub nvidia_setup: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Quirk {
    pub id: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub expires_on: String,
    #[serde(default)]
    pub provenance: String,
    #[serde(rename = "match", default)]
    pub selector: Match,
    #[serde(default)]
    pub actions: Vec<QuirkAction>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct QuirkAction {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub module: String,
    #[serde(default)]
    pub options: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Match {
    #[serde(default)]
    pub always: bool,
    #[serde(default)]
    pub pci: Vec<DeviceSelector>,
    #[serde(default)]
    pub usb: Vec<DeviceSelector>,
    #[serde(default)]
    pub cpu_vendors: Vec<String>,
    #[serde(default)]
    pub dmi_vendors: Vec<String>,
    #[serde(default)]
    pub dmi_products: Vec<String>,
    #[serde(default)]
    pub dmi_boards: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DeviceSelector {
    #[serde(default)]
    pub vendor: String,
    #[serde(default)]
    pub devices: Vec<String>,
    #[serde(default)]
    pub classes: Vec<String>,
    #[serde(default)]
    pub drivers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Evaluation {
    pub policy_revision: String,
    pub policy_digest: String,
    pub inventory: Inventory,
    pub profiles: Vec<Value>,
    pub quirks: Vec<Value>,
    pub capabilities: Vec<String>,
    pub recommended_variant: String,
    pub warnings: Vec<String>,
}

fn read_text(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn sysfs_hex(path: &Path) -> String {
    read_text(path)
        .to_lowercase()
        .trim_start_matches("0x")
        .to_string()
}

fn driver_name(path: &Path) -> String {
    fs::canonicalize(path)
        .ok()
        .and_then(|resolved| {
            resolved
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_default()
}

fn sorted_directories(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    paths
}

/// Collect stable matching identifiers without invoking external utilities.
pub fn collect_inventory() -> Inventory {
    collect_inventory_from(
        Path::new("/sys/bus/pci/devices"),
        Path::new("/sys/bus/usb/devices"),
        Path::new("/sys/class/dmi/id"),
        Path::new("/proc/cpuinfo"),
    )
}

pub fn collect_inventory_from(
    pci_root: &Path,
    usb_root: &Path,
    dmi_root: &Path,
    cpuinfo_path: &Path,
) -> Inventory {
    let mut pci = Vec::new();
    for node in sorted_directories(pci_root) {
        let vendor = sysfs_hex(&node.join("vendor"));
        let device = sysfs_hex(&node.join("device"));
        if vendor.is_empty() || device.is_empty() {
            continue;
        }
        pci.push(Device {
            bus: "pci".to_string(),
            vendor,
            device,
            class_code: sysfs_hex(&node.join("class")),
            driver: driver_name(&node.join("driver")),
            name: String::new(),
        });
    }

    let mut usb = Vec::new();
    for node in sorted_directories(usb_root) {
        let vendor = sysfs_hex(&node.join("idVendor"));
        let device = sysfs_hex(&node.join("idProduct"));
        if vendor.is_empty() || device.is_empty() {
            continue;
        }
        let mut driver = driver_name(&node.join("driver"));
        if driver.is_empty() {
            let prefix = node
                .file_name()
                .map(|name| format!("{}:", name.to_string_lossy()))
                .unwrap_or_default();
            if let Some(parent) = node.parent() {
                for child in sorted_directories(parent) {
                    if child
                        .file_name()
                        .map(|name| name.to_string_lossy().starts_with(&prefix))
                        .unwrap_or(false)
                    {
                        driver = driver_name(&child.join("driver"));
                        if !driver.is_empty() {
                            break;
                        }
                    }
                }
            }
        }
        usb.push(Device {
            bus: "usb".to_string(),
            vendor,
            device,
            class_code: String::new(),
            driver,
            name: read_text(&node.join("product")),
        });
    }

    let cpu_vendor = fs::read_to_string(cpuinfo_path)
        .unwrap_or_default()
        .lines()
        .find_map(|line| {
            line.strip_prefix("vendor_id")
                .and_then(|value| value.strip_prefix(':'))
                .map(str::trim)
                .map(str::to_string)
        })
        .unwrap_or_default();
    Inventory {
        cpu_vendor,
        dmi_vendor: read_text(&dmi_root.join("sys_vendor")),
        dmi_product: read_text(&dmi_root.join("product_name")),
        dmi_board: read_text(&dmi_root.join("board_name")),
        pci,
        usb,
    }
}

pub fn load_policy(path: &Path) -> Result<(Policy, String), String> {
    let raw = fs::read(path).map_err(|err| format!("could not read hardware policy: {err}"))?;
    let text = String::from_utf8(raw.clone())
        .map_err(|err| format!("hardware policy is not UTF-8: {err}"))?;
    let policy: Policy =
        toml::from_str(&text).map_err(|err| format!("could not parse hardware policy: {err}"))?;
    if policy.schema_version != 1 {
        return Err(format!(
            "unsupported hardware policy schema {}",
            policy.schema_version
        ));
    }
    Ok((policy, format!("{:x}", Sha256::digest(raw))))
}

fn glob_match(value: &str, pattern: &str) -> bool {
    let value: Vec<char> = value.to_lowercase().chars().collect();
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let mut dp = vec![vec![false; pattern.len() + 1]; value.len() + 1];
    dp[0][0] = true;
    for j in 1..=pattern.len() {
        if pattern[j - 1] == '*' {
            dp[0][j] = dp[0][j - 1];
        }
    }
    for i in 1..=value.len() {
        for j in 1..=pattern.len() {
            dp[i][j] = match pattern[j - 1] {
                '*' => dp[i][j - 1] || dp[i - 1][j],
                '?' => dp[i - 1][j - 1],
                ch => ch == value[i - 1] && dp[i - 1][j - 1],
            };
        }
    }
    dp[value.len()][pattern.len()]
}

fn patterns_match(value: &str, patterns: &[String]) -> bool {
    patterns.is_empty() || patterns.iter().any(|pattern| glob_match(value, pattern))
}

fn device_matches(device: &Device, selector: &DeviceSelector) -> bool {
    let vendor = selector.vendor.to_lowercase();
    let devices: Vec<String> = selector
        .devices
        .iter()
        .map(|value| value.to_lowercase())
        .collect();
    let classes: Vec<String> = selector
        .classes
        .iter()
        .map(|value| value.to_lowercase())
        .collect();
    (vendor.is_empty() || device.vendor == vendor)
        && (devices.is_empty() || devices.iter().any(|value| value == &device.device))
        && (classes.is_empty()
            || classes
                .iter()
                .any(|value| device.class_code.starts_with(value)))
        && (selector.drivers.is_empty()
            || selector.drivers.iter().any(|value| value == &device.driver))
}

/// Match all selectors in a policy entry against one inventory.
pub fn matches(inventory: &Inventory, selector: &Match) -> bool {
    if selector.always {
        return true;
    }
    if !patterns_match(&inventory.cpu_vendor, &selector.cpu_vendors)
        || !patterns_match(&inventory.dmi_vendor, &selector.dmi_vendors)
        || !patterns_match(&inventory.dmi_product, &selector.dmi_products)
        || !patterns_match(&inventory.dmi_board, &selector.dmi_boards)
    {
        return false;
    }
    for entry in &selector.pci {
        if !inventory
            .pci
            .iter()
            .any(|device| device_matches(device, entry))
        {
            return false;
        }
    }
    for entry in &selector.usb {
        if !inventory
            .usb
            .iter()
            .any(|device| device_matches(device, entry))
        {
            return false;
        }
    }
    !selector.pci.is_empty()
        || !selector.usb.is_empty()
        || !selector.cpu_vendors.is_empty()
        || !selector.dmi_vendors.is_empty()
        || !selector.dmi_products.is_empty()
        || !selector.dmi_boards.is_empty()
}

fn current_date_utc() -> String {
    // Convert Unix days to a proleptic Gregorian date.  This avoids adding a
    // date/time dependency for one warning-only comparison.
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };
    format!("{year:04}-{month:02}-{day:02}")
}

pub fn expired_quirks(policy: &Policy) -> Vec<String> {
    let today = current_date_utc();
    policy
        .quirks
        .iter()
        .filter(|quirk| !quirk.expires_on.is_empty() && quirk.expires_on < today)
        .map(|quirk| quirk.id.clone())
        .collect()
}

pub fn evaluate(policy: &Policy, policy_digest: &str, inventory: Inventory) -> Evaluation {
    let mut profiles: Vec<&Profile> = policy
        .profiles
        .iter()
        .filter(|profile| matches(&inventory, &profile.selector))
        .collect();
    profiles.sort_by_key(|profile| (profile.priority, profile.id.clone()));
    let mut quirks = Vec::new();
    let mut warnings = Vec::new();
    let today = current_date_utc();
    for quirk in &policy.quirks {
        if !matches(&inventory, &quirk.selector) {
            continue;
        }
        let mut value = serde_json::to_value(quirk).unwrap_or(Value::Null);
        let expired = !quirk.expires_on.is_empty() && quirk.expires_on < today;
        if expired {
            warnings.push(format!(
                "quirk {} expired on {} and needs review",
                quirk.id, quirk.expires_on
            ));
        }
        if let Some(object) = value.as_object_mut() {
            object.insert("expired".to_string(), Value::Bool(expired));
        }
        quirks.push(value);
    }
    let capabilities: Vec<String> = profiles
        .iter()
        .flat_map(|profile| profile.capabilities.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let recommended_variant = profiles
        .last()
        .map(|profile| profile.image_variant.clone())
        .filter(|variant| !variant.is_empty())
        .unwrap_or_else(|| "universal".to_string());
    Evaluation {
        policy_revision: policy.policy_revision.clone(),
        policy_digest: policy_digest.to_string(),
        inventory,
        profiles: profiles
            .into_iter()
            .map(|profile| serde_json::to_value(profile).unwrap_or(Value::Null))
            .collect(),
        quirks,
        capabilities,
        recommended_variant,
        warnings,
    }
}

pub fn evaluate_system() -> Result<Evaluation, String> {
    let (policy, digest) = load_policy(Path::new(DEFAULT_POLICY_PATH))?;
    Ok(evaluate(&policy, &digest, collect_inventory()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory(pci: Vec<Device>) -> Inventory {
        Inventory {
            cpu_vendor: "AuthenticAMD".into(),
            dmi_vendor: "Valve".into(),
            dmi_product: "Galileo".into(),
            dmi_board: "Jupiter".into(),
            pci,
            usb: Vec::new(),
        }
    }

    #[test]
    fn device_and_class_selectors_match_the_same_device() {
        let inv = inventory(vec![Device {
            bus: "pci".into(),
            vendor: "1002".into(),
            device: "744c".into(),
            class_code: "030000".into(),
            driver: "amdgpu".into(),
            name: String::new(),
        }]);
        let selector = DeviceSelector {
            vendor: "1002".into(),
            classes: vec!["0300".into()],
            ..Default::default()
        };
        assert!(matches(
            &inv,
            &Match {
                pci: vec![selector],
                ..Default::default()
            }
        ));
    }

    #[test]
    fn evaluation_sorts_profiles_and_unions_capabilities() {
        let policy = Policy {
            schema_version: 1,
            policy_revision: "test".into(),
            variants: Vec::new(),
            profiles: vec![
                Profile {
                    id: "baseline".into(),
                    priority: 0,
                    image_variant: "universal".into(),
                    capabilities: vec!["base".into()],
                    selector: Match {
                        always: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                Profile {
                    id: "gpu".into(),
                    priority: 100,
                    image_variant: "universal".into(),
                    capabilities: vec!["gpu.amd".into()],
                    selector: Match {
                        pci: vec![DeviceSelector {
                            vendor: "1002".into(),
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ],
            quirks: Vec::new(),
        };
        let result = evaluate(
            &policy,
            "digest",
            inventory(vec![Device {
                vendor: "1002".into(),
                ..Default::default()
            }]),
        );
        assert_eq!(result.profiles.len(), 2);
        assert_eq!(result.capabilities, vec!["base", "gpu.amd"]);
    }

    #[test]
    fn parses_the_shipped_policy() {
        let (policy, digest) =
            load_policy(Path::new("../../build_files/config/hardware-profiles.toml")).unwrap();
        assert_eq!(policy.schema_version, 1);
        assert_eq!(digest.len(), 64);
    }
}
