//! Typed execution of the alongside-installation `@home` configuration.

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Deserialize, serde::Serialize)]
pub(crate) struct AlongsideHomeInput {
    pub config_root: String,
    pub target_device: String,
    pub fstab_path: String,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct AlongsideHomeResult {
    pub mounted: bool,
    pub fstab_written: bool,
}

fn safe_path(raw: &str, label: &str) -> Result<PathBuf, String> {
    let value = raw.trim();
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
    Ok(PathBuf::from(value))
}

fn safe_device(raw: &str) -> Result<String, String> {
    let value = raw.trim();
    if !value.starts_with("/dev/")
        || value.contains("..")
        || value.contains("//")
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'.'))
    {
        return Err("target device must be a safe /dev path".into());
    }
    Ok(value.to_owned())
}

pub(crate) fn validate(input: &AlongsideHomeInput) -> Result<(PathBuf, String, PathBuf), String> {
    let root = safe_path(&input.config_root, "config root")?;
    let device = safe_device(&input.target_device)?;
    let fstab = safe_path(&input.fstab_path, "fstab path")?;
    // `Path::ends_with` compares path components and treats a leading `/` in
    // the argument as an anchored path.  That makes a perfectly valid
    // `/mnt/target/etc/fstab` fail the check.  Validate the installed fstab
    // shape component-wise instead, without weakening the absolute-path
    // checks above.
    if fstab.file_name().and_then(|name| name.to_str()) != Some("fstab")
        || fstab
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            != Some("etc")
    {
        return Err("fstab path must point to installed /etc/fstab".into());
    }
    Ok((root, device, fstab))
}

pub(crate) fn apply(input: AlongsideHomeInput) -> Result<AlongsideHomeResult, String> {
    let (root, device, fstab) = validate(&input)?;
    let home = root.join("ostree/deploy/default/var/home");
    if let Ok(metadata) = fs::symlink_metadata(&home) {
        if metadata.file_type().is_symlink() {
            return Err("alongside home path is a symlink".into());
        }
    }
    fs::create_dir_all(&home)
        .map_err(|e| format!("could not create alongside home mountpoint: {e}"))?;
    let _ = Command::new("/usr/bin/umount")
        .args(["-R", "-l"])
        .arg(&home)
        .status();
    let mounted = Command::new("/usr/bin/mount")
        .args(["-o", "subvol=@home"])
        .arg(&device)
        .arg(&home)
        .status()
        .map_err(|e| format!("could not mount alongside home: {e}"))?;
    if !mounted.success() {
        return Err("could not mount alongside home".into());
    }
    let uuid = crate::installer_probe::lookup_uuid(crate::installer_probe::UuidInput { device })?;
    let line = format!("UUID={uuid} /var/home btrfs subvol=@home,compress=zstd:1 0 0\n");
    let fstab_written = match crate::installer_configuration::append_fstab(
        crate::installer_configuration::FstabAppendInput {
            path: fstab.to_string_lossy().into_owned(),
            line,
        },
    ) {
        Ok(()) => true,
        Err(_) => false,
    };
    Ok(AlongsideHomeResult {
        mounted: true,
        fstab_written,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_fixed_alongside_paths() {
        let input = AlongsideHomeInput {
            config_root: "/mnt/target".into(),
            target_device: "/dev/sda3".into(),
            fstab_path: "/mnt/target/etc/fstab".into(),
        };
        let (root, device, fstab) = validate(&input).unwrap();
        assert_eq!(root, Path::new("/mnt/target"));
        assert_eq!(device, "/dev/sda3");
        assert_eq!(fstab, Path::new("/mnt/target/etc/fstab"));
    }
    #[test]
    fn rejects_unsafe_alongside_inputs() {
        let base = AlongsideHomeInput {
            config_root: "/mnt/target".into(),
            target_device: "/dev/sda3".into(),
            fstab_path: "/mnt/target/etc/fstab".into(),
        };
        assert!(validate(&AlongsideHomeInput {
            target_device: "/dev/sda;id".into(),
            ..base
        })
        .is_err());
    }
}
