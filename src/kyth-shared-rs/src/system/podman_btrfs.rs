//! Offline Podman storage-driver preference and drop-in rendering.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PodmanMode {
    Auto,
    Btrfs,
    Overlay,
    Off,
}

impl PodmanMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Btrfs => "btrfs",
            Self::Overlay => "overlay",
            Self::Off => "off",
        }
    }
}

pub fn parse(value: Option<&str>) -> PodmanMode {
    match value.unwrap_or("auto").to_ascii_lowercase().as_str() {
        "btrfs" => PodmanMode::Btrfs,
        "overlay" => PodmanMode::Overlay,
        "off" => PodmanMode::Off,
        _ => PodmanMode::Auto,
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/podman-btrfs.toml");
        }
    }
    PathBuf::from("/etc/kyth/podman-btrfs.toml")
}

pub fn load(path: impl AsRef<Path>) -> PodmanMode {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return PodmanMode::Auto;
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return PodmanMode::Auto;
    };
    parse(value.get("mode").and_then(toml::Value::as_str))
}

pub fn save(path: impl AsRef<Path>, mode: PodmanMode) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth podman btrfs — offline\nmode = {:?}\n",
            mode.as_str()
        ),
        Some(0o600),
    )
}

pub fn resolve(mode: PodmanMode, on_btrfs: bool) -> PodmanMode {
    match mode {
        PodmanMode::Auto => {
            if on_btrfs {
                PodmanMode::Btrfs
            } else {
                PodmanMode::Overlay
            }
        }
        other => other,
    }
}

pub fn generate(
    mode: PodmanMode,
    on_btrfs: bool,
    destination: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let destination = destination.as_ref();
    if resolve(mode, on_btrfs) != PodmanMode::Btrfs {
        match std::fs::remove_file(destination) {
            Ok(()) | Err(_) => {}
        }
        return Ok(None);
    }
    crate::atomic_io::atomic_write_text(
        destination,
        "# Kyth podman btrfs — generated\n[storage]\ndriver = \"btrfs\"\n",
        Some(0o644),
    )?;
    Ok(Some(destination.to_path_buf()))
}

pub fn status(destination: impl AsRef<Path>, on_btrfs: bool) -> &'static str {
    if destination.as_ref().is_file() {
        "btrfs"
    } else if on_btrfs {
        "overlay (auto)"
    } else {
        "overlay"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolves_auto_and_reversibly_renders_driver() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("podman.conf");
        assert_eq!(resolve(PodmanMode::Auto, true), PodmanMode::Btrfs);
        generate(PodmanMode::Btrfs, false, &path).unwrap();
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("driver = \"btrfs\""));
        generate(PodmanMode::Overlay, true, &path).unwrap();
        assert!(!path.exists());
    }
}
