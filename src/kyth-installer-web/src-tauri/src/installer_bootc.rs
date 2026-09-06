//! Pure bootc install-command planning.
//!
//! This module deliberately constructs an operation description instead of
//! spawning bootc. The root-owned Rust daemon and its typed helper are the
//! only executors; the unprivileged shell can use this plan for preflight.

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct BootcInstallInput {
    pub subcommand: String,
    pub source_imgref: String,
    pub target_imgref: String,
    pub target: String,
    #[serde(default = "default_true")]
    pub skip_fetch_check: bool,
    #[serde(default)]
    pub skip_finalize: bool,
    #[serde(default)]
    pub root_subvolume: bool,
    #[serde(default)]
    pub wipe: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct BootcInstallPlan {
    pub subcommand: String,
    pub argv: Vec<String>,
    pub target: String,
    pub destructive: bool,
    pub requires_network: bool,
    pub executor: &'static str,
}

fn safe_reference(value: &str, label: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 4096 || value.contains("..") {
        return Err(format!("{label} is empty or unsafe."));
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(byte, b'.' | b'/' | b'_' | b'@' | b':' | b'+' | b'-')
    }) {
        return Err(format!("{label} contains unsupported characters."));
    }
    Ok(value.to_string())
}

fn safe_absolute_path(value: &str, label: &str) -> Result<String, String> {
    let value = value.trim();
    if !value.starts_with('/') || value.len() > 4096 || value.contains("..") {
        return Err(format!("{label} must be an absolute safe path."));
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'+' | b':' | b'-')
    }) {
        return Err(format!("{label} contains unsupported characters."));
    }
    Ok(value.to_string())
}

pub(crate) fn build_plan(input: BootcInstallInput) -> Result<BootcInstallPlan, String> {
    let subcommand = input.subcommand.trim().to_ascii_lowercase();
    if !matches!(subcommand.as_str(), "to-disk" | "to-filesystem") {
        return Err(format!(
            "unsupported bootc install subcommand: {subcommand}"
        ));
    }
    let source_imgref = safe_reference(&input.source_imgref, "source image reference")?;
    let target_imgref = safe_reference(&input.target_imgref, "target image reference")?;
    let target = if subcommand == "to-disk" {
        crate::installer_plan::normalize_device_path(&input.target)
            .ok_or_else(|| "bootc disk target must be a safe device path.".to_string())?
    } else {
        safe_absolute_path(&input.target, "bootc filesystem target")?
    };

    let mut argv = vec![
        "bootc".to_string(),
        "install".to_string(),
        subcommand.clone(),
        "--source-imgref".to_string(),
        source_imgref.clone(),
        "--target-imgref".to_string(),
        target_imgref,
    ];
    if subcommand == "to-filesystem" {
        argv.push("--acknowledge-destructive".to_string());
        if input.skip_finalize {
            argv.push("--skip-finalize".to_string());
        }
        if input.root_subvolume {
            argv.push("--karg=rootflags=subvol=@".to_string());
        }
    } else {
        argv.extend(["--filesystem".to_string(), "btrfs".to_string()]);
        if input.wipe {
            argv.push("--wipe".to_string());
        }
    }
    if input.skip_fetch_check && !argv.iter().any(|arg| arg == "--skip-fetch-check") {
        argv.push("--skip-fetch-check".to_string());
    }
    argv.push(target.clone());

    Ok(BootcInstallPlan {
        subcommand,
        argv,
        target,
        destructive: true,
        // --skip-fetch-check bypasses the reachability preflight only; a
        // docker:// source still needs the network when bootc executes.
        requires_network: source_imgref.starts_with("docker://"),
        executor: "kyth-installerd",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(subcommand: &str) -> BootcInstallInput {
        BootcInstallInput {
            subcommand: subcommand.to_string(),
            source_imgref: "docker://ghcr.io/kyth-os/kyth:latest".to_string(),
            target_imgref: "ghcr.io/kyth-os/kyth:latest".to_string(),
            target: "/dev/sda".to_string(),
            skip_fetch_check: true,
            skip_finalize: false,
            root_subvolume: false,
            wipe: false,
        }
    }

    #[test]
    fn builds_disk_install_without_arbitrary_flags() {
        let plan = build_plan(BootcInstallInput {
            wipe: true,
            ..input("to-disk")
        })
        .expect("disk plan should validate");
        assert_eq!(plan.argv[..3], ["bootc", "install", "to-disk"]);
        assert!(plan
            .argv
            .windows(2)
            .any(|pair| pair == ["--filesystem", "btrfs"]));
        assert!(plan.argv.iter().any(|arg| arg == "--wipe"));
        assert!(plan.requires_network);
    }

    #[test]
    fn builds_filesystem_install_with_safe_bootc_flags() {
        let plan = build_plan(BootcInstallInput {
            target: "/mnt/kyth".to_string(),
            skip_fetch_check: false,
            skip_finalize: true,
            root_subvolume: true,
            ..input("to-filesystem")
        })
        .expect("filesystem plan should validate");
        assert!(plan
            .argv
            .iter()
            .any(|arg| arg == "--acknowledge-destructive"));
        assert!(plan.argv.iter().any(|arg| arg == "--skip-finalize"));
        assert!(plan
            .argv
            .iter()
            .any(|arg| arg == "--karg=rootflags=subvol=@"));
        assert!(plan.requires_network);
    }

    #[test]
    fn rejects_unsafe_targets_and_references() {
        for target in ["../../etc", "relative", "/mnt/with space"] {
            let error = build_plan(BootcInstallInput {
                target: target.to_string(),
                ..input("to-filesystem")
            })
            .expect_err("unsafe target must fail");
            assert!(error.contains("target"), "{error}");
        }
        let error = build_plan(BootcInstallInput {
            source_imgref: "docker://example/$(touch /tmp/pwned)".to_string(),
            ..input("to-disk")
        })
        .expect_err("unsafe image reference must fail");
        assert!(error.contains("image reference"));
    }
}
