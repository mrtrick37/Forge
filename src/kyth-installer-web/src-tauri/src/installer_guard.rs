//! Last-moment guards for the selected installation disk.

use crate::installer_storage;

fn normalized(disk: &str) -> Result<String, String> {
    crate::installer_plan::normalize_device_path(disk)
        .ok_or_else(|| "selected disk is not a safe device path".to_string())
}

/// Validate disk ownership from already-captured read-only snapshots.
///
/// `protected_sources` contains mounted device paths, not disk paths. Keeping
/// this function snapshot-based makes the policy independently testable and
/// ensures callers can pair it with a fresh probe immediately before a
/// destructive helper or bootc invocation.
pub(crate) fn validate_target_snapshots(
    disk_snapshot: &str,
    ancestry_snapshot: &str,
    protected_sources: &[String],
    current_source: Option<&str>,
    disk: &str,
) -> Result<(), String> {
    let disk = normalized(disk)?;
    let records = installer_storage::runtime_disks_from_snapshots(
        disk_snapshot,
        ancestry_snapshot,
        protected_sources,
        current_source,
    )?;
    let Some(record) = records.iter().find(|record| record.name == disk) else {
        return Err(format!(
            "selected disk {disk} is unavailable, read-only, or protected by a live mount"
        ));
    };
    if record.current {
        return Err(format!(
            "selected disk {disk} is the current live-session disk"
        ));
    }
    Ok(())
}

fn findmnt_sources(path: &str, recursive: bool) -> Result<Vec<String>, String> {
    let mut args = Vec::with_capacity(5);
    if recursive {
        args.push("-R");
    }
    args.extend(["-n", "-o", "SOURCE", path]);
    let output = std::process::Command::new("/usr/bin/findmnt")
        .args(args)
        .output()
        .map_err(|error| format!("could not run findmnt: {error}"))?;
    if !output.status.success() && output.status.code() != Some(1) {
        return Err(format!(
            "findmnt failed with exit code {}: {}",
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8(output.stdout)
        .map_err(|_| "findmnt returned non-UTF-8 output".to_string())?
        .lines()
        .map(str::trim)
        .filter(|source| source.starts_with("/dev/"))
        .map(str::to_string)
        .collect())
}

fn command_output(program: &str, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} failed with exit code {}: {}",
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("{program} returned non-UTF-8 output"))
}

/// Re-probe all disk ancestry and live mounts immediately before mutation.
pub(crate) fn validate_target_disk(disk: &str) -> Result<(), String> {
    let disk = normalized(disk)?;
    let disk_snapshot = command_output(
        "/usr/bin/lsblk",
        &[
            "--json",
            "--bytes",
            "--paths",
            "--output",
            "NAME,SIZE,TYPE,FSTYPE,PARTTYPE,PARTN,LABEL,MOUNTPOINT,MOUNTPOINTS,START,RO,MODEL,TRAN,ROTA,RM,PTTYPE,PKNAME",
        ],
    )?;
    let ancestry_snapshot = command_output(
        "/usr/bin/lsblk",
        &[
            "--json",
            "--bytes",
            "--paths",
            "--output",
            "NAME,PKNAME,TYPE",
        ],
    )?;
    let mut protected_sources = Vec::new();
    for mount in [
        "/",
        "/boot",
        "/boot/efi",
        "/sysroot",
        "/run/initramfs/live",
        "/run/initramfs/iso",
    ] {
        protected_sources.extend(findmnt_sources(mount, false)?);
    }
    protected_sources.extend(findmnt_sources("/run/initramfs", true)?);
    protected_sources.extend(findmnt_sources("/run/media", true)?);
    let current_source = findmnt_sources("/", false)?.into_iter().next();
    validate_target_snapshots(
        &disk_snapshot,
        &ancestry_snapshot,
        &protected_sources,
        current_source.as_deref(),
        &disk,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshots() -> (&'static str, &'static str) {
        (
            r#"{"blockdevices":[
                {"name":"/dev/sda","size":100,"type":"disk","ro":false},
                {"name":"/dev/sdb","size":100,"type":"disk","ro":false}
            ]}"#,
            r#"{"blockdevices":[
                {"name":"/dev/sda","type":"disk"},
                {"name":"/dev/sdb","type":"disk"}
            ]}"#,
        )
    }

    #[test]
    fn accepts_unprotected_disk_and_rejects_current_or_protected_disk() {
        let (disk_snapshot, ancestry_snapshot) = snapshots();
        assert!(validate_target_snapshots(
            disk_snapshot,
            ancestry_snapshot,
            &[],
            Some("/dev/sda"),
            "/dev/sdb"
        )
        .is_ok());
        assert!(validate_target_snapshots(
            disk_snapshot,
            ancestry_snapshot,
            &[],
            Some("/dev/sda"),
            "/dev/sda"
        )
        .is_err());
        assert!(validate_target_snapshots(
            disk_snapshot,
            ancestry_snapshot,
            &["/dev/sdb".to_string()],
            Some("/dev/sda"),
            "/dev/sdb"
        )
        .is_err());
    }

    #[test]
    fn rejects_unknown_target_before_probe_policy() {
        let (disk_snapshot, ancestry_snapshot) = snapshots();
        let error = validate_target_snapshots(
            disk_snapshot,
            ancestry_snapshot,
            &[],
            None,
            "../../etc/passwd",
        )
        .expect_err("unsafe target must fail closed");
        assert!(error.contains("safe device path"));
    }
}
