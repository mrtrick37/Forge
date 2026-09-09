//! Pure, side-effect-free disk argument normalization helpers.

use std::path::{Component, Path, PathBuf};

pub fn safe_int(value: Option<&str>, default: i64) -> i64 {
    value
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(default)
}

fn allowed_device_path(path: &Path) -> bool {
    if !path.is_absolute() || !path.starts_with("/dev") {
        return false;
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return false;
    }
    path.to_string_lossy()
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"._/+:-".contains(&byte))
}

/// Normalize a user/device-supplied name without accepting arbitrary paths.
/// Existing symlinks are canonicalized, matching Python's `realpath` behavior.
pub fn normalize_device_path(name: Option<&str>) -> Option<PathBuf> {
    let name = name?.trim();
    if name.is_empty() {
        return None;
    }
    let requested = if name.starts_with("/dev/") {
        PathBuf::from(name)
    } else {
        PathBuf::from("/dev").join(name)
    };
    let normalized = std::fs::canonicalize(&requested).unwrap_or(requested);
    allowed_device_path(&normalized).then_some(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_safe_integers() {
        assert_eq!(safe_int(Some("42"), 0), 42);
        assert_eq!(safe_int(Some(""), 7), 7);
        assert_eq!(safe_int(Some("nope"), 7), 7);
    }

    #[test]
    fn accepts_device_names_and_rejects_path_escape() {
        assert_eq!(
            normalize_device_path(Some("sda")),
            Some(PathBuf::from("/dev/sda"))
        );
        assert_eq!(
            normalize_device_path(Some("/dev/nvme0n1")),
            Some(PathBuf::from("/dev/nvme0n1"))
        );
        assert!(normalize_device_path(Some("/tmp/disk")).is_none());
        assert!(normalize_device_path(Some("/dev/../../etc/passwd")).is_none());
        assert!(normalize_device_path(Some("/dev/disk with spaces")).is_none());
    }
}
