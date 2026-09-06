//! Pure installer request normalization and plan projection.
//!
//! The native daemon validates request shape and mode-specific scalar
//! invariants before the native executor re-scans storage and repeats safety
//! checks immediately before mutation.

use serde::{Deserialize, Serialize};

const MIN_KYTHOS_GIB: i64 = 32;
const BYTES_PER_GIB: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct InstallerPlanInput {
    #[serde(default)]
    pub disk: String,
    #[serde(default)]
    pub install_mode: String,
    #[serde(default)]
    pub target_partition: String,
    #[serde(default)]
    pub resize_partition: String,
    #[serde(default)]
    pub resize_gib: i64,
    #[serde(default)]
    pub free_region_start: i64,
    #[serde(default)]
    pub free_region_end: i64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct InstallerPlan {
    pub mode: String,
    pub disk: String,
    pub target_partition: Option<String>,
    pub resize_partition: Option<String>,
    pub resize_bytes: u64,
    pub free_region_start: Option<u64>,
    pub free_region_end: Option<u64>,
}

pub(crate) fn normalize_device_path(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    let value = if value.starts_with("/dev/") {
        value.to_string()
    } else {
        format!("/dev/{value}")
    };
    if !value.starts_with("/dev/")
        || value.len() > 4096
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'+' | b':' | b'-')
        })
    {
        return None;
    }
    Some(value)
}

fn required_device(raw: &str, message: &str) -> Result<String, String> {
    normalize_device_path(raw).ok_or_else(|| message.to_string())
}

/// Normalize a frontend request into the storage portion consumed by planning.
pub fn build_plan(input: InstallerPlanInput) -> Result<InstallerPlan, String> {
    let mode = {
        let value = input.install_mode.trim().to_ascii_lowercase();
        if value.is_empty() {
            "wipe".to_string()
        } else {
            value
        }
    };
    if !matches!(
        mode.as_str(),
        "wipe" | "resize_ntfs" | "alongside" | "free_space" | "manual"
    ) {
        return Err(format!("Unsupported install mode: {mode}"));
    }

    let disk = required_device(&input.disk, "No target disk was selected.")?;
    let target_partition = match mode.as_str() {
        "alongside" => Some(required_device(
            &input.target_partition,
            "No target partition was selected for alongside installation.",
        )?),
        "manual" => normalize_device_path(&input.target_partition),
        _ => None,
    };

    let (resize_partition, resize_bytes) = if mode == "resize_ntfs" {
        let partition = if input.resize_partition.trim().is_empty() {
            &input.target_partition
        } else {
            &input.resize_partition
        };
        if input.resize_gib < MIN_KYTHOS_GIB {
            return Err(format!(
                "NTFS shrink install requires at least {MIN_KYTHOS_GIB} GiB for KythOS."
            ));
        }
        (
            Some(required_device(
                partition,
                "No NTFS partition was selected to shrink.",
            )?),
            (input.resize_gib as u64) * BYTES_PER_GIB,
        )
    } else {
        (None, 0)
    };

    let (free_region_start, free_region_end) = if mode == "free_space" {
        if input.free_region_start < 0 || input.free_region_end <= input.free_region_start {
            return Err("No free space region was selected for installation.".to_string());
        }
        let start = input.free_region_start as u64;
        let end = input.free_region_end as u64;
        if end - start < (MIN_KYTHOS_GIB as u64) * BYTES_PER_GIB {
            return Err(format!(
                "Free space install requires at least {MIN_KYTHOS_GIB} GiB for KythOS."
            ));
        }
        (Some(start), Some(end))
    } else {
        (None, None)
    };

    Ok(InstallerPlan {
        mode,
        disk,
        target_partition,
        resize_partition,
        resize_bytes,
        free_region_start,
        free_region_end,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn input() -> InstallerPlanInput {
        InstallerPlanInput {
            disk: "sda".to_string(),
            install_mode: " WIPE ".to_string(),
            target_partition: String::new(),
            resize_partition: String::new(),
            resize_gib: 0,
            free_region_start: 0,
            free_region_end: 0,
        }
    }

    #[test]
    fn normalizes_mode_and_device_path() {
        let plan = build_plan(input()).expect("wipe request should validate");
        assert_eq!(plan.mode, "wipe");
        assert_eq!(plan.disk, "/dev/sda");
        assert_eq!(plan.target_partition, None);
    }

    #[test]
    fn projects_alongside_target() {
        let plan = build_plan(InstallerPlanInput {
            install_mode: "alongside".to_string(),
            disk: "/dev/nvme0n1".to_string(),
            target_partition: "nvme0n1p5".to_string(),
            ..input()
        })
        .expect("alongside request should validate");
        assert_eq!(plan.target_partition.as_deref(), Some("/dev/nvme0n1p5"));
    }

    #[test]
    fn resize_uses_target_partition_fallback() {
        let plan = build_plan(InstallerPlanInput {
            install_mode: "resize_ntfs".to_string(),
            disk: "/dev/sda".to_string(),
            target_partition: "/dev/sda2".to_string(),
            resize_gib: 40,
            ..input()
        })
        .expect("resize request should validate");
        assert_eq!(plan.resize_partition.as_deref(), Some("/dev/sda2"));
        assert_eq!(plan.resize_bytes, 40 * BYTES_PER_GIB);
    }

    #[test]
    fn rejects_invalid_modes_paths_and_sizes() {
        let cases = [
            (
                InstallerPlanInput {
                    install_mode: "other".to_string(),
                    ..input()
                },
                "Unsupported",
            ),
            (
                InstallerPlanInput {
                    disk: "../../etc/passwd".to_string(),
                    ..input()
                },
                "target disk",
            ),
            (
                InstallerPlanInput {
                    install_mode: "alongside".to_string(),
                    ..input()
                },
                "target partition",
            ),
            (
                InstallerPlanInput {
                    install_mode: "resize_ntfs".to_string(),
                    resize_gib: 1,
                    ..input()
                },
                "at least",
            ),
            (
                InstallerPlanInput {
                    install_mode: "free_space".to_string(),
                    disk: "/dev/sda".to_string(),
                    free_region_start: 10,
                    free_region_end: 5,
                    ..input()
                },
                "free space",
            ),
        ];
        for (request, message) in cases {
            let error = build_plan(request).expect_err("invalid request must fail");
            assert!(
                error.contains(message),
                "{error:?} did not contain {message:?}"
            );
        }
    }

    #[test]
    fn shared_parity_fixture_matches_rust_projection_and_errors() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("../testdata/installer_plan_cases.json"))
                .expect("installer plan parity fixture must be valid JSON");
        for case in cases {
            let name = case["name"].as_str().expect("fixture case needs a name");
            let input: InstallerPlanInput = serde_json::from_value(case["input"].clone())
                .unwrap_or_else(|error| panic!("{name}: invalid input: {error}"));
            let result = build_plan(input);
            if let Some(expected) = case.get("expected") {
                let plan = result.unwrap_or_else(|error| panic!("{name}: {error}"));
                assert_eq!(
                    serde_json::to_value(plan).expect("plan serializes"),
                    *expected,
                    "{name}"
                );
            } else {
                let error = result.expect_err("invalid fixture case must fail");
                assert!(
                    error.contains(case["error_contains"].as_str().unwrap()),
                    "{name}: {error}"
                );
            }
        }
    }
}
