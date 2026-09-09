//! Offline zswap preference and explicit kernel drop-in rendering.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZswapConfig {
    pub profile: String,
    pub compressor: String,
    pub zpool: String,
}

impl Default for ZswapConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            compressor: "zstd".into(),
            zpool: "zsmalloc".into(),
        }
    }
}

fn normalize(config: ZswapConfig) -> ZswapConfig {
    ZswapConfig {
        profile: if config.profile == "kyth" {
            "kyth".into()
        } else {
            "balanced".into()
        },
        compressor: match config.compressor.as_str() {
            "lz4" | "lzo" => config.compressor,
            _ => "zstd".into(),
        },
        zpool: match config.zpool.as_str() {
            "zbud" | "z3fold" => config.zpool,
            _ => "zsmalloc".into(),
        },
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/zswap.toml");
        }
    }
    PathBuf::from("/etc/kyth/zswap.toml")
}

pub fn load(path: impl AsRef<Path>) -> ZswapConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return ZswapConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return ZswapConfig::default();
    };
    normalize(ZswapConfig {
        profile: value
            .get("profile")
            .and_then(toml::Value::as_str)
            .unwrap_or("balanced")
            .into(),
        compressor: value
            .get("compressor")
            .and_then(toml::Value::as_str)
            .unwrap_or("zstd")
            .into(),
        zpool: value
            .get("zpool")
            .and_then(toml::Value::as_str)
            .unwrap_or("zsmalloc")
            .into(),
    })
}

pub fn save(path: impl AsRef<Path>, config: &ZswapConfig) -> std::io::Result<()> {
    let config = normalize(config.clone());
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth zswap — offline\nprofile = {:?}\ncompressor = {:?}\nzpool = {:?}\n",
            config.profile, config.compressor, config.zpool
        ),
        Some(0o600),
    )
}

pub fn generate(
    config: &ZswapConfig,
    sysctl_path: impl AsRef<Path>,
    modprobe_path: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let config = normalize(config.clone());
    let sysctl_path = sysctl_path.as_ref();
    let modprobe_path = modprobe_path.as_ref();
    if config.profile != "kyth" {
        for path in [sysctl_path, modprobe_path] {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        return Ok(None);
    }
    crate::atomic_io::atomic_write_text(sysctl_path, &format!("# Kyth zswap — generated\nvm.zswap_enabled = 1\nvm.zswap_compressor = {}\nvm.zswap_zpool = {}\n", config.compressor, config.zpool), Some(0o644))?;
    crate::atomic_io::atomic_write_text(
        modprobe_path,
        &format!(
            "# Kyth zswap — generated\noptions zswap enabled=1 compressor={} zpool={}\n",
            config.compressor, config.zpool
        ),
        Some(0o644),
    )?;
    Ok(Some(sysctl_path.to_path_buf()))
}

pub fn status(sysctl_path: impl AsRef<Path>) -> &'static str {
    if sysctl_path.as_ref().is_file() {
        "kyth"
    } else {
        "balanced"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn normalizes_zswap_choices() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("zswap.toml");
        std::fs::write(
            &path,
            "profile = \"kyth\"\ncompressor = \"bad\"\nzpool = \"z3fold\"\n",
        )
        .unwrap();
        assert_eq!(
            load(&path),
            ZswapConfig {
                profile: "kyth".into(),
                compressor: "zstd".into(),
                zpool: "z3fold".into()
            }
        );
    }

    #[test]
    fn renders_both_drop_ins_and_cleans_them() {
        let directory = tempdir().unwrap();
        let sysctl = directory.path().join("zswap.conf");
        let modprobe = directory.path().join("zswap.mod");
        let config = ZswapConfig {
            profile: "kyth".into(),
            compressor: "lz4".into(),
            zpool: "zbud".into(),
        };
        generate(&config, &sysctl, &modprobe).unwrap();
        assert!(std::fs::read_to_string(&modprobe)
            .unwrap()
            .contains("compressor=lz4 zpool=zbud"));
        generate(&ZswapConfig::default(), &sysctl, &modprobe).unwrap();
        assert!(!sysctl.exists() && !modprobe.exists());
    }
}
