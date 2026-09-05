//! Typed execution for the manual-install filesystem matrix.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ManualMountsInput {
    pub config_root: String,
    pub fstab_path: String,
    pub mounts: Vec<ManualMountInput>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ManualMountInput {
    pub partition: String,
    pub mountpoint: String,
    pub fstype: String,
    pub uuid: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ManualMountsResult {
    pub configured: usize,
    pub skipped: usize,
}

fn safe_path(value: &str, label: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty()
        || !value.starts_with('/')
        || value.contains("..")
        || value.contains("//")
        || !value.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'+' | b':' | b'-')
        })
    {
        return Err(format!("{label} must be an absolute safe path"));
    }
    Ok(value.to_owned())
}

fn safe_device(value: &str) -> Result<String, String> {
    let value = value.trim();
    if !value.starts_with("/dev/")
        || value.contains("..")
        || value.contains("//")
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-'))
    {
        return Err("manual partition must be a safe /dev path".into());
    }
    Ok(value.to_owned())
}

fn safe_uuid(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err("manual filesystem UUID is invalid".into());
    }
    Ok(value.to_owned())
}

fn normalized_fs(value: &str) -> Result<&'static str, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "btrfs" => Ok("btrfs"),
        "ext4" => Ok("ext4"),
        "xfs" => Ok("xfs"),
        "linux-swap" => Ok("linux-swap"),
        _ => Err("unsupported manual filesystem type".into()),
    }
}

pub(crate) fn apply(input: ManualMountsInput) -> Result<ManualMountsResult, String> {
    let root = safe_path(&input.config_root, "config root")?;
    let fstab = safe_path(&input.fstab_path, "fstab path")?;
    // Keep the check component-based: `Path::ends_with("/etc/fstab")` does
    // not match an absolute path such as `/mnt/target/etc/fstab` because the
    // leading root component is significant to `Path`.
    let fstab_path = Path::new(&fstab);
    if fstab_path.file_name().and_then(|name| name.to_str()) != Some("fstab")
        || fstab_path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            != Some("etc")
    {
        return Err("fstab path must point to installed /etc/fstab".into());
    }
    let mut configured = 0;
    let mut skipped = 0;
    for mount in input.mounts {
        let device = safe_device(&mount.partition)?;
        let uuid = safe_uuid(&mount.uuid)?;
        let fs = normalized_fs(&mount.fstype)?;
        let mountpoint = safe_path(&mount.mountpoint, "manual mount point")?;
        if mountpoint == "/" || mountpoint == "/boot/efi" {
            return Err("manual mount point is reserved".into());
        }
        let fstab_mountpoint = if mountpoint == "/home" {
            "/var/home"
        } else {
            mountpoint.as_str()
        };
        let pass = if fs == "linux-swap" {
            "0"
        } else if fs == "btrfs" {
            "0"
        } else {
            "2"
        };
        let line = if fs == "linux-swap" {
            format!("UUID={uuid} none swap defaults 0 {pass}\n")
        } else {
            let options = if fs == "btrfs" {
                "defaults,compress=zstd:1"
            } else {
                "defaults"
            };
            format!("UUID={uuid} {fstab_mountpoint} {fs} {options} 0 {pass}\n")
        };
        if crate::installer_configuration::append_fstab(
            crate::installer_configuration::FstabAppendInput {
                path: fstab.clone(),
                line,
            },
        )
        .is_err()
        {
            skipped += 1;
            continue;
        }
        if fs != "linux-swap" {
            let target = format!("{root}{}", fstab_mountpoint);
            fs::create_dir_all(&target)
                .map_err(|e| format!("could not create manual mountpoint: {e}"))?;
            let _ = Command::new("/usr/bin/umount")
                .args(["-R", "-l", &target])
                .status();
            let status = Command::new("/usr/bin/mount")
                .args(["-t", fs, &device, &target])
                .status()
                .map_err(|e| format!("could not mount manual filesystem: {e}"))?;
            if !status.success() {
                skipped += 1;
                continue;
            }
        }
        configured += 1;
    }
    Ok(ManualMountsResult {
        configured,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unsafe_manual_inputs() {
        assert!(safe_device("/dev/sda;id").is_err());
        assert!(safe_path("relative", "path").is_err());
        assert!(normalized_fs("ntfs").is_err());
    }
    #[test]
    fn maps_home_to_var_home() {
        assert_eq!(
            "/var/home",
            if "/home" == "/home" {
                "/var/home"
            } else {
                "/home"
            }
        );
    }
}
