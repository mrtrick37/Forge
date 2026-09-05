//! Installed-system configuration planning and durable writes.
//!
//! Passwords and account creation intentionally do not cross this model. The
//! native daemon validates and applies the non-secret configuration contract,
//! while the typed helper performs the same writes when called from a
//! compatibility boundary.

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

const MAX_FSTAB_LINE_BYTES: usize = 4096;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ConfigurationInput {
    pub target_root: String,
    pub hostname: String,
    pub timezone: String,
    #[serde(default = "default_locale")]
    pub locale: String,
    #[serde(default = "default_keymap")]
    pub keymap: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ConfigWrite {
    pub path: String,
    pub content: String,
    pub mode: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ConfigurationPlan {
    pub target_root: String,
    pub writes: Vec<ConfigWrite>,
    pub localtime_target: String,
    pub executor: &'static str,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct FstabAppendInput {
    pub path: String,
    pub line: String,
}

/// A support-safe snapshot used to roll back fstab changes made by storage
/// helpers when a later configuration operation fails.
#[derive(Debug)]
pub(crate) struct FstabSnapshot {
    path: String,
    content: Option<Vec<u8>>,
    mode: u32,
}

fn default_locale() -> String {
    "en_US.UTF-8".to_string()
}
fn default_keymap() -> String {
    "us".to_string()
}

fn safe_component(value: &str, label: &str, allow_slash: bool) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 4096 || value.contains("..") {
        return Err(format!("{label} is empty or unsafe."));
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(byte, b'.' | b'_' | b'@' | b'+' | b'-')
            || (allow_slash && byte == b'/')
    }) {
        return Err(format!("{label} contains unsupported characters."));
    }
    Ok(value.to_string())
}

fn safe_root(value: &str) -> Result<String, String> {
    let value = value.trim();
    if !value.starts_with('/') || value.contains("..") || value.len() > 4096 {
        return Err("target root must be an absolute safe path.".to_string());
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'+' | b':' | b'-')
    }) {
        return Err("target root contains unsupported characters.".to_string());
    }
    Ok(value.to_string())
}

fn safe_absolute_path(raw: &str, label: &str) -> Result<String, String> {
    let value = raw.trim();
    if value.is_empty()
        || value.len() > 4096
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

pub(crate) fn snapshot_fstab(raw: &str) -> Result<FstabSnapshot, String> {
    let path = safe_absolute_path(raw, "fstab path")?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err("fstab path must not be a symlink".to_string())
        }
        Ok(metadata) if !metadata.is_file() => Err("fstab path must be a regular file".to_string()),
        Ok(metadata) => Ok(FstabSnapshot {
            content: Some(
                fs::read(&path).map_err(|error| format!("could not snapshot fstab: {error}"))?,
            ),
            path,
            mode: metadata.permissions().mode() & 0o777,
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(FstabSnapshot {
            path,
            content: None,
            mode: 0o644,
        }),
        Err(error) => Err(format!("could not inspect fstab: {error}")),
    }
}

pub(crate) fn restore_fstab(snapshot: FstabSnapshot) -> Result<(), String> {
    let path = Path::new(&snapshot.path);
    let parent = path
        .parent()
        .ok_or_else(|| "fstab path has no parent directory".to_string())?;
    let parent_metadata = fs::symlink_metadata(parent)
        .map_err(|error| format!("could not inspect fstab directory: {error}"))?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        return Err("fstab parent must be a real directory".to_string());
    }

    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.is_dir() {
            return Err("cannot roll back fstab over a directory".to_string());
        }
        fs::remove_file(path)
            .map_err(|error| format!("could not remove changed fstab: {error}"))?;
    }

    if let Some(content) = snapshot.content {
        let temporary = path.with_extension("kyth-rollback.tmp");
        let mut file = OpenOptions::new();
        file.write(true)
            .create_new(true)
            .mode(snapshot.mode)
            .custom_flags(libc::O_NOFOLLOW);
        let mut file = file
            .open(&temporary)
            .map_err(|error| format!("could not create fstab rollback: {error}"))?;
        file.write_all(&content)
            .map_err(|error| format!("could not write fstab rollback: {error}"))?;
        file.flush()
            .map_err(|error| format!("could not flush fstab rollback: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("could not sync fstab rollback: {error}"))?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(snapshot.mode))
            .map_err(|error| format!("could not secure fstab rollback: {error}"))?;
        fs::rename(&temporary, path)
            .map_err(|error| format!("could not install fstab rollback: {error}"))?;
    }
    OpenOptions::new()
        .read(true)
        .open(parent)
        .map_err(|error| format!("could not open fstab directory: {error}"))?
        .sync_all()
        .map_err(|error| format!("could not sync fstab directory: {error}"))
}

pub(crate) fn build_plan(input: ConfigurationInput) -> Result<ConfigurationPlan, String> {
    let target_root = safe_root(&input.target_root)?;
    let hostname = safe_component(&input.hostname, "hostname", false)?;
    if hostname.starts_with('-') || hostname.ends_with('-') {
        return Err("hostname cannot start or end with '-'.".to_string());
    }
    let timezone = safe_component(&input.timezone, "timezone", true)?;
    if timezone.starts_with('/') || timezone.ends_with('/') || timezone.contains("//") {
        return Err("timezone must be a relative zoneinfo path.".to_string());
    }
    let locale = safe_component(&input.locale, "locale", false)?;
    let keymap = safe_component(&input.keymap, "keymap", false)?;
    let etc = format!("{target_root}/etc");
    Ok(ConfigurationPlan {
        target_root,
        writes: vec![
            ConfigWrite {
                path: format!("{etc}/hostname"),
                content: format!("{hostname}\n"),
                mode: 0o644,
            },
            ConfigWrite {
                path: format!("{etc}/locale.conf"),
                content: format!("LANG={locale}\n"),
                mode: 0o644,
            },
            ConfigWrite {
                path: format!("{etc}/vconsole.conf"),
                content: format!("KEYMAP={keymap}\n"),
                mode: 0o644,
            },
        ],
        localtime_target: format!("/usr/share/zoneinfo/{timezone}"),
        executor: "kyth-installer-exec",
    })
}

pub(crate) fn apply_plan(plan: ConfigurationPlan) -> Result<(), String> {
    for write in &plan.writes {
        let mut file = OpenOptions::new();
        file.write(true)
            .create(true)
            .truncate(true)
            .mode(write.mode)
            .custom_flags(libc::O_NOFOLLOW);
        let mut file = file
            .open(&write.path)
            .map_err(|error| format!("could not open configuration file: {error}"))?;
        file.write_all(write.content.as_bytes())
            .map_err(|error| format!("could not write configuration file: {error}"))?;
        file.flush()
            .map_err(|error| format!("could not flush configuration file: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("could not sync configuration file: {error}"))?;
        fs::set_permissions(&write.path, fs::Permissions::from_mode(write.mode))
            .map_err(|error| format!("could not secure configuration file: {error}"))?;
    }

    let localtime = Path::new(&plan.target_root).join("etc/localtime");
    if let Ok(metadata) = fs::symlink_metadata(&localtime) {
        if metadata.is_dir() {
            return Err("installed localtime path is a directory".to_string());
        }
        fs::remove_file(&localtime)
            .map_err(|error| format!("could not replace installed localtime: {error}"))?;
    }
    std::os::unix::fs::symlink(&plan.localtime_target, &localtime)
        .map_err(|error| format!("could not set installed timezone: {error}"))?;
    Ok(())
}

fn safe_fstab_path(raw: &str) -> Result<String, String> {
    let path = safe_root(raw)?;
    if !path.ends_with("/etc/fstab") {
        return Err("fstab path must point to an installed /etc/fstab".to_string());
    }
    Ok(path)
}

fn validate_fstab_line(line: &str) -> Result<(), String> {
    if line.is_empty() || line.len() > MAX_FSTAB_LINE_BYTES || !line.ends_with('\n') {
        return Err("fstab entry must be one bounded line".to_string());
    }
    let content = line.strip_suffix('\n').unwrap_or(line);
    if content.bytes().any(|byte| byte.is_ascii_control()) {
        return Err("fstab entry contains control characters".to_string());
    }
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() != 6 || !fields[0].starts_with("UUID=") {
        return Err("fstab entry has an unsupported shape".to_string());
    }
    if fields[0].len() <= "UUID=".len()
        || !fields[0]["UUID=".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        || (fields[1] != "none" && safe_absolute_path(fields[1], "fstab mount point").is_err())
        || !fields[2]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        || !fields[3].bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'=' | b',' | b':' | b'.' | b'_' | b'+' | b'@' | b'-')
        })
        || fields[4] != "0"
        || !matches!(fields[5], "0" | "2")
    {
        return Err("fstab entry contains unsupported values".to_string());
    }
    Ok(())
}

pub(crate) fn append_fstab(input: FstabAppendInput) -> Result<(), String> {
    let path = safe_fstab_path(&input.path)?;
    validate_fstab_line(&input.line)?;
    let mut file = OpenOptions::new();
    file.create(true)
        .append(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o644);
    let mut file = file
        .open(&path)
        .map_err(|error| format!("could not open installed fstab: {error}"))?;
    file.write_all(input.line.as_bytes())
        .map_err(|error| format!("could not append installed fstab: {error}"))?;
    file.flush()
        .map_err(|error| format!("could not flush installed fstab: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("could not sync installed fstab: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_non_secret_installed_configuration() {
        let plan = build_plan(ConfigurationInput {
            target_root: "/mnt/target".to_string(),
            hostname: "kyth-box".to_string(),
            timezone: "Europe/Berlin".to_string(),
            locale: "en_US.UTF-8".to_string(),
            keymap: "us".to_string(),
        })
        .expect("configuration should validate");
        assert_eq!(plan.writes.len(), 3);
        assert!(plan
            .writes
            .iter()
            .any(|write| write.content == "kyth-box\n"));
        assert_eq!(plan.localtime_target, "/usr/share/zoneinfo/Europe/Berlin");
    }

    #[test]
    fn rejects_path_traversal_and_invalid_identity_values() {
        let base = ConfigurationInput {
            target_root: "/mnt/target".to_string(),
            hostname: "kyth".to_string(),
            timezone: "UTC".to_string(),
            locale: "en_US.UTF-8".to_string(),
            keymap: "us".to_string(),
        };
        assert!(build_plan(ConfigurationInput {
            target_root: "/mnt/../etc".to_string(),
            ..base.clone()
        })
        .is_err());
        assert!(build_plan(ConfigurationInput {
            hostname: "bad name".to_string(),
            ..base.clone()
        })
        .is_err());
        assert!(build_plan(ConfigurationInput {
            timezone: "../UTC".to_string(),
            ..base
        })
        .is_err());
    }

    #[test]
    fn applies_non_secret_configuration_files_and_timezone_link() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let etc = directory.path().join("etc");
        std::fs::create_dir(&etc).expect("etc directory");
        let plan = build_plan(ConfigurationInput {
            target_root: directory.path().to_string_lossy().into_owned(),
            hostname: "kyth-box".to_string(),
            timezone: "UTC".to_string(),
            locale: "en_US.UTF-8".to_string(),
            keymap: "us".to_string(),
        })
        .expect("configuration should validate");
        apply_plan(plan).expect("configuration should apply");
        assert_eq!(
            std::fs::read_to_string(etc.join("hostname")).unwrap(),
            "kyth-box\n"
        );
        assert_eq!(
            std::fs::read_link(etc.join("localtime")).unwrap(),
            Path::new("/usr/share/zoneinfo/UTC")
        );
    }

    #[test]
    fn validates_and_appends_one_safe_fstab_entry() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let etc = directory.path().join("etc");
        std::fs::create_dir(&etc).expect("etc directory");
        let path = etc.join("fstab");
        append_fstab(FstabAppendInput {
            path: path.to_string_lossy().into_owned(),
            line: "UUID=ABCD-1234 /var/home btrfs subvol=@home,compress=zstd:1 0 0\n".into(),
        })
        .expect("fstab entry should append");
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "UUID=ABCD-1234 /var/home btrfs subvol=@home,compress=zstd:1 0 0\n"
        );
        assert!(append_fstab(FstabAppendInput {
            path: etc.join("not-fstab").to_string_lossy().into_owned(),
            line: "UUID=ABCD /data ext4 defaults 0 2\n".into(),
        })
        .is_err());
    }

    #[test]
    fn snapshots_and_restores_existing_fstab_atomically() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let etc = directory.path().join("etc");
        std::fs::create_dir(&etc).expect("etc directory");
        let path = etc.join("fstab");
        std::fs::write(&path, b"old fstab\n").expect("initial fstab");
        let snapshot = snapshot_fstab(&path.to_string_lossy()).expect("fstab should snapshot");
        std::fs::write(&path, b"partially changed\n").expect("changed fstab");
        restore_fstab(snapshot).expect("fstab should restore");
        assert_eq!(std::fs::read(&path).unwrap(), b"old fstab\n");
    }

    #[test]
    fn restoring_absent_fstab_removes_changes_and_rejects_symlinks() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let etc = directory.path().join("etc");
        std::fs::create_dir(&etc).expect("etc directory");
        let path = etc.join("fstab");
        let snapshot = snapshot_fstab(&path.to_string_lossy()).expect("absence should snapshot");
        std::fs::write(&path, b"created by failed phase\n").expect("changed fstab");
        restore_fstab(snapshot).expect("new fstab should be removed");
        assert!(!path.exists());

        std::os::unix::fs::symlink("/etc/passwd", &path).expect("symlink");
        assert!(snapshot_fstab(&path.to_string_lossy()).is_err());
    }
}
