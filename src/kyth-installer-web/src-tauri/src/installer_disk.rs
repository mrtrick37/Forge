//! Typed, root-only execution plans for non-interactive disk operations.
//!
//! The native executor chooses when an operation is needed but never accepts
//! caller-provided argv. Every accepted request maps to one fixed executable
//! and one fixed argv shape.

use serde::Deserialize;

use crate::installer_plan::normalize_device_path;

const MAX_LABEL_BYTES: usize = 128;
const MAX_PATH_BYTES: usize = 4096;
const DEFAULT_SECTOR_SIZE: u64 = 512;

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub(crate) enum DiskOperationInput {
    BackupTable {
        disk: String,
        backup_path: String,
    },
    RestoreTable {
        disk: String,
        backup_path: String,
    },
    CreateLabel {
        disk: String,
        table_type: String,
    },
    CreatePartition {
        disk: String,
        start: u64,
        size: u64,
        fs: String,
        label: String,
        #[serde(default = "default_sector_size")]
        sector_size: u64,
    },
    CreateUnformattedPartition {
        disk: String,
        start: u64,
        size: u64,
        label: String,
        #[serde(default = "default_sector_size")]
        sector_size: u64,
    },
    DeletePartition {
        disk: String,
        part_num: u32,
    },
    ResizePartition {
        disk: String,
        part_num: u32,
        start: u64,
        new_size: u64,
        #[serde(default = "default_sector_size")]
        sector_size: u64,
    },
    FilesystemCheck {
        device: String,
    },
    FilesystemResize {
        device: String,
        fs: String,
        new_size_bytes: u64,
        stage: String,
    },
    MountFilesystem {
        device: String,
        mountpoint: String,
        #[serde(default)]
        options: Vec<String>,
        #[serde(default)]
        bind: bool,
    },
    UnmountFilesystem {
        mountpoint: String,
        #[serde(default)]
        recursive: bool,
        #[serde(default)]
        lazy: bool,
    },
    SetPartitionFlag {
        disk: String,
        part_num: u32,
        flag: String,
        #[serde(default = "default_true")]
        enabled: bool,
    },
    FormatFilesystem {
        device: String,
        fs: String,
        label: String,
    },
    BtrfsSubvolumeCreate {
        mountpoint: String,
        name: String,
    },
    BtrfsSubvolumeSetDefault {
        mountpoint: String,
        name: String,
    },
    EnsureDirectory {
        path: String,
    },
}

impl DiskOperationInput {
    pub(crate) fn backup_path(&self) -> Option<&str> {
        match self {
            Self::BackupTable { backup_path, .. } => Some(backup_path),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DiskPlan {
    pub(crate) argv: Vec<String>,
    pub(crate) timeout_seconds: u64,
    pub(crate) needs_confirmation: bool,
}

/// Make a completed partition-table backup durable before the caller can
/// mutate the disk. This belongs next to the root-only backup operation so a
/// compatibility caller cannot accidentally persist an unsynced snapshot.
pub(crate) fn sync_backup(path: &str) -> Result<(), String> {
    let file = std::fs::File::open(path)
        .map_err(|error| format!("could not open partition backup for syncing: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("could not sync partition backup: {error}"))?;
    let parent = std::path::Path::new(path)
        .parent()
        .ok_or_else(|| "partition backup has no parent directory".to_string())?;
    let directory = std::fs::File::open(parent)
        .map_err(|error| format!("could not open partition backup directory: {error}"))?;
    directory
        .sync_all()
        .map_err(|error| format!("could not sync partition backup directory: {error}"))
}

fn default_sector_size() -> u64 {
    DEFAULT_SECTOR_SIZE
}

fn default_true() -> bool {
    true
}

fn required_device(raw: &str, label: &str) -> Result<String, String> {
    let device = normalize_device_path(raw)
        .filter(|value| value.len() > "/dev/".len() && !value.contains("//"))
        .ok_or_else(|| format!("{label} must be a safe device path."))?;
    Ok(device)
}

fn safe_absolute_path(raw: &str, label: &str) -> Result<String, String> {
    let value = raw.trim();
    if value.is_empty()
        || value.len() > MAX_PATH_BYTES
        || !value.starts_with('/')
        || value.contains("..")
        || value.contains("//")
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'+' | b':' | b'-')
        })
    {
        return Err(format!("{label} must be an absolute safe path."));
    }
    Ok(value.to_string())
}

fn safe_mountpoint(raw: &str) -> Result<String, String> {
    safe_absolute_path(raw, "mount point")
}

fn safe_btrfs_subvolume_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if !matches!(name, "@" | "@home") {
        return Err("unsupported Btrfs subvolume name".to_string());
    }
    Ok(name.to_string())
}

fn safe_mount_options(options: &[String]) -> Result<Vec<String>, String> {
    if options.len() > 8 {
        return Err("mount options are too numerous".to_string());
    }
    options
        .iter()
        .map(|option| {
            if option.is_empty()
                || option.len() > 128
                || !option.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(byte, b'=' | b':' | b'.' | b'_' | b'+' | b'@' | b'-')
                })
            {
                return Err("mount options contain unsupported characters".to_string());
            }
            Ok(option.clone())
        })
        .collect()
}

fn safe_label(raw: String) -> Result<String, String> {
    if raw.len() > MAX_LABEL_BYTES || raw.chars().any(char::is_control) {
        return Err("partition label is empty or contains unsafe characters.".to_string());
    }
    Ok(raw)
}

fn normalized_name(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

fn partition_end(start: u64, size: u64, sector_size: u64) -> Result<u64, String> {
    if sector_size == 0 || !sector_size.is_power_of_two() || !(512..=4096).contains(&sector_size) {
        return Err("partition sector size is unsupported.".to_string());
    }
    if start == 0 || size < sector_size {
        return Err("partition start or size is invalid.".to_string());
    }
    if start % sector_size != 0 || size % sector_size != 0 {
        return Err("partition geometry is not aligned to the device sector size.".to_string());
    }
    start
        .checked_add(size - sector_size)
        .ok_or_else(|| "partition geometry overflows.".to_string())
}

fn parted_device(
    disk: String,
    args: Vec<String>,
    timeout_seconds: u64,
) -> Result<DiskPlan, String> {
    let disk = required_device(&disk, "disk")?;
    Ok(DiskPlan {
        argv: std::iter::once("/usr/sbin/parted".to_string())
            .chain(std::iter::once("-s".to_string()))
            .chain(std::iter::once(disk))
            .chain(args)
            .collect(),
        timeout_seconds,
        needs_confirmation: false,
    })
}

fn interactive_parted_device(
    disk: String,
    args: Vec<String>,
    timeout_seconds: u64,
) -> Result<DiskPlan, String> {
    let disk = required_device(&disk, "disk")?;
    Ok(DiskPlan {
        argv: std::iter::once("/usr/sbin/parted".to_string())
            .chain(std::iter::once("---pretend-input-tty".to_string()))
            .chain(std::iter::once(disk))
            .chain(args)
            .collect(),
        timeout_seconds,
        needs_confirmation: true,
    })
}

fn build_mkfs(device: String, fs: String, label: String) -> Result<DiskPlan, String> {
    let device = required_device(&device, "filesystem device")?;
    let fs = normalized_name(&fs);
    let label = safe_label(label)?;
    let (binary, mut args): (&str, Vec<String>) = match fs.as_str() {
        "btrfs" => ("/usr/sbin/mkfs.btrfs", vec!["-f".to_string()]),
        "ext4" => ("/usr/sbin/mkfs.ext4", vec!["-F".to_string()]),
        "xfs" => ("/usr/sbin/mkfs.xfs", vec!["-f".to_string()]),
        "fat32" => ("/usr/sbin/mkfs.fat", vec!["-F32".to_string()]),
        "linux-swap" => ("/usr/sbin/mkswap", Vec::new()),
        _ => return Err(format!("unsupported filesystem type: {fs}")),
    };
    if !label.is_empty() {
        args.extend([if fs == "fat32" { "-n" } else { "-L" }.to_string(), label]);
    }
    args.push(device);
    Ok(DiskPlan {
        argv: std::iter::once(binary.to_string()).chain(args).collect(),
        timeout_seconds: if fs == "btrfs" { 300 } else { 120 },
        needs_confirmation: false,
    })
}

fn build_filesystem_resize(
    device: String,
    fs: String,
    new_size_bytes: u64,
    stage: String,
) -> Result<DiskPlan, String> {
    let device = required_device(&device, "filesystem device")?;
    if new_size_bytes == 0 {
        return Err("filesystem resize size must be positive.".to_string());
    }
    let fs = normalized_name(&fs);
    let stage = normalized_name(&stage);
    match (fs.as_str(), stage.as_str()) {
        ("ntfs" | "ntfs3", "check") => Ok(DiskPlan {
            argv: vec![
                "/usr/sbin/ntfsresize".to_string(),
                "--check".to_string(),
                device,
            ],
            timeout_seconds: 240,
            needs_confirmation: false,
        }),
        ("ntfs" | "ntfs3", "info") => Ok(DiskPlan {
            argv: vec![
                "/usr/sbin/ntfsresize".to_string(),
                "--info".to_string(),
                device,
            ],
            timeout_seconds: 120,
            needs_confirmation: false,
        }),
        ("ntfs" | "ntfs3", "dry_run") => Ok(DiskPlan {
            argv: vec![
                "/usr/sbin/ntfsresize".to_string(),
                "--no-action".to_string(),
                "--size".to_string(),
                new_size_bytes.to_string(),
                device,
            ],
            timeout_seconds: 240,
            needs_confirmation: false,
        }),
        ("ntfs" | "ntfs3", "resize") => Ok(DiskPlan {
            argv: vec![
                "/usr/sbin/ntfsresize".to_string(),
                "--size".to_string(),
                new_size_bytes.to_string(),
                device,
            ],
            timeout_seconds: 1800,
            needs_confirmation: true,
        }),
        ("ext2" | "ext3" | "ext4", "resize") => {
            let size_kib = std::cmp::max(1, new_size_bytes / 1024);
            Ok(DiskPlan {
                argv: vec![
                    "/usr/sbin/resize2fs".to_string(),
                    device,
                    format!("{size_kib}K"),
                ],
                timeout_seconds: 1800,
                needs_confirmation: false,
            })
        }
        ("btrfs", "resize") => Ok(DiskPlan {
            argv: vec![
                "/usr/sbin/btrfs".to_string(),
                "filesystem".to_string(),
                "resize".to_string(),
                new_size_bytes.to_string(),
                device,
            ],
            timeout_seconds: 1800,
            needs_confirmation: false,
        }),
        _ => Err(format!(
            "unsupported filesystem resize operation: {fs}/{stage}"
        )),
    }
}

pub(crate) fn build_plan(input: DiskOperationInput) -> Result<DiskPlan, String> {
    match input {
        DiskOperationInput::BackupTable { disk, backup_path } => Ok(DiskPlan {
            argv: vec![
                "/usr/sbin/sgdisk".to_string(),
                "--backup".to_string(),
                safe_absolute_path(&backup_path, "backup path")?,
                required_device(&disk, "disk")?,
            ],
            timeout_seconds: 30,
            needs_confirmation: false,
        }),
        DiskOperationInput::RestoreTable { disk, backup_path } => Ok(DiskPlan {
            argv: vec![
                "/usr/sbin/sgdisk".to_string(),
                "--load-backup".to_string(),
                safe_absolute_path(&backup_path, "backup path")?,
                required_device(&disk, "disk")?,
            ],
            timeout_seconds: 60,
            needs_confirmation: false,
        }),
        DiskOperationInput::CreateLabel { disk, table_type } => {
            let table_type = normalized_name(&table_type);
            if !matches!(table_type.as_str(), "gpt" | "msdos") {
                return Err(format!("unsupported partition table type: {table_type}"));
            }
            parted_device(disk, vec!["mklabel".to_string(), table_type], 30)
        }
        DiskOperationInput::CreatePartition {
            disk,
            start,
            size,
            fs,
            label,
            sector_size,
        } => {
            let fs = normalized_name(&fs);
            if !matches!(
                fs.as_str(),
                "btrfs" | "ext4" | "xfs" | "fat32" | "linux-swap"
            ) {
                return Err(format!("unsupported filesystem type: {fs}"));
            }
            let label = safe_label(label)?;
            let end = partition_end(start, size, sector_size)?;
            parted_device(
                disk,
                vec![
                    "unit".to_string(),
                    "B".to_string(),
                    "mkpart".to_string(),
                    if label.is_empty() {
                        "partition".to_string()
                    } else {
                        label
                    },
                    fs,
                    format!("{start}B"),
                    format!("{end}B"),
                ],
                120,
            )
        }
        DiskOperationInput::CreateUnformattedPartition {
            disk,
            start,
            size,
            label,
            sector_size,
        } => {
            let end = partition_end(start, size, sector_size)?;
            parted_device(
                disk,
                vec![
                    "unit".to_string(),
                    "B".to_string(),
                    "mkpart".to_string(),
                    safe_label(label)?,
                    format!("{start}B"),
                    format!("{end}B"),
                ],
                120,
            )
        }
        DiskOperationInput::DeletePartition { disk, part_num } => {
            if part_num == 0 {
                return Err("partition number must be positive.".to_string());
            }
            parted_device(disk, vec!["rm".to_string(), part_num.to_string()], 60)
        }
        DiskOperationInput::ResizePartition {
            disk,
            part_num,
            start,
            new_size,
            sector_size,
        } => {
            if part_num == 0 {
                return Err("partition number must be positive.".to_string());
            }
            let end = partition_end(start, new_size, sector_size)?;
            interactive_parted_device(
                disk,
                vec![
                    "unit".to_string(),
                    "B".to_string(),
                    "resizepart".to_string(),
                    part_num.to_string(),
                    format!("{end}B"),
                ],
                120,
            )
        }
        DiskOperationInput::FilesystemCheck { device } => {
            let device = required_device(&device, "filesystem device")?;
            Ok(DiskPlan {
                argv: vec![
                    "/usr/sbin/e2fsck".to_string(),
                    "-f".to_string(),
                    "-y".to_string(),
                    device,
                ],
                timeout_seconds: 600,
                needs_confirmation: false,
            })
        }
        DiskOperationInput::FilesystemResize {
            device,
            fs,
            new_size_bytes,
            stage,
        } => build_filesystem_resize(device, fs, new_size_bytes, stage),
        DiskOperationInput::MountFilesystem {
            device,
            mountpoint,
            options,
            bind,
        } => {
            let device = if bind {
                safe_absolute_path(&device, "mount source")?
            } else {
                required_device(&device, "filesystem device")?
            };
            let mountpoint = safe_mountpoint(&mountpoint)?;
            let options = safe_mount_options(&options)?;
            let mut argv = vec!["/usr/sbin/mount".to_string()];
            if bind {
                argv.push("--bind".to_string());
            } else if !options.is_empty() {
                argv.extend(["-o".to_string(), options.join(",")]);
            }
            argv.extend([device, mountpoint]);
            Ok(DiskPlan {
                argv,
                timeout_seconds: 30,
                needs_confirmation: false,
            })
        }
        DiskOperationInput::UnmountFilesystem {
            mountpoint,
            recursive,
            lazy,
        } => {
            let mountpoint = safe_mountpoint(&mountpoint)?;
            let mut argv = vec!["/usr/sbin/umount".to_string()];
            if recursive {
                argv.push("-R".to_string());
            }
            if lazy {
                argv.push("-l".to_string());
            }
            argv.push(mountpoint);
            Ok(DiskPlan {
                argv,
                timeout_seconds: 30,
                needs_confirmation: false,
            })
        }
        DiskOperationInput::SetPartitionFlag {
            disk,
            part_num,
            flag,
            enabled,
        } => {
            if part_num == 0 {
                return Err("partition number must be positive.".to_string());
            }
            let flag = normalized_name(&flag);
            if !matches!(flag.as_str(), "bios_grub" | "esp") {
                return Err(format!("unsupported partition flag: {flag}"));
            }
            parted_device(
                disk,
                vec![
                    "set".to_string(),
                    part_num.to_string(),
                    flag,
                    if enabled { "on" } else { "off" }.to_string(),
                ],
                60,
            )
        }
        DiskOperationInput::FormatFilesystem { device, fs, label } => build_mkfs(device, fs, label),
        DiskOperationInput::BtrfsSubvolumeCreate { mountpoint, name } => {
            let mountpoint = safe_mountpoint(&mountpoint)?;
            let name = safe_btrfs_subvolume_name(&name)?;
            Ok(DiskPlan {
                argv: vec![
                    "/usr/sbin/btrfs".to_string(),
                    "subvolume".to_string(),
                    "create".to_string(),
                    format!("{mountpoint}/{name}"),
                ],
                timeout_seconds: 60,
                needs_confirmation: false,
            })
        }
        DiskOperationInput::BtrfsSubvolumeSetDefault { mountpoint, name } => {
            let mountpoint = safe_mountpoint(&mountpoint)?;
            let name = safe_btrfs_subvolume_name(&name)?;
            Ok(DiskPlan {
                argv: vec![
                    "/usr/sbin/btrfs".to_string(),
                    "subvolume".to_string(),
                    "set-default".to_string(),
                    format!("{mountpoint}/{name}"),
                ],
                timeout_seconds: 60,
                needs_confirmation: false,
            })
        }
        DiskOperationInput::EnsureDirectory { path } => Ok(DiskPlan {
            argv: vec![
                "/usr/bin/mkdir".to_string(),
                "-p".to_string(),
                safe_absolute_path(&path, "directory path")?,
            ],
            timeout_seconds: 30,
            needs_confirmation: false,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device() -> String {
        "/dev/sda".to_string()
    }

    #[test]
    fn projects_fixed_partition_commands() {
        let plan = build_plan(DiskOperationInput::CreatePartition {
            disk: device(),
            start: 1024 * 1024,
            size: 1024 * 1024 * 1024,
            fs: "btrfs".into(),
            label: "KythOS".into(),
            sector_size: 512,
        })
        .expect("partition plan should validate");
        assert_eq!(
            plan.argv,
            [
                "/usr/sbin/parted",
                "-s",
                "/dev/sda",
                "unit",
                "B",
                "mkpart",
                "KythOS",
                "btrfs",
                "1048576B",
                "1074789888B",
            ]
        );
    }

    #[test]
    fn projects_filesystem_commands_with_type_specific_labels() {
        let plan = build_plan(DiskOperationInput::FormatFilesystem {
            device: device(),
            fs: "fat32".into(),
            label: "EFI".into(),
        })
        .expect("filesystem plan should validate");
        assert_eq!(
            plan.argv,
            ["/usr/sbin/mkfs.fat", "-F32", "-n", "EFI", "/dev/sda"]
        );
    }

    #[test]
    fn projects_table_and_flag_operations() {
        let label = build_plan(DiskOperationInput::CreateLabel {
            disk: device(),
            table_type: "GPT".into(),
        })
        .expect("label plan should validate");
        assert_eq!(
            label.argv,
            ["/usr/sbin/parted", "-s", "/dev/sda", "mklabel", "gpt"]
        );

        let bios = build_plan(DiskOperationInput::CreateUnformattedPartition {
            disk: device(),
            start: 1024 * 1024,
            size: 1024 * 1024,
            label: "biosboot".into(),
            sector_size: 512,
        })
        .expect("unformatted partition plan should validate");
        assert_eq!(
            bios.argv,
            [
                "/usr/sbin/parted",
                "-s",
                "/dev/sda",
                "unit",
                "B",
                "mkpart",
                "biosboot",
                "1048576B",
                "2096640B",
            ]
        );

        let delete = build_plan(DiskOperationInput::DeletePartition {
            disk: device(),
            part_num: 2,
        })
        .expect("delete plan should validate");
        assert_eq!(
            delete.argv,
            ["/usr/sbin/parted", "-s", "/dev/sda", "rm", "2"]
        );

        let flag = build_plan(DiskOperationInput::SetPartitionFlag {
            disk: device(),
            part_num: 1,
            flag: "esp".into(),
            enabled: false,
        })
        .expect("flag plan should validate");
        assert_eq!(
            flag.argv,
            [
                "/usr/sbin/parted",
                "-s",
                "/dev/sda",
                "set",
                "1",
                "esp",
                "off"
            ]
        );
    }

    #[test]
    fn projects_backup_and_restore_operations() {
        let backup = build_plan(DiskOperationInput::BackupTable {
            disk: device(),
            backup_path: "/tmp/table.backup".into(),
        })
        .expect("backup plan should validate");
        assert_eq!(
            backup.argv,
            [
                "/usr/sbin/sgdisk",
                "--backup",
                "/tmp/table.backup",
                "/dev/sda"
            ]
        );

        let restore = build_plan(DiskOperationInput::RestoreTable {
            disk: device(),
            backup_path: "/tmp/table.backup".into(),
        })
        .expect("restore plan should validate");
        assert_eq!(
            restore.argv,
            [
                "/usr/sbin/sgdisk",
                "--load-backup",
                "/tmp/table.backup",
                "/dev/sda",
            ]
        );
    }

    #[test]
    fn syncs_backup_file_and_parent_directory() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("table.backup");
        std::fs::write(&path, b"partition table").expect("backup file");
        sync_backup(path.to_str().expect("UTF-8 temporary path"))
            .expect("backup and directory should sync");
    }

    #[test]
    fn rejects_missing_backup_when_syncing() {
        let error = sync_backup("/tmp/kyth-missing-partition-backup")
            .expect_err("missing backup must fail closed");
        assert!(error.contains("open partition backup"));
    }

    #[test]
    fn projects_interactive_resize_with_fixed_confirmation() {
        let plan = build_plan(DiskOperationInput::ResizePartition {
            disk: device(),
            part_num: 3,
            start: 128 * 1024 * 1024,
            new_size: 64 * 1024 * 1024,
            sector_size: 512,
        })
        .expect("resize plan should validate");
        assert_eq!(
            plan.argv,
            [
                "/usr/sbin/parted",
                "---pretend-input-tty",
                "/dev/sda",
                "unit",
                "B",
                "resizepart",
                "3",
                "201326080B",
            ]
        );
        assert!(plan.needs_confirmation);
    }

    #[test]
    fn projects_filesystem_shrink_stages() {
        let check = build_plan(DiskOperationInput::FilesystemCheck { device: device() })
            .expect("filesystem check plan should validate");
        assert_eq!(check.argv, ["/usr/sbin/e2fsck", "-f", "-y", "/dev/sda"]);

        let ntfs = build_plan(DiskOperationInput::FilesystemResize {
            device: device(),
            fs: "ntfs".into(),
            new_size_bytes: 10 * 1024 * 1024,
            stage: "dry_run".into(),
        })
        .expect("NTFS dry-run plan should validate");
        assert_eq!(
            ntfs.argv,
            [
                "/usr/sbin/ntfsresize",
                "--no-action",
                "--size",
                "10485760",
                "/dev/sda",
            ]
        );
        assert!(!ntfs.needs_confirmation);

        let ext = build_plan(DiskOperationInput::FilesystemResize {
            device: device(),
            fs: "ext4".into(),
            new_size_bytes: 10 * 1024 + 1,
            stage: "resize".into(),
        })
        .expect("ext resize plan should validate");
        assert_eq!(ext.argv, ["/usr/sbin/resize2fs", "/dev/sda", "10K"]);

        let btrfs = build_plan(DiskOperationInput::FilesystemResize {
            device: device(),
            fs: "btrfs".into(),
            new_size_bytes: 20 * 1024 * 1024,
            stage: "resize".into(),
        })
        .expect("Btrfs resize plan should validate");
        assert_eq!(
            btrfs.argv,
            [
                "/usr/sbin/btrfs",
                "filesystem",
                "resize",
                "20971520",
                "/dev/sda",
            ]
        );
    }

    #[test]
    fn projects_btrfs_mount_lifecycle() {
        let mount = build_plan(DiskOperationInput::MountFilesystem {
            device: device(),
            mountpoint: "/tmp/kyth-resize".into(),
            options: vec![],
            bind: false,
        })
        .expect("mount plan should validate");
        assert_eq!(
            mount.argv,
            ["/usr/sbin/mount", "/dev/sda", "/tmp/kyth-resize"]
        );

        let subvolume = build_plan(DiskOperationInput::MountFilesystem {
            device: device(),
            mountpoint: "/tmp/kyth-target".into(),
            options: vec!["subvol=@".into(), "ro".into()],
            bind: false,
        })
        .expect("mount options should validate");
        assert_eq!(
            subvolume.argv,
            [
                "/usr/sbin/mount",
                "-o",
                "subvol=@,ro",
                "/dev/sda",
                "/tmp/kyth-target"
            ]
        );

        let bind = build_plan(DiskOperationInput::MountFilesystem {
            device: "/boot/efi".into(),
            mountpoint: "/tmp/kyth-target/boot/efi".into(),
            options: vec![],
            bind: true,
        })
        .expect("bind mount should validate");
        assert_eq!(
            bind.argv,
            [
                "/usr/sbin/mount",
                "--bind",
                "/boot/efi",
                "/tmp/kyth-target/boot/efi"
            ]
        );

        let unmount = build_plan(DiskOperationInput::UnmountFilesystem {
            mountpoint: "/tmp/kyth-resize".into(),
            recursive: false,
            lazy: false,
        })
        .expect("unmount plan should validate");
        assert_eq!(unmount.argv, ["/usr/sbin/umount", "/tmp/kyth-resize"]);

        let recursive = build_plan(DiskOperationInput::UnmountFilesystem {
            mountpoint: "/tmp/kyth-resize".into(),
            recursive: true,
            lazy: true,
        })
        .expect("recursive unmount should validate");
        assert_eq!(
            recursive.argv,
            ["/usr/sbin/umount", "-R", "-l", "/tmp/kyth-resize"]
        );
    }

    #[test]
    fn rejects_unsafe_paths_and_values() {
        let cases = [
            DiskOperationInput::CreateLabel {
                disk: "../../etc".into(),
                table_type: "gpt".into(),
            },
            DiskOperationInput::BackupTable {
                disk: device(),
                backup_path: "/tmp/../etc/x".into(),
            },
            DiskOperationInput::CreateLabel {
                disk: device(),
                table_type: "bsd".into(),
            },
            DiskOperationInput::SetPartitionFlag {
                disk: device(),
                part_num: 1,
                flag: "boot".into(),
                enabled: true,
            },
            DiskOperationInput::FormatFilesystem {
                device: device(),
                fs: "zfs".into(),
                label: String::new(),
            },
        ];
        for input in cases {
            assert!(build_plan(input).is_err());
        }
        assert!(build_plan(DiskOperationInput::BtrfsSubvolumeCreate {
            mountpoint: "/tmp/kyth-btrfs-root".into(),
            name: "../escape".into(),
        })
        .is_err());
    }

    #[test]
    fn projects_fixed_btrfs_subvolume_operations() {
        let create = build_plan(DiskOperationInput::BtrfsSubvolumeCreate {
            mountpoint: "/var/tmp/kyth-btrfs-root".into(),
            name: "@home".into(),
        })
        .expect("subvolume create should validate");
        assert_eq!(
            create.argv,
            [
                "/usr/sbin/btrfs",
                "subvolume",
                "create",
                "/var/tmp/kyth-btrfs-root/@home"
            ]
        );

        let default = build_plan(DiskOperationInput::BtrfsSubvolumeSetDefault {
            mountpoint: "/var/tmp/kyth-btrfs-root".into(),
            name: "@".into(),
        })
        .expect("subvolume default should validate");
        assert_eq!(
            default.argv,
            [
                "/usr/sbin/btrfs",
                "subvolume",
                "set-default",
                "/var/tmp/kyth-btrfs-root/@"
            ]
        );

        let directory = build_plan(DiskOperationInput::EnsureDirectory {
            path: "/var/tmp/kyth-alongside-target/boot/efi".into(),
        })
        .expect("directory creation should validate");
        assert_eq!(
            directory.argv,
            [
                "/usr/bin/mkdir",
                "-p",
                "/var/tmp/kyth-alongside-target/boot/efi"
            ]
        );
    }

    #[test]
    fn rejects_geometry_overflow_and_bad_sector_sizes() {
        for (start, size, sector_size) in [
            (u64::MAX, 512, 512),
            (1024, 511, 512),
            (1024, 1024, 1000),
            (1025, 1024, 512),
        ] {
            let input = DiskOperationInput::CreateUnformattedPartition {
                disk: device(),
                start,
                size,
                label: "biosboot".into(),
                sector_size,
            };
            assert!(build_plan(input).is_err());
        }
    }
}
