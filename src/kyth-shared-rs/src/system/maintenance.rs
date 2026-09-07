//! Maintenance discovery, trash pruning, and command planning.
//!
//! This ports the bounded target scan, trash pruning, and argv projection
//! from `kyth_shared.maintenance`. Deduplication database creation and the
//! deduplication run itself stay with the caller.

use std::path::{Path, PathBuf};

pub fn supports_dedupe_filesystem(filesystem: &str) -> bool {
    matches!(filesystem.trim().to_ascii_lowercase().as_str(), "btrfs" | "xfs")
}

pub fn find_dedupe_targets(root: impl AsRef<Path>) -> Vec<PathBuf> {
    fn walk(current: &Path, depth: usize, targets: &mut Vec<PathBuf>) {
        if depth > 7 { return; }
        let Ok(entries) = std::fs::read_dir(current) else { return; };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else { continue; };
            if !file_type.is_dir() || file_type.is_symlink() { continue; }
            let text = path.to_string_lossy();
            if depth >= 4 && (text.ends_with("/Steam/steamapps/compatdata") || text.ends_with("/Steam/steamapps/shadercache")) {
                targets.push(path);
                continue;
            }
            walk(&path, depth + 1, targets);
        }
    }
    let mut targets = Vec::new();
    let root = root.as_ref();
    if root.is_dir() { walk(root, 1, &mut targets); }
    targets.sort();
    targets.dedup();
    targets
}

pub fn dedupe_command(target: impl AsRef<Path>, hash_file: impl AsRef<Path>, ionice_available: bool) -> Vec<String> {
    let core = ["nice", "-n", "19", "duperemove", "-rdh", "--hashfile"];
    let mut command = if ionice_available { vec!["ionice".into(), "-c3".into()] } else { Vec::new() };
    command.extend(core.into_iter().map(String::from));
    command.push(hash_file.as_ref().display().to_string());
    command.push(target.as_ref().display().to_string());
    command
}

pub fn cleanup_flatpaks_command() -> Vec<String> {
    vec!["flatpak".into(), "uninstall".into(), "--unused".into(), "-y".into(), "--noninteractive".into()]
}

pub fn vacuum_user_journal_command(days: i64) -> Vec<String> {
    vec!["journalctl".into(), "--user".into(), format!("--vacuum-time={days}d")]
}

/// Parse a trash `DeletionDate=` value to UTC epoch seconds, mirroring
/// `datetime.fromisoformat`: naive stamps are UTC, a trailing `Z` or a
/// numeric offset adjusts accordingly. Returns `None` when unparseable.
pub fn parse_deletion_epoch(text: &str) -> Option<i64> {
    let text = text.trim();
    let text = text.strip_suffix(['Z', 'z']).unwrap_or(text);
    let (naive, offset_secs) = match text.rfind(['+', '-']) {
        Some(index) if text[index..].contains(':') && text[..index].contains('T') => {
            let offset = parse_offset(&text[index..])?;
            (&text[..index], offset)
        }
        _ => (text, 0),
    };
    let naive = naive.split('.').next().unwrap_or(naive);
    let mut fields = naive.split(['-', 'T', ':']);
    let year: i32 = fields.next()?.parse().ok()?;
    let month: i32 = fields.next()?.parse().ok()?;
    let day: i32 = fields.next()?.parse().ok()?;
    let hour: i32 = fields.next()?.parse().ok()?;
    let minute: i32 = fields.next()?.parse().ok()?;
    let second: i32 = fields.next()?.parse().ok()?;
    if fields.next().is_some() {
        return None;
    }
    let mut broken = unsafe { std::mem::zeroed::<libc::tm>() };
    broken.tm_year = year - 1900;
    broken.tm_mon = month - 1;
    broken.tm_mday = day;
    broken.tm_hour = hour;
    broken.tm_min = minute;
    broken.tm_sec = second;
    let epoch = unsafe { libc::timegm(&mut broken) };
    if epoch == -1 {
        return None;
    }
    Some(epoch - offset_secs)
}

fn parse_offset(text: &str) -> Option<i64> {
    let sign: i64 = if text.starts_with('-') { -1 } else { 1 };
    let mut parts = text[1..].split(':');
    let hours: i64 = parts.next()?.parse().ok()?;
    let minutes: i64 = parts.next().unwrap_or("0").parse().ok()?;
    if parts.next().is_some() || hours > 23 || minutes > 59 {
        return None;
    }
    Some(sign * (hours * 3600 + minutes * 60))
}

/// Prune trash entries older than `days`, deleting both the trashed file
/// and its `.trashinfo` metadata. Per-file failures are swallowed, exactly
/// like the Python loop. Returns the number of pruned entries.
pub fn prune_trash(home: &Path, days: i64, now_secs: i64) -> usize {
    let info_dir = home.join(".local/share/Trash/info");
    let files_dir = home.join(".local/share/Trash/files");
    if !info_dir.is_dir() {
        return 0;
    }
    let mut infos: Vec<PathBuf> = std::fs::read_dir(&info_dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.extension().is_some_and(|ext| ext == "trashinfo"))
                .collect()
        })
        .unwrap_or_default();
    infos.sort();
    let mut pruned = 0;
    for info in &infos {
        let old = std::fs::read_to_string(info)
            .ok()
            .and_then(|content| {
                content.lines().find_map(|line| {
                    line.split_once('=').and_then(|(key, value)| {
                        if key.trim() == "DeletionDate" {
                            parse_deletion_epoch(value)
                        } else {
                            None
                        }
                    })
                })
            })
            .is_some_and(|deleted| now_secs - deleted > days * 86400);
        if !old {
            continue;
        }
        if let Some(name) = info.file_stem() {
            let target = files_dir.join(name);
            if target.exists() {
                let is_real_dir =
                    std::fs::symlink_metadata(&target).is_ok_and(|meta| meta.file_type().is_dir() && !meta.file_type().is_symlink());
                if is_real_dir {
                    let _ = std::fs::remove_dir_all(&target);
                } else {
                    let _ = std::fs::remove_file(&target);
                }
            }
        }
        if std::fs::remove_file(info).is_ok() {
            pruned += 1;
        }
    }
    pruned
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn scans_only_bounded_non_symlink_targets() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("a/b/c/Steam/steamapps/compatdata");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(directory.path().join("a/b/c/Steam/steamapps/other")).unwrap();
        assert_eq!(find_dedupe_targets(directory.path()), vec![target]);
        assert!(supports_dedupe_filesystem(" BTRFS\n"));
        assert!(!supports_dedupe_filesystem("ext4"));
    }

    #[test]
    fn projects_nice_ionice_dedupe_argv_without_running_it() {
        let command = dedupe_command("/var/home/user/Steam/steamapps/shadercache", "/var/lib/kyth/abc.hash", true);
        assert_eq!(command, vec!["ionice", "-c3", "nice", "-n", "19", "duperemove", "-rdh", "--hashfile", "/var/lib/kyth/abc.hash", "/var/home/user/Steam/steamapps/shadercache"]);
    }

    #[test]
    fn projects_noninteractive_cleanup_commands() {
        assert_eq!(cleanup_flatpaks_command(), vec!["flatpak", "uninstall", "--unused", "-y", "--noninteractive"]);
        assert_eq!(vacuum_user_journal_command(30), vec!["journalctl", "--user", "--vacuum-time=30d"]);
    }

    #[test]
    fn parses_trash_deletion_dates_like_fromisoformat() {
        let naive = parse_deletion_epoch("2026-07-20T14:30:00").unwrap();
        assert_eq!(parse_deletion_epoch("2026-07-20T14:30:00.123").unwrap(), naive);
        assert_eq!(parse_deletion_epoch("2026-07-20T14:30:00Z").unwrap(), naive);
        assert_eq!(parse_deletion_epoch("2026-07-20T16:30:00+02:00").unwrap(), naive);
        assert_eq!(parse_deletion_epoch("2026-07-20T09:30:00-05:00").unwrap(), naive);
        assert!(parse_deletion_epoch("not a date").is_none());
        assert!(parse_deletion_epoch("2026-07-20").is_none());
    }

    #[test]
    fn prunes_only_expired_trash_entries() {
        let home = tempdir().unwrap();
        let info = home.path().join(".local/share/Trash/info");
        let files = home.path().join(".local/share/Trash/files");
        fs::create_dir_all(&info).unwrap();
        fs::create_dir_all(&files).unwrap();
        let old = "[Trash Info]\nPath=/tmp/old\nDeletionDate=2020-01-01T00:00:00\n";
        let fresh = "[Trash Info]\nPath=/tmp/fresh\nDeletionDate=2999-01-01T00:00:00\n";
        fs::write(info.join("old.trashinfo"), old).unwrap();
        fs::write(info.join("fresh.trashinfo"), fresh).unwrap();
        fs::write(info.join("nodate.trashinfo"), "[Trash Info]\nPath=/tmp/x\n").unwrap();
        fs::write(files.join("old"), "data").unwrap();
        fs::create_dir_all(files.join("gone-dir")).unwrap();
        fs::write(info.join("gone-dir.trashinfo"), old.replace("old", "gone-dir")).unwrap();
        let now = parse_deletion_epoch("2026-01-01T00:00:00").unwrap();
        assert_eq!(prune_trash(home.path(), 30, now), 2);
        assert!(!info.join("old.trashinfo").exists());
        assert!(!files.join("old").exists());
        assert!(!files.join("gone-dir").exists());
        assert!(info.join("fresh.trashinfo").exists());
        assert!(info.join("nodate.trashinfo").exists());
        assert_eq!(prune_trash(home.path(), 30, now), 0);
    }
}
