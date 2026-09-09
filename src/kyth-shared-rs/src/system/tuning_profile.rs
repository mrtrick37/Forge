//! Shared profile configuration for the small Kyth tuning modules.
//!
//! Many Python modules differ only by their config filename and generated
//! sysctl payload. This ports the common offline profile behavior while
//! leaving privileged sysctl application to the existing service boundary.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Balanced,
    Gaming,
}

impl Profile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::Gaming => "gaming",
        }
    }
}

pub fn profile_from_str(value: Option<&str>) -> Profile {
    match value
        .unwrap_or("balanced")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "gaming" => Profile::Gaming,
        _ => Profile::Balanced,
    }
}

pub fn load_profile(path: impl AsRef<Path>) -> Profile {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Profile::Balanced;
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return Profile::Balanced;
    };
    profile_from_str(value.get("profile").and_then(toml::Value::as_str))
}

pub fn save_profile(
    path: impl AsRef<Path>,
    comment: &str,
    profile: Profile,
) -> std::io::Result<()> {
    let text = format!(
        "# {comment} — offline\nprofile = \"{}\"\n",
        profile.as_str()
    );
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

pub fn status_from_conf(path: impl AsRef<Path>) -> Profile {
    if path.as_ref().is_file() {
        Profile::Gaming
    } else {
        Profile::Balanced
    }
}

pub fn config_path(
    path: Option<impl AsRef<Path>>,
    default_path: impl AsRef<Path>,
    test_filename: &str,
) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth").join(test_filename);
        }
    }
    default_path.as_ref().to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn normalizes_profiles_and_round_trips_config() {
        assert_eq!(profile_from_str(Some(" GAMING ")), Profile::Gaming);
        assert_eq!(profile_from_str(Some("unknown")), Profile::Balanced);
        let directory = tempdir().unwrap();
        let path = directory.path().join("profile.toml");
        save_profile(&path, "Kyth tuning", Profile::Gaming).unwrap();
        assert_eq!(load_profile(&path), Profile::Gaming);
        assert_eq!(status_from_conf(&path), Profile::Gaming);
        fs::remove_file(&path).unwrap();
        assert_eq!(status_from_conf(&path), Profile::Balanced);
    }
}
