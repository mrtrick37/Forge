//! Offline Podman overlay metacopy preference and drop-in rendering.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metacopy {
    Auto,
    On,
    Off,
}

impl Metacopy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::On => "on",
            Self::Off => "off",
        }
    }
}

pub fn parse(value: Option<&str>) -> Metacopy {
    match value.unwrap_or("auto").to_ascii_lowercase().as_str() {
        "on" => Metacopy::On,
        "off" => Metacopy::Off,
        _ => Metacopy::Auto,
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/podman-overlay.toml");
        }
    }
    PathBuf::from("/etc/kyth/podman-overlay.toml")
}

pub fn load(path: impl AsRef<Path>) -> Metacopy {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Metacopy::Auto;
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return Metacopy::Auto;
    };
    parse(value.get("metacopy").and_then(toml::Value::as_str))
}

pub fn save(path: impl AsRef<Path>, value: Metacopy) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth overlay — offline\nmetacopy = {:?}\n",
            value.as_str()
        ),
        Some(0o600),
    )
}

pub fn resolve(value: Metacopy, on_btrfs: bool) -> Metacopy {
    if value == Metacopy::Auto {
        if on_btrfs {
            Metacopy::On
        } else {
            Metacopy::Off
        }
    } else {
        value
    }
}

pub fn generate(
    value: Metacopy,
    on_btrfs: bool,
    destination: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let destination = destination.as_ref();
    if resolve(value, on_btrfs) == Metacopy::Off {
        match std::fs::remove_file(destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        return Ok(None);
    }
    let content = "# Kyth overlay metacopy — generated\n[storage.options.overlay]\nmountopt = \"metacopy=on\"\n";
    crate::atomic_io::atomic_write_text(destination, content, Some(0o644))?;
    Ok(Some(destination.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn auto_follows_filesystem_capability() {
        assert_eq!(resolve(Metacopy::Auto, true), Metacopy::On);
        assert_eq!(resolve(Metacopy::Auto, false), Metacopy::Off);
        assert_eq!(parse(Some("unknown")), Metacopy::Auto);
    }

    #[test]
    fn renders_and_removes_drop_in() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("overlay.conf");
        generate(Metacopy::On, false, &path).unwrap();
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("metacopy=on"));
        generate(Metacopy::Off, false, &path).unwrap();
        assert!(!path.exists());
    }
}
