//! Port of `kyth_shared.system.drives` — NTFS/sanitize + mount helpers.
//!
//! `repair` mirrors `repair_ntfs_drives` print for print: partition scan,
//! `sudo` mount stabilization, up-to-3-level Steam library search, and the
//! compatdata backup/symlink/fsync dance. Only the `*_bin.rs` entry point
//! runs it against the live home directory.

use std::path::{Path, PathBuf};
use std::time::Duration;

// Simplified allow-list without regex crate: manual prefix checks
pub fn sanitize_dev_path(raw: &str) -> Option<String> {
    if raw.is_empty() { return None; }
    let c = Path::new(raw).canonicalize().ok()?.to_string_lossy().to_string();
    if c.starts_with("/dev/sd") || c.starts_with("/dev/nvme") || c.starts_with("/dev/vd") || c.starts_with("/dev/mmcblk") {
        // Basic check: must match /dev/(sd[a-z][0-9]* etc.)
        if c.starts_with("/dev/") && !c.contains("..") && !c.contains(' ') { return Some(c); }
    }
    None
}

pub fn sanitize_mount(raw: &str) -> Option<String> {
    const PREFIX: &str = "/var/mnt/ntfs_";
    if raw.is_empty() || !raw.starts_with(PREFIX) { return None; }
    let c = Path::new(raw).canonicalize().ok()?.to_string_lossy().to_string();
    if c.starts_with(PREFIX) || c == "/var/mnt" { return Some(c); }
    None
}

pub fn get_ntfs_devices() -> Vec<serde_json::Value> {
    let argv = ["lsblk", "-J", "-o", "NAME,FSTYPE,LABEL,UUID,MOUNTPOINT"]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let out = super::process::run_bounded(&argv, Duration::from_secs(5));
    if let Ok(o) = out {
        if o.status.success() {
            if let Ok(s) = String::from_utf8(o.stdout) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                    // Simplified: return blockdevices array
                    if let Some(arr) = v.get("blockdevices").and_then(|a| a.as_array()) {
                        return arr.iter().cloned().collect();
                    }
                }
            }
        }
    }
    Vec::new()
}

pub const MNT_PREFIX: &str = "/var/mnt/ntfs_";

pub fn native_compatdata(home: &Path) -> PathBuf {
    home.join(".local/share/Steam/steamapps/compatdata")
}

pub fn flatpak_compatdata(home: &Path) -> PathBuf {
    home.join(".var/app/com.valvesoftware.Steam/data/Steam/steamapps/compatdata")
}

pub fn mount_options() -> String {
    format!("uid={},gid={},dmask=027,fmask=137,windows_names,rw", unsafe { libc::getuid() }, unsafe { libc::getgid() })
}

/// Mirrors the `uuid_safe` derivation (sanitized, truncated to 64 chars).
pub fn uuid_safe_name(uuid: &str, dev_name: &str) -> String {
    let raw = if uuid.is_empty() { dev_name.replace('/', "_") } else { uuid.to_string() };
    raw.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).take(64).collect()
}

fn copy_dir_all(source: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn move_dir_backup(source: &Path, backup: &Path) -> std::io::Result<()> {
    match std::fs::rename(source, backup) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_dir_all(source, backup)?;
            std::fs::remove_dir_all(source)
        }
    }
}

/// Up-to-3-level `steamapps` search under a mount point, best-effort.
pub fn find_steam_dirs(mount: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(level1) = std::fs::read_dir(mount) else { return found };
    for entry1 in level1.filter_map(|entry| entry.ok()) {
        let path1 = entry1.path();
        if !path1.is_dir() {
            continue;
        }
        if path1.file_name().map(|name| name.to_string_lossy().to_lowercase() == "steamapps").unwrap_or(false) {
            found.push(path1);
            continue;
        }
        let Ok(level2) = std::fs::read_dir(&path1) else { continue };
        for entry2 in level2.filter_map(|entry| entry.ok()) {
            let path2 = entry2.path();
            if !path2.is_dir() {
                continue;
            }
            if path2.file_name().map(|name| name.to_string_lossy().to_lowercase() == "steamapps").unwrap_or(false) {
                found.push(path2);
                continue;
            }
            let Ok(level3) = std::fs::read_dir(&path2) else { continue };
            for entry3 in level3.filter_map(|entry| entry.ok()) {
                let path3 = entry3.path();
                if path3.is_dir()
                    && path3.file_name().map(|name| name.to_string_lossy().to_lowercase() == "steamapps").unwrap_or(false)
                {
                    found.push(path3);
                }
            }
        }
    }
    found
}

fn link_compatdata(steam_dir: &Path, native: &Path) {
    let target = steam_dir.join("compatdata");
    let target_link = target.to_string_lossy().into_owned();
    if target.is_symlink() {
        match std::fs::read_link(&target) {
            Ok(resolved) => println!("  Compatdata is already symlinked to native storage: {}", resolved.display()),
            Err(_) => println!("  Compatdata is already symlinked to native storage: {target_link}"),
        }
        return;
    }
    let mut backup: Option<PathBuf> = None;
    if target.is_dir() {
        println!("  Moving existing NTFS compatdata folder to backup...");
        let backup_name = format!(
            "compatdata.bak.{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
        );
        let backup_path = steam_dir.join(backup_name);
        match move_dir_backup(&target, &backup_path) {
            Ok(()) => backup = Some(backup_path),
            Err(error) => {
                println!("  Failed to backup existing compatdata: {error}");
            }
        }
    }
    let has_contents = std::fs::read_dir(native).map(|mut entries| entries.any(|_| true)).unwrap_or(false);
    if has_contents {
        println!("  NOTE: {native} already holds Proton prefixes from another library.", native = native.display());
        println!("  Any App ID present in both will now share one Proton prefix instead of having separate ones.");
    }
    println!("  Symlinking compatdata -> {native}", native = native.display());
    let tmp_link = steam_dir.join(format!(".compatdata.tmp.{}", std::process::id()));
    let linked = (|| -> std::io::Result<()> {
        if target.exists() || target.is_symlink() {
            let _ = std::fs::remove_file(&target);
        }
        std::os::unix::fs::symlink(native, &tmp_link)?;
        std::fs::rename(&tmp_link, &target)?;
        if let Ok(parent) = std::fs::File::open(steam_dir) {
            let _ = parent.sync_all();
        }
        Ok(())
    })();
    if let Err(error) = linked {
        println!("  Failed to create symlink: {error}");
        let _ = std::fs::remove_file(&tmp_link);
        if let Some(backup_path) = backup.filter(|path| path.exists()) {
            if target.is_symlink() || target.exists() {
                let _ = std::fs::remove_file(&target);
            }
            if std::fs::rename(&backup_path, &target).is_err() {
                println!("  WARNING: could not restore compatdata backup from {backup}", backup = backup_path.display());
            }
        }
    }
}

/// Lists NTFS partitions from raw `lsblk -J` output, then repairs.
pub fn repair(home: &Path, lsblk_json: &str) {
    let partitions = super::runtime_output::parse_ntfs_devices(lsblk_json);
    repair_partitions(home, &partitions);
}

/// Mirrors `repair_ntfs_drives` exactly, including every status line.
pub fn repair_partitions(home: &Path, partitions: &[serde_json::Value]) {
    let native = native_compatdata(home);
    let _ = std::fs::create_dir_all(&native);
    if home.join(".var/app/com.valvesoftware.Steam").is_dir() {
        let _ = std::fs::create_dir_all(flatpak_compatdata(home));
    }
    println!("[kyth-ntfs-repair] Scanning for connected NTFS storage partitions...");
    if partitions.is_empty() {
        println!("[kyth-ntfs-repair] No NTFS partitions detected.");
        return;
    }
    println!("[kyth-ntfs-repair] Found NTFS partition(s). Applying mount stabilization & Proton compatdata redirection...");
    let opts = mount_options();
    for dev in partitions {
        let name = dev.get("name").and_then(|v| v.as_str()).unwrap_or_default();
        let label = dev.get("label").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("NTFS_Drive");
        let uuid = dev.get("uuid").and_then(|v| v.as_str()).unwrap_or_default();
        let mut mount = dev.get("mountpoint").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let raw_dev = if name.starts_with('/') { name.to_string() } else { format!("/dev/{name}") };
        let Some(dev_path) = sanitize_dev_path(&raw_dev) else {
            eprintln!("drives: rejecting unexpected device path {raw_dev:?}");
            continue;
        };
        println!("[kyth-ntfs-repair] Processing partition: {dev_path} (UUID: {uuid}, Label: {label})",
            uuid = if uuid.is_empty() { "unknown" } else { uuid });
        if mount.is_empty() {
            let raw_mount = format!("{MNT_PREFIX}{}", uuid_safe_name(uuid, name));
            let resolved = sanitize_mount(&raw_mount).unwrap_or(raw_mount.clone());
            if !resolved.starts_with(MNT_PREFIX) {
                eprintln!("drives: rejecting mount path {raw_mount:?}");
                continue;
            }
            mount = resolved;
            println!("[kyth-ntfs-repair] Drive not mounted. Mounting to {mount}...");
            let mkdir = super::process::run_bounded(
                &["sudo".to_string(), "mkdir".to_string(), "-p".to_string(), mount.clone()],
                Duration::from_secs(30),
            );
            if let Err(error) = mkdir {
                println!("[kyth-ntfs-repair] Failed to mount {dev_path}: {error}");
                continue;
            }
            let ntfs3g = super::process::run_bounded(
                &["sudo".to_string(), "mount".to_string(), "-t".to_string(), "ntfs-3g".to_string(),
                    "-o".to_string(), opts.clone(), dev_path.clone(), mount.clone()],
                Duration::from_secs(60),
            );
            let mounted = ntfs3g.map(|output| output.status.success()).unwrap_or(false);
            if !mounted {
                let _ = super::process::run_bounded(
                    &["sudo".to_string(), "mount".to_string(), "-o".to_string(), opts.clone(),
                        dev_path.clone(), mount.clone()],
                    Duration::from_secs(60),
                );
            }
        }
        let mount_path = Path::new(&mount);
        if mount_path.is_dir() {
            for steam_dir in find_steam_dirs(mount_path) {
                println!("[kyth-ntfs-repair] Found Steam library at: {}", steam_dir.display());
                link_compatdata(&steam_dir, &native);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sanitize_none() {
        assert!(sanitize_dev_path("").is_none());
        assert!(sanitize_dev_path("../../etc/passwd").is_none());
    }

    #[test]
    fn derives_safe_mount_names() {
        assert_eq!(uuid_safe_name("ABC-123", "sda1"), "ABC-123");
        assert_eq!(uuid_safe_name("", "sda/1"), "sda_1");
        assert_eq!(uuid_safe_name("a/b?c", "x"), "a_b_c");
        assert_eq!(uuid_safe_name(&"a".repeat(100), "x").len(), 64);
    }

    #[test]
    fn finds_nested_steam_libraries() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b").join("SteamApps");
        std::fs::create_dir_all(&nested).unwrap();
        let found = find_steam_dirs(dir.path());
        assert_eq!(found, vec![nested]);
        assert!(find_steam_dirs(&dir.path().join("missing")).is_empty());
    }
}
