//! Offline Btrfs autotune configuration and script rendering.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BtrfsAutotuneConfig {
    pub enabled: bool,
    pub threshold: i64,
}

impl Default for BtrfsAutotuneConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold: 80,
        }
    }
}

fn clamp(config: BtrfsAutotuneConfig) -> BtrfsAutotuneConfig {
    BtrfsAutotuneConfig {
        threshold: config.threshold.clamp(50, 95),
        ..config
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/btrfs-autotune.toml");
        }
    }
    PathBuf::from("/etc/kyth/btrfs-autotune.toml")
}

pub fn load(path: impl AsRef<Path>) -> BtrfsAutotuneConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return BtrfsAutotuneConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return BtrfsAutotuneConfig::default();
    };
    clamp(BtrfsAutotuneConfig {
        enabled: value
            .get("enabled")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true),
        threshold: value
            .get("threshold")
            .and_then(toml::Value::as_integer)
            .unwrap_or(80),
    })
}

pub fn save(path: impl AsRef<Path>, config: BtrfsAutotuneConfig) -> std::io::Result<()> {
    let config = clamp(config);
    let text = format!(
        "# Kyth btrfs autotune — offline\nenabled = {}\nthreshold = {}\n",
        config.enabled, config.threshold
    );
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

pub fn generate(
    config: BtrfsAutotuneConfig,
    script: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let config = clamp(config);
    let script = script.as_ref();
    if !config.enabled {
        match std::fs::remove_file(script) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        return Ok(None);
    }
    let content = format!(
        r##"#!/usr/bin/env bash
set -euo pipefail
# Kyth btrfs autotune — generated, runs weekly
th={}
for mp in / /var; do
  [[ -d $mp ]] || continue
  fst=$(findmnt -no FSTYPE -T $mp 2>/dev/null || echo "")
  [[ $fst == btrfs ]] || continue
  used=$(btrfs filesystem usage -b $mp 2>/dev/null | awk '/Used:/ {{print $2}}' | head -n1 || echo 0)
  total=$(btrfs filesystem usage -b $mp 2>/dev/null | awk '/Device size:/ {{print $4}}' | head -n1 || echo 0)
  if [[ $total -gt 0 ]]; then
    pct=$(( used * 100 / total ))
    if (( pct > th )); then
      btrfs balance start -dusage=50 -musage=50 $mp 2>&1 | logger -t kyth-btrfs-autotune || true
      btrfs filesystem defragment -r -czstd $mp 2>&1 | logger -t kyth-btrfs-autotune || true
    fi
  fi
done
"##,
        config.threshold
    );
    crate::atomic_io::atomic_write_text(script, &content, Some(0o755))?;
    Ok(Some(script.to_path_buf()))
}

pub fn status(script: impl AsRef<Path>) -> &'static str {
    if script.as_ref().is_file() {
        "enabled"
    } else {
        "off"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clamps_threshold_and_renders_script() {
        let directory = tempdir().unwrap();
        let config_path = directory.path().join("btrfs-autotune.toml");
        let script = directory.path().join("kyth-btrfs-autotune");
        save(
            &config_path,
            BtrfsAutotuneConfig {
                enabled: true,
                threshold: 1,
            },
        )
        .unwrap();
        let config = load(&config_path);
        assert_eq!(config.threshold, 50);
        generate(config, &script).unwrap();
        assert!(std::fs::read_to_string(script).unwrap().contains("th=50"));
    }

    #[test]
    fn disabled_removes_generated_script() {
        let directory = tempdir().unwrap();
        let script = directory.path().join("script");
        std::fs::write(&script, "generated").unwrap();
        assert_eq!(
            generate(
                BtrfsAutotuneConfig {
                    enabled: false,
                    threshold: 80
                },
                &script
            )
            .unwrap(),
            None
        );
        assert_eq!(status(&script), "off");
    }
}
