//! Native post-configuration assurance for an installed target.
//!
//! These checks are deliberately read-only and support-safe. They run after
//! the typed configuration/account operations and before the native executor
//! records configure_complete, so a partially configured target cannot be
//! reported as successful.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_TARGET_ROOT_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct AssuranceCheck {
    pub name: String,
    pub status: String,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub(crate) struct AssuranceInput {
    pub target_root: String,
    pub hostname: String,
    pub locale: String,
    pub keymap: String,
    pub timezone: String,
    pub username: String,
}

fn safe_target_root(raw: &str) -> Result<PathBuf, String> {
    let value = raw.trim();
    if value.is_empty()
        || value.len() > MAX_TARGET_ROOT_BYTES
        || !value.starts_with('/')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'+' | b':' | b'-')
        })
    {
        return Err("installed target root is not a safe absolute path".to_string());
    }
    Ok(PathBuf::from(value))
}

fn regular_file(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect installed {label}: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("installed {label} is not a regular file"));
    }
    Ok(())
}

fn real_directory(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect installed {label}: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!("installed {label} is not a real directory"));
    }
    Ok(())
}

fn contains_real_entry(path: &Path) -> bool {
    fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| {
            fs::symlink_metadata(entry.path())
                .map(|metadata| !metadata.file_type().is_symlink())
                .unwrap_or(false)
        })
}

fn has_loader_entry(path: &Path) -> bool {
    fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "conf")
                && fs::symlink_metadata(entry.path())
                    .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
                    .unwrap_or(false)
        })
}

fn boot_metadata_reason(root: &Path) -> Option<&'static str> {
    if has_loader_entry(&root.join("boot/loader/entries")) {
        return Some("boot loader entries are present");
    }
    if contains_real_entry(&root.join("boot/efi/EFI")) {
        return Some("EFI boot files are present");
    }
    if contains_real_entry(&root.join("ostree/deploy")) {
        return Some("ostree deployment metadata is present");
    }
    None
}

pub(crate) fn validate(input: AssuranceInput) -> Result<Vec<AssuranceCheck>, String> {
    let root = safe_target_root(&input.target_root)?;
    real_directory(&root, "target root")?;
    let etc = root.join("etc");
    real_directory(&etc, "/etc tree")?;

    let hostname_path = etc.join("hostname");
    regular_file(&hostname_path, "hostname")?;
    let installed_hostname = fs::read_to_string(&hostname_path)
        .map_err(|error| format!("could not read installed hostname: {error}"))?;
    if installed_hostname.trim() != input.hostname.trim() {
        return Err(format!(
            "installed hostname verification failed: expected {:?}",
            input.hostname.trim()
        ));
    }

    let locale_path = etc.join("locale.conf");
    regular_file(&locale_path, "locale configuration")?;
    let locale = fs::read_to_string(&locale_path)
        .map_err(|error| format!("could not read installed locale: {error}"))?;
    if locale.trim() != format!("LANG={}", input.locale.trim()) {
        return Err("installed locale verification failed".to_string());
    }

    let keymap_path = etc.join("vconsole.conf");
    regular_file(&keymap_path, "console keymap configuration")?;
    let keymap = fs::read_to_string(&keymap_path)
        .map_err(|error| format!("could not read installed keymap: {error}"))?;
    if keymap.trim() != format!("KEYMAP={}", input.keymap.trim()) {
        return Err("installed keymap verification failed".to_string());
    }

    let localtime = etc.join("localtime");
    let localtime_metadata = fs::symlink_metadata(&localtime)
        .map_err(|error| format!("could not inspect installed timezone link: {error}"))?;
    if !localtime_metadata.file_type().is_symlink() {
        return Err("installed timezone is not a symlink".to_string());
    }
    let expected_timezone = Path::new("/usr/share/zoneinfo").join(input.timezone.trim());
    if fs::read_link(&localtime)
        .map_err(|error| format!("could not read installed timezone link: {error}"))?
        != expected_timezone
    {
        return Err("installed timezone verification failed".to_string());
    }

    if !input.username.trim().is_empty() {
        let passwd_path = etc.join("passwd");
        regular_file(&passwd_path, "passwd database")?;
        let passwd = fs::read_to_string(&passwd_path)
            .map_err(|error| format!("could not read installed account database: {error}"))?;
        if !passwd
            .lines()
            .filter_map(|line| line.split_once(':'))
            .any(|(name, _)| name == input.username.trim())
        {
            return Err(format!(
                "installed account {:?} was not created",
                input.username.trim()
            ));
        }
    }

    let fstab = etc.join("fstab");
    regular_file(&fstab, "fstab")?;
    let reason = boot_metadata_reason(&root).ok_or_else(|| {
        "installed system has no boot metadata (loader entries, EFI files, or ostree deployment)"
            .to_string()
    })?;

    Ok(vec![
        AssuranceCheck {
            name: "hostname".to_string(),
            status: "pass".to_string(),
            detail: installed_hostname.trim().to_string(),
        },
        AssuranceCheck {
            name: "locale".to_string(),
            status: "pass".to_string(),
            detail: input.locale.trim().to_string(),
        },
        AssuranceCheck {
            name: "keymap".to_string(),
            status: "pass".to_string(),
            detail: input.keymap.trim().to_string(),
        },
        AssuranceCheck {
            name: "timezone".to_string(),
            status: "pass".to_string(),
            detail: input.timezone.trim().to_string(),
        },
        AssuranceCheck {
            name: "account".to_string(),
            status: "pass".to_string(),
            detail: if input.username.trim().is_empty() {
                "no account requested".to_string()
            } else {
                input.username.trim().to_string()
            },
        },
        AssuranceCheck {
            name: "filesystem".to_string(),
            status: "pass".to_string(),
            detail: "Installed fstab is present".to_string(),
        },
        AssuranceCheck {
            name: "bootloader".to_string(),
            status: "pass".to_string(),
            detail: reason.to_string(),
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(root: &Path) -> AssuranceInput {
        AssuranceInput {
            target_root: root.to_string_lossy().into_owned(),
            hostname: "kyth-box".to_string(),
            locale: "en_US.UTF-8".to_string(),
            keymap: "us".to_string(),
            timezone: "UTC".to_string(),
            username: "alice".to_string(),
        }
    }

    fn target_fixture() -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("temporary target");
        let root = directory.path();
        let etc = root.join("etc");
        std::fs::create_dir_all(&etc).unwrap();
        std::fs::write(etc.join("hostname"), "kyth-box\n").unwrap();
        std::fs::write(etc.join("locale.conf"), "LANG=en_US.UTF-8\n").unwrap();
        std::fs::write(etc.join("vconsole.conf"), "KEYMAP=us\n").unwrap();
        std::fs::write(
            etc.join("passwd"),
            "alice:x:1000:1000::/home/alice:/bin/bash\n",
        )
        .unwrap();
        std::fs::write(etc.join("fstab"), "# generated\n").unwrap();
        std::os::unix::fs::symlink("/usr/share/zoneinfo/UTC", etc.join("localtime")).unwrap();
        std::fs::create_dir_all(root.join("ostree/deploy/default")).unwrap();
        directory
    }

    #[test]
    fn validates_identity_account_filesystem_and_boot_metadata() {
        let directory = target_fixture();
        let checks = validate(input(directory.path())).expect("target should pass assurance");
        assert_eq!(checks.len(), 7);
        assert!(checks.iter().all(|check| check.status == "pass"));
    }

    #[test]
    fn rejects_missing_boot_metadata_and_path_traversal() {
        let directory = target_fixture();
        std::fs::remove_dir_all(directory.path().join("ostree")).unwrap();
        assert!(validate(input(directory.path()))
            .expect_err("boot metadata is required")
            .contains("boot metadata"));
        let mut unsafe_input = input(directory.path());
        unsafe_input.target_root = "/tmp/../etc".to_string();
        assert!(validate(unsafe_input).is_err());
    }
}
