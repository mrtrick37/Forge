//! Read-only `lsblk --json --bytes` snapshot parsing for installer discovery.
//!
//! The parser accepts explicit snapshots so the safety policy is testable
//! without touching devices. The root-owned daemon supplies those snapshots
//! through fixed, read-only probes and serializes the same API records that
//! the compatibility service historically returned.

use serde::{Deserialize, Serialize};

const EFI_PART_GUID: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";
const MIN_KYTHOS_BYTES: u64 = 32 * 1024 * 1024 * 1024;
const NTFS_MIN_BYTES: u64 = (64 + 32) * 1024 * 1024 * 1024;
const BIOS_BOOT_GUID: &str = "21686148-6449-6e6f-744e-656564454649";
const GPT_RESERVE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Deserialize)]
struct LsblkSnapshot {
    #[serde(default)]
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Debug, Deserialize)]
struct LsblkDevice {
    name: Option<String>,
    partn: Option<u32>,
    size: Option<u64>,
    #[serde(rename = "type")]
    device_type: Option<String>,
    pkname: Option<String>,
    fstype: Option<String>,
    parttype: Option<String>,
    label: Option<String>,
    model: Option<String>,
    mountpoint: Option<String>,
    mountpoints: Option<Vec<Option<String>>>,
    start: Option<u64>,
    ro: Option<bool>,
    rm: Option<bool>,
    rota: Option<bool>,
    tran: Option<String>,
    pttype: Option<String>,
    #[serde(default)]
    children: Vec<LsblkDevice>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct DiskRecord {
    pub name: String,
    pub size_bytes: u64,
    pub model: String,
    pub ssd: bool,
    pub transport: String,
    pub removable: bool,
    pub partition_table: String,
    pub current: bool,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct PartitionRecord {
    pub name: String,
    pub size_bytes: u64,
    pub start_bytes: u64,
    pub fstype: String,
    pub label: String,
    pub parttype: String,
    pub mountpoints: Vec<String>,
    pub efi: bool,
    pub current: bool,
    pub in_use: bool,
    pub read_only: bool,
    pub alongside_candidate: bool,
    pub ntfs_resize_candidate: bool,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct FreeRegionRecord {
    pub start_bytes: u64,
    pub end_bytes: u64,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PartitionProbe {
    pub name: String,
    pub number: u32,
    pub size_bytes: u64,
    pub start_bytes: u64,
    pub fstype: String,
    pub efi: bool,
    pub current: bool,
    pub in_use: bool,
    pub read_only: bool,
}

fn normalize_device_path(raw: &str) -> Option<String> {
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
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'+' | b':' | b'-')
        })
    {
        return None;
    }
    Some(value)
}

fn mountpoints(device: &LsblkDevice) -> Vec<String> {
    if let Some(values) = &device.mountpoints {
        return values.iter().filter_map(|value| value.clone()).collect();
    }
    device.mountpoint.clone().into_iter().collect()
}

fn descendant_mountpoints(device: &LsblkDevice) -> Vec<String> {
    device
        .children
        .iter()
        .flat_map(|child| {
            let mut mounts = mountpoints(child);
            mounts.extend(descendant_mountpoints(child));
            mounts
        })
        .collect()
}

fn parse_snapshot(input: &str) -> Result<LsblkSnapshot, String> {
    serde_json::from_str(input).map_err(|error| format!("invalid lsblk snapshot: {error}"))
}

fn device_ancestry(
    input: &str,
) -> Result<std::collections::HashMap<String, (String, Option<String>)>, String> {
    let snapshot = parse_snapshot(input)?;
    let mut devices = std::collections::HashMap::new();
    fn walk(
        entries: &[LsblkDevice],
        devices: &mut std::collections::HashMap<String, (String, Option<String>)>,
    ) {
        for entry in entries {
            if let Some(name) = entry.name.as_deref().and_then(normalize_device_path) {
                let parent = entry.pkname.as_deref().and_then(normalize_device_path);
                let _ = devices.insert(
                    name,
                    (entry.device_type.clone().unwrap_or_default(), parent),
                );
            }
            walk(&entry.children, devices);
        }
    }
    walk(&snapshot.blockdevices, &mut devices);
    Ok(devices)
}

/// Resolve a mount source to its physical disk using an ancestry snapshot.
pub(crate) fn parent_disk_in_snapshot(input: &str, source: &str) -> Result<Option<String>, String> {
    let devices = device_ancestry(input)?;
    let mut current = normalize_device_path(source);
    let mut seen = std::collections::HashSet::new();
    while let Some(device) = current {
        if !seen.insert(device.clone()) {
            return Ok(None);
        }
        let Some((device_type, parent)) = devices.get(&device) else {
            return Ok(None);
        };
        if device_type == "disk" {
            return Ok(Some(device));
        }
        current = parent.clone();
    }
    Ok(None)
}

/// Build the runtime disk inventory from separate device and ancestry probes.
pub(crate) fn runtime_disks_from_snapshots(
    disk_snapshot: &str,
    ancestry_snapshot: &str,
    protected_sources: &[String],
    current_source: Option<&str>,
) -> Result<Vec<DiskRecord>, String> {
    let mut protected = std::collections::HashSet::new();
    for source in protected_sources {
        if let Some(disk) = parent_disk_in_snapshot(ancestry_snapshot, source)? {
            protected.insert(disk);
        }
    }
    let current_disk = current_source
        .map(|source| parent_disk_in_snapshot(ancestry_snapshot, source))
        .transpose()?
        .flatten();
    parse_disks(
        disk_snapshot,
        &protected.into_iter().collect::<Vec<_>>(),
        current_disk.as_deref(),
    )
}

fn disk_metadata(input: &str, disk: &str) -> Result<(u64, bool), String> {
    let snapshot = parse_snapshot(input)?;
    let target = normalize_device_path(disk)
        .ok_or_else(|| "invalid disk path for storage query".to_string())?;
    snapshot
        .blockdevices
        .iter()
        .find_map(|entry| {
            let name = entry.name.as_deref().and_then(normalize_device_path)?;
            (name == target && entry.device_type.as_deref() == Some("disk")).then(|| {
                (
                    entry.size.unwrap_or(0),
                    entry
                        .pttype
                        .as_deref()
                        .unwrap_or_default()
                        .eq_ignore_ascii_case("gpt"),
                )
            })
        })
        .ok_or_else(|| "storage query did not return the selected disk".to_string())
}

/// Calculate free regions using the same reserved-boundary and BIOS-boot
/// minimums as the Python compatibility query.
pub(crate) fn free_regions(
    disk_snapshot: &str,
    disk: &str,
    sector_size: u64,
) -> Result<Vec<FreeRegionRecord>, String> {
    if !sector_size.is_power_of_two() || !(512..=4096).contains(&sector_size) {
        return Err("storage query returned an unsupported sector size".to_string());
    }
    let (disk_size, is_gpt) = disk_metadata(disk_snapshot, disk)?;
    if disk_size <= GPT_RESERVE_BYTES.saturating_mul(2) {
        return Ok(Vec::new());
    }
    let partitions = parse_partitions(disk_snapshot)?;
    let has_bios_boot = partitions
        .iter()
        .any(|part| part.parttype.eq_ignore_ascii_case(BIOS_BOOT_GUID));
    let required = MIN_KYTHOS_BYTES
        + if is_gpt && !has_bios_boot {
            GPT_RESERVE_BYTES
        } else {
            0
        };
    let mut spans = Vec::new();
    for partition in partitions {
        if partition.size_bytes == 0
            || partition.start_bytes > disk_size
            || partition.size_bytes > disk_size.saturating_sub(partition.start_bytes)
        {
            return Ok(Vec::new());
        }
        let start = (partition.start_bytes / sector_size) * sector_size;
        let size = (partition.size_bytes / sector_size) * sector_size;
        if size == 0 || start > disk_size.saturating_sub(size) {
            return Ok(Vec::new());
        }
        spans.push((start, start + size));
    }
    spans.sort_unstable();
    let usable_end = disk_size - GPT_RESERVE_BYTES;
    let mut cursor = GPT_RESERVE_BYTES;
    let mut regions = Vec::new();
    for (start, end) in spans {
        if start > cursor {
            append_region(&mut regions, cursor, start, sector_size, required);
        }
        cursor = cursor.max(end);
    }
    if cursor < usable_end {
        append_region(&mut regions, cursor, usable_end, sector_size, required);
    }
    Ok(regions)
}

fn append_region(
    regions: &mut Vec<FreeRegionRecord>,
    start: u64,
    end: u64,
    sector_size: u64,
    required: u64,
) {
    let aligned_start = start.div_ceil(sector_size) * sector_size;
    let aligned_end = (end / sector_size) * sector_size;
    if aligned_end > aligned_start && aligned_end - aligned_start >= required {
        regions.push(FreeRegionRecord {
            start_bytes: aligned_start,
            end_bytes: aligned_end,
            size_bytes: aligned_end - aligned_start,
        });
    }
}

/// Parse safe, writable whole-disk records from an explicit lsblk snapshot.
///
pub(crate) fn parse_disks(
    input: &str,
    protected: &[String],
    current_disk: Option<&str>,
) -> Result<Vec<DiskRecord>, String> {
    let snapshot = parse_snapshot(input)?;
    Ok(snapshot
        .blockdevices
        .iter()
        .filter(|device| device.device_type.as_deref() == Some("disk"))
        .filter_map(|device| {
            let name = normalize_device_path(device.name.as_deref()?)?;
            let size_bytes = device.size.unwrap_or(0);
            if size_bytes == 0 || device.ro.unwrap_or(false) || protected.contains(&name) {
                return None;
            }
            Some(DiskRecord {
                current: current_disk == Some(name.as_str()),
                name,
                size_bytes,
                model: device
                    .model
                    .as_deref()
                    .unwrap_or("Unknown drive")
                    .trim()
                    .to_string(),
                ssd: !device.rota.unwrap_or(false),
                transport: device.tran.clone().unwrap_or_default(),
                removable: device.rm.unwrap_or(false),
                partition_table: device
                    .pttype
                    .clone()
                    .unwrap_or_default()
                    .to_ascii_lowercase(),
            })
        })
        .collect())
}

/// Parse partition records, including descendant mounts, from an lsblk tree.
///
pub(crate) fn parse_partitions(input: &str) -> Result<Vec<PartitionRecord>, String> {
    let snapshot = parse_snapshot(input)?;
    let mut partitions = Vec::new();

    fn walk(devices: &[LsblkDevice], partitions: &mut Vec<PartitionRecord>) {
        for device in devices {
            if device.device_type.as_deref() == Some("part") {
                if let Some(name) = device.name.as_deref().and_then(normalize_device_path) {
                    let size_bytes = device.size.unwrap_or(0);
                    let fstype = device
                        .fstype
                        .as_deref()
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    let parttype = device
                        .parttype
                        .as_deref()
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    let mut mounts = mountpoints(device);
                    mounts.extend(descendant_mountpoints(device));
                    let efi = parttype == EFI_PART_GUID
                        || (fstype == "vfat" && mounts.iter().any(|mount| mount == "/boot/efi"));
                    let current = !mounts.is_empty();
                    let in_use = !device.children.is_empty();
                    let read_only = device.ro.unwrap_or(false);
                    let alongside_candidate =
                        size_bytes >= MIN_KYTHOS_BYTES && !efi && !current && !in_use && !read_only;
                    let ntfs_resize_candidate = alongside_candidate
                        && matches!(fstype.as_str(), "ntfs" | "ntfs3")
                        && size_bytes >= NTFS_MIN_BYTES;
                    partitions.push(PartitionRecord {
                        name,
                        size_bytes,
                        start_bytes: device.start.unwrap_or(0).saturating_mul(512),
                        fstype,
                        label: device.label.clone().unwrap_or_default(),
                        parttype,
                        mountpoints: mounts,
                        efi,
                        current,
                        in_use,
                        read_only,
                        alongside_candidate,
                        ntfs_resize_candidate,
                    });
                }
            }
            walk(&device.children, partitions);
        }
    }

    walk(&snapshot.blockdevices, &mut partitions);
    Ok(partitions)
}

/// Select the installed Btrfs root partition from a fresh lsblk tree.
///
/// Partition numbers are not guessed: the result must be a child of the
/// requested disk, must be a partition, and must report Btrfs as its
/// filesystem. EFI, BIOS-boot, and unrelated filesystems are ignored.
pub(crate) fn root_partition_from_snapshot(input: &str, disk: &str) -> Result<String, String> {
    let disk = normalize_device_path(disk)
        .ok_or_else(|| "root partition query has an invalid disk".to_string())?;
    let snapshot = parse_snapshot(input)?;
    let root = snapshot
        .blockdevices
        .iter()
        .find(|device| {
            normalize_device_path(device.name.as_deref().unwrap_or_default()).as_deref()
                == Some(disk.as_str())
                && device.device_type.as_deref() == Some("disk")
        })
        .ok_or_else(|| "target disk was not present in root partition probe".to_string())?;
    let mut candidates = Vec::new();
    fn collect(device: &LsblkDevice, candidates: &mut Vec<String>) {
        if device.device_type.as_deref() == Some("part")
            && device
                .fstype
                .as_deref()
                .map(str::to_ascii_lowercase)
                .as_deref()
                == Some("btrfs")
        {
            if let Some(name) = device.name.as_deref().and_then(normalize_device_path) {
                candidates.push(name);
            }
        }
        for child in &device.children {
            collect(child, candidates);
        }
    }
    collect(root, &mut candidates);
    candidates.into_iter().next().ok_or_else(|| {
        "target disk has no Btrfs root partition after bootc installation".to_string()
    })
}

/// Revalidate one partition as a member of the selected disk.
///
/// The returned geometry is intentionally sourced from the same fresh tree
/// used to validate the parent relationship. Callers must use it immediately
/// before a destructive operation; a stale caller-supplied partition number or
/// size is never trusted.
pub(crate) fn partition_probe_from_snapshot(
    input: &str,
    disk: &str,
    partition: &str,
) -> Result<PartitionProbe, String> {
    let disk = normalize_device_path(disk)
        .ok_or_else(|| "partition query has an invalid disk".to_string())?;
    let partition = normalize_device_path(partition)
        .ok_or_else(|| "partition query has an invalid partition".to_string())?;
    let snapshot = parse_snapshot(input)?;
    let root = snapshot
        .blockdevices
        .iter()
        .find(|device| {
            normalize_device_path(device.name.as_deref().unwrap_or_default()).as_deref()
                == Some(disk.as_str())
                && device.device_type.as_deref() == Some("disk")
        })
        .ok_or_else(|| "target disk was not present in partition probe".to_string())?;

    fn find_partition(device: &LsblkDevice, wanted: &str) -> Option<PartitionProbe> {
        if device.device_type.as_deref() == Some("part")
            && normalize_device_path(device.name.as_deref().unwrap_or_default()).as_deref()
                == Some(wanted)
        {
            let name = device.name.as_deref().and_then(normalize_device_path)?;
            let number = device.partn?;
            let fstype = device
                .fstype
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase();
            let mounts = mountpoints(device);
            let efi = device
                .parttype
                .as_deref()
                .is_some_and(|parttype| parttype.eq_ignore_ascii_case(EFI_PART_GUID))
                || (fstype == "vfat" && mounts.iter().any(|mount| mount == "/boot/efi"));
            return Some(PartitionProbe {
                name,
                number,
                size_bytes: device.size.unwrap_or(0),
                start_bytes: device.start.unwrap_or(0).saturating_mul(512),
                fstype,
                efi,
                current: !mounts.is_empty(),
                in_use: !device.children.is_empty(),
                read_only: device.ro.unwrap_or(false),
            });
        }
        device
            .children
            .iter()
            .find_map(|child| find_partition(child, wanted))
    }

    find_partition(root, &partition)
        .ok_or_else(|| "selected partition was not present on the target disk".to_string())
}

/// Confirm that a selected free-space interval is still one of the safe
/// regions in a fresh disk snapshot.
pub(crate) fn contains_free_region(
    input: &str,
    disk: &str,
    start: u64,
    end: u64,
    sector_size: u64,
) -> Result<bool, String> {
    if end <= start {
        return Ok(false);
    }
    Ok(free_regions(input, disk, sector_size)?
        .iter()
        .any(|region| start >= region.start_bytes && end <= region.end_bytes))
}

/// Identify one newly created partition by its post-mutation geometry.
///
/// A name-set difference alone is unsafe when udev exposes stale entries or a
/// partition operation creates more than one object. Geometry must match
/// within one MiB and exactly one new candidate must remain.
pub(crate) fn new_partition_from_snapshots(
    before: &str,
    after: &str,
    start_bytes: u64,
    size_bytes: u64,
) -> Result<String, String> {
    if start_bytes == 0 || size_bytes == 0 {
        return Err("new partition geometry must be positive".to_string());
    }
    let prior = parse_partitions(before)?
        .into_iter()
        .map(|partition| partition.name)
        .collect::<std::collections::HashSet<_>>();
    const GEOMETRY_TOLERANCE: u64 = 1024 * 1024;
    let candidates = parse_partitions(after)?
        .into_iter()
        .filter(|partition| !prior.contains(&partition.name))
        .filter(|partition| {
            partition.start_bytes.abs_diff(start_bytes) <= GEOMETRY_TOLERANCE
                && partition.size_bytes.abs_diff(size_bytes) <= GEOMETRY_TOLERANCE
        })
        .map(|partition| partition.name)
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [name] => Ok(name.clone()),
        [] => Err("new partition was not visible at the requested geometry".to_string()),
        _ => Err("multiple new partitions matched the requested geometry".to_string()),
    }
}

pub(crate) fn has_bios_boot_partition(input: &str) -> Result<bool, String> {
    Ok(parse_partitions(input)?
        .into_iter()
        .any(|partition| partition.parttype.eq_ignore_ascii_case(BIOS_BOOT_GUID)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNAPSHOT: &str = include_str!("../testdata/lsblk_snapshot.json");

    #[test]
    fn parses_and_filters_disk_snapshot() {
        let disks = parse_disks(SNAPSHOT, &["/dev/sdb".to_string()], Some("/dev/sda"))
            .expect("snapshot should parse");
        assert_eq!(disks.len(), 1);
        assert_eq!(disks[0].name, "/dev/sda");
        assert!(disks[0].current);
        assert_eq!(disks[0].partition_table, "gpt");
    }

    #[test]
    fn parses_partition_candidates_and_descendant_mounts() {
        let partitions = parse_partitions(SNAPSHOT).expect("snapshot should parse");
        assert_eq!(partitions.len(), 2);
        assert!(partitions[0].efi);
        assert!(!partitions[0].alongside_candidate);
        assert!(partitions[1].in_use);
        assert!(partitions[1].current);
        assert!(!partitions[1].ntfs_resize_candidate);
        assert!(partitions[1]
            .mountpoints
            .iter()
            .any(|mount| mount == "/mnt"));
    }

    #[test]
    fn rejects_malformed_snapshot() {
        let error = parse_partitions("not-json").expect_err("malformed JSON must fail closed");
        assert!(error.contains("invalid lsblk snapshot"));
    }

    #[test]
    fn runtime_inventory_resolves_protected_and_current_disks() {
        let ancestry = r#"{"blockdevices":[
            {"name":"/dev/sda","type":"disk"},
            {"name":"/dev/sda1","type":"part","pkname":"/dev/sda"},
            {"name":"/dev/sdb","type":"disk"},
            {"name":"/dev/sdb1","type":"part","pkname":"/dev/sdb"}
        ]}"#;
        let disks = runtime_disks_from_snapshots(
            SNAPSHOT,
            ancestry,
            &["/dev/sdb1".to_string()],
            Some("/dev/sda1"),
        )
        .expect("runtime snapshots should parse");
        assert_eq!(disks.len(), 1);
        assert_eq!(disks[0].name, "/dev/sda");
        assert!(disks[0].current);
        assert_eq!(
            parent_disk_in_snapshot(ancestry, "/dev/sdb1")
                .unwrap()
                .as_deref(),
            Some("/dev/sdb")
        );
    }

    #[test]
    fn free_regions_retain_aligned_space_and_minimum() {
        let disk_size = 100 * 1024 * 1024 * 1024_u64;
        let snapshot = format!(
            r#"{{"blockdevices":[{{"name":"/dev/sda","size":{disk_size},"type":"disk","pttype":"gpt","children":[{{"name":"/dev/sda1","size":{},"type":"part","start":2048,"parttype":"x"}}]}}]}}"#,
            32 * 1024 * 1024 * 1024_u64
        );
        let regions = free_regions(&snapshot, "/dev/sda", 512).expect("free space should parse");
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].start_bytes % 512, 0);
        assert_eq!(regions[0].end_bytes, disk_size - GPT_RESERVE_BYTES);
        assert!(regions[0].size_bytes >= MIN_KYTHOS_BYTES + GPT_RESERVE_BYTES);
    }

    #[test]
    fn selects_btrfs_root_only_from_selected_disk() {
        let snapshot = r#"{
            "blockdevices": [
                {
                    "name": "/dev/sda",
                    "type": "disk",
                    "children": [
                        {"name": "/dev/sda1", "type": "part", "fstype": "vfat"},
                        {"name": "/dev/sda2", "type": "part", "fstype": "BTRFS"}
                    ]
                },
                {
                    "name": "/dev/sdb",
                    "type": "disk",
                    "children": [
                        {"name": "/dev/sdb1", "type": "part", "fstype": "btrfs"}
                    ]
                }
            ]
        }"#;
        assert_eq!(
            root_partition_from_snapshot(snapshot, "sda").unwrap(),
            "/dev/sda2"
        );
    }

    #[test]
    fn rejects_missing_or_unrelated_btrfs_root() {
        let no_root = r#"{
            "blockdevices": [{
                "name": "/dev/sda",
                "type": "disk",
                "children": [{"name": "/dev/sda1", "type": "part", "fstype": "vfat"}]
            }]
        }"#;
        let error = root_partition_from_snapshot(no_root, "/dev/sda")
            .expect_err("a disk without Btrfs must fail closed");
        assert!(error.contains("no Btrfs root partition"), "{error}");

        let unrelated = r#"{
            "blockdevices": [{
                "name": "/dev/sdb",
                "type": "disk",
                "children": [{"name": "/dev/sdb1", "type": "part", "fstype": "btrfs"}]
            }]
        }"#;
        let error = root_partition_from_snapshot(unrelated, "/dev/sda")
            .expect_err("an absent selected disk must fail closed");
        assert!(error.contains("target disk was not present"), "{error}");
    }

    #[test]
    fn partition_probe_requires_parent_and_reports_live_safety_fields() {
        let disk_size = 128 * 1024 * 1024 * 1024_u64;
        let partition_size = 96 * 1024 * 1024 * 1024_u64;
        let snapshot = format!(
            r#"{{"blockdevices":[{{"name":"/dev/sda","size":{disk_size},"type":"disk","pttype":"gpt","children":[{{"name":"/dev/sda1","partn":1,"size":{partition_size},"type":"part","fstype":"ntfs","start":2048,"mountpoints":[null],"ro":false}}]}}]}}"#
        );
        let partition = partition_probe_from_snapshot(&snapshot, "/dev/sda", "sda1")
            .expect("selected partition should be found");
        assert_eq!(partition.name, "/dev/sda1");
        assert_eq!(partition.number, 1);
        assert_eq!(partition.fstype, "ntfs");
        assert_eq!(partition.start_bytes, 2048 * 512);
        assert!(!partition.current);
        assert!(!partition.in_use);
        assert!(!partition.efi);
        assert!(!partition.read_only);

        assert!(partition_probe_from_snapshot(&snapshot, "/dev/sdb", "/dev/sda1").is_err());
    }

    #[test]
    fn free_region_check_rejects_stale_or_overlapping_selection() {
        let disk_size = 100 * 1024 * 1024 * 1024_u64;
        let snapshot = format!(
            r#"{{"blockdevices":[{{"name":"/dev/sda","size":{disk_size},"type":"disk","pttype":"gpt","children":[]}}]}}"#
        );
        let regions = free_regions(&snapshot, "/dev/sda", 512).unwrap();
        let region = &regions[0];
        assert!(contains_free_region(
            &snapshot,
            "/dev/sda",
            region.start_bytes,
            region.end_bytes,
            512
        )
        .unwrap());
        assert!(!contains_free_region(
            &snapshot,
            "/dev/sda",
            region.start_bytes.saturating_sub(512),
            region.end_bytes,
            512
        )
        .unwrap());
    }

    #[test]
    fn new_partition_selection_requires_unique_geometry_match() {
        let before = r#"{"blockdevices":[{"name":"/dev/sda","type":"disk","children":[]}] }"#;
        let after = r#"{"blockdevices":[{"name":"/dev/sda","type":"disk","children":[
            {"name":"/dev/sda1","type":"part","size":34359738368,"start":4096}
        ]}] }"#;
        assert_eq!(
            new_partition_from_snapshots(before, after, 4096 * 512, 34359738368).unwrap(),
            "/dev/sda1"
        );
        assert!(new_partition_from_snapshots(before, after, 8192 * 512, 34359738368).is_err());
    }

    #[test]
    fn detects_bios_boot_partition_by_guid() {
        let snapshot = format!(
            r#"{{"blockdevices":[{{"name":"/dev/sda","type":"disk","children":[{{"name":"/dev/sda1","type":"part","parttype":"{}"}}]}}]}}"#,
            BIOS_BOOT_GUID
        );
        assert!(has_bios_boot_partition(&snapshot).unwrap());
    }
}
