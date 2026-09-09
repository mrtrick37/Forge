//! Read-only installer planning queries.
//!
//! These helpers mirror the non-mutating portions of
//! `kyth_installer.plan_query`. They accept already-collected command or
//! storage data so callers retain ownership of probing, timeout, and error
//! policy. Nothing in this module opens a device or executes a command.

pub const BIOS_BOOT_BYTES: u64 = 1 << 20;
pub const MIN_KYTHOS_GIB: u64 = 32;
pub const MIN_KYTHOS_BYTES: u64 = MIN_KYTHOS_GIB * 1024 * 1024 * 1024;

/// Interpret the two bounded partition-table probe outputs used by Python.
/// The `blkid` result has precedence; `parted` is the fallback signal.
pub fn is_gpt_probe(blkid_stdout: &str, parted_stdout: &str) -> bool {
    blkid_stdout.trim().eq_ignore_ascii_case("gpt")
        || parted_stdout.contains("Partition Table: gpt")
}

/// Return whether any discovered partition has the BIOS boot type GUID.
pub fn has_bios_boot_partition<'a, I>(parttypes: I) -> bool
where
    I: IntoIterator<Item = &'a str>,
{
    const BIOS_BOOT_GUID: &str = "21686148-6449-6e6f-744e-656564454649";
    parttypes
        .into_iter()
        .any(|parttype| parttype.eq_ignore_ascii_case(BIOS_BOOT_GUID))
}

/// Return the minimum guided-install space, including a BIOS helper partition
/// when the disk is GPT and does not already have one.
pub fn required_guided_space(is_gpt: bool, has_bios_boot: bool) -> u64 {
    if is_gpt && !has_bios_boot {
        MIN_KYTHOS_BYTES + BIOS_BOOT_BYTES
    } else {
        MIN_KYTHOS_BYTES
    }
}

/// Parse the firmware's `BootCurrent` entry without invoking efibootmgr.
///
/// The returned line is intentionally the complete matching entry, matching
/// the Python helper's support/debug output rather than attempting to parse
/// firmware-specific fields after the entry name.
pub fn bootcurrent_entry(output: &str) -> Option<String> {
    let boot_id = output.lines().find_map(|line| {
        let value = line.trim().strip_prefix("BootCurrent:")?.trim();
        let candidate = value.split_whitespace().next()?;
        (candidate.len() == 4 && candidate.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then(|| candidate.to_ascii_uppercase())
    })?;
    let prefix = format!("Boot{boot_id}");
    output
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with(&prefix))
        .map(str::to_string)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskPartitionSnapshot {
    pub disk: String,
    pub partition: String,
    pub filesystem: String,
    pub size_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsResizeCandidate {
    pub disk: String,
    pub partition: String,
    pub size_bytes: u64,
    pub free_bytes: u64,
}

/// Select the largest viable NTFS partition from already-collected snapshots.
///
/// This preserves Python's stable first-winner behavior for equal-sized
/// partitions and skips undersized or non-NTFS records.
pub fn suggest_windows_resize_target(
    partitions: &[DiskPartitionSnapshot],
) -> Option<WindowsResizeCandidate> {
    let minimum = (64 + MIN_KYTHOS_GIB) * 1024 * 1024 * 1024;
    partitions
        .iter()
        .filter(|partition| partition.filesystem.to_ascii_lowercase() == "ntfs")
        .filter(|partition| partition.size_bytes >= minimum)
        .fold(None, |best, partition| {
            if best
                .as_ref()
                .is_none_or(|candidate: &WindowsResizeCandidate| {
                    partition.size_bytes > candidate.size_bytes
                })
            {
                Some(WindowsResizeCandidate {
                    disk: partition.disk.clone(),
                    partition: partition.partition.clone(),
                    size_bytes: partition.size_bytes,
                    free_bytes: partition.free_bytes,
                })
            } else {
                best
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpt_probe_prefers_blkid_and_falls_back_to_parted() {
        assert!(is_gpt_probe(" gPt\n", "Partition Table: msdos"));
        assert!(is_gpt_probe("", "Model\nPartition Table: gpt\n"));
        assert!(!is_gpt_probe("msdos", "Partition Table: unknown"));
    }

    #[test]
    fn detects_bios_partition_case_insensitively() {
        assert!(has_bios_boot_partition([
            "",
            "21686148-6449-6E6F-744E-656564454649",
        ]));
        assert!(!has_bios_boot_partition(["efi", "btrfs"]));
        assert_eq!(
            required_guided_space(true, false),
            MIN_KYTHOS_BYTES + BIOS_BOOT_BYTES
        );
        assert_eq!(required_guided_space(true, true), MIN_KYTHOS_BYTES);
    }

    #[test]
    fn parses_bootcurrent_entry_and_fails_closed() {
        let output = "BootCurrent: 0002\nBootOrder: 0002,0001\nBoot0002* KythOS HD(1,GPT)\n";
        assert_eq!(
            bootcurrent_entry(output).as_deref(),
            Some("Boot0002* KythOS HD(1,GPT)")
        );
        assert_eq!(
            bootcurrent_entry("BootCurrent: nope\nBoot0001* old\n"),
            None
        );
        assert_eq!(bootcurrent_entry("BootOrder: 0001\nBoot0001* old\n"), None);
    }

    #[test]
    fn chooses_largest_viable_ntfs_partition_stably() {
        let minimum = (64 + MIN_KYTHOS_GIB) * 1024 * 1024 * 1024;
        let partitions = vec![
            DiskPartitionSnapshot {
                disk: "/dev/sda".to_string(),
                partition: "/dev/sda1".to_string(),
                filesystem: "ext4".to_string(),
                size_bytes: minimum + 1,
                free_bytes: 0,
            },
            DiskPartitionSnapshot {
                disk: "/dev/sdb".to_string(),
                partition: "/dev/sdb1".to_string(),
                filesystem: "NTFS".to_string(),
                size_bytes: minimum + 10,
                free_bytes: 80 * 1024 * 1024 * 1024,
            },
            DiskPartitionSnapshot {
                disk: "/dev/sdc".to_string(),
                partition: "/dev/sdc1".to_string(),
                filesystem: "ntfs".to_string(),
                size_bytes: minimum + 10,
                free_bytes: 90 * 1024 * 1024 * 1024,
            },
        ];
        assert_eq!(
            suggest_windows_resize_target(&partitions)
                .expect("a viable NTFS partition")
                .partition,
            "/dev/sdb1"
        );
    }
}
