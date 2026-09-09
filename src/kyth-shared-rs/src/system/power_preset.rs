//! Offline governor/EPP power profiles.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerPreset {
    pub governor: String,
    pub epp: String,
}

fn defaults() -> BTreeMap<String, PowerPreset> {
    BTreeMap::from([
        (
            "balanced".into(),
            PowerPreset {
                governor: "schedutil".into(),
                epp: "balance_performance".into(),
            },
        ),
        (
            "powersave".into(),
            PowerPreset {
                governor: "powersave".into(),
                epp: "power".into(),
            },
        ),
    ])
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/power.toml");
        }
    }
    PathBuf::from("/etc/kyth/power.toml")
}

pub fn load(path: impl AsRef<Path>) -> BTreeMap<String, PowerPreset> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return defaults();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return defaults();
    };
    let Some(items) = value.get("profiles").and_then(toml::Value::as_table) else {
        return defaults();
    };
    let profiles: BTreeMap<_, _> = items
        .iter()
        .filter_map(|(name, value)| {
            let table = value.as_table()?;
            Some((
                name.clone(),
                PowerPreset {
                    governor: table
                        .get("governor")
                        .and_then(toml::Value::as_str)
                        .unwrap_or("schedutil")
                        .into(),
                    epp: table
                        .get("epp")
                        .and_then(toml::Value::as_str)
                        .unwrap_or("balance_performance")
                        .into(),
                },
            ))
        })
        .collect();
    if profiles.is_empty() {
        defaults()
    } else {
        profiles
    }
}

pub fn save(
    path: impl AsRef<Path>,
    profiles: &BTreeMap<String, PowerPreset>,
) -> std::io::Result<()> {
    let quote = |value: &str| toml::Value::String(value.to_string()).to_string();
    let mut lines = vec!["# Kyth power tuned per profile, offline".to_string()];
    for (name, preset) in profiles {
        lines.push(format!("[profiles.{}]", quote(name)));
        lines.push(format!("governor = {}", quote(&preset.governor)));
        lines.push(format!("epp = {}", quote(&preset.epp)));
        lines.push(String::new());
    }
    crate::atomic_io::atomic_write_text(path, &format!("{}\n", lines.join("\n")), Some(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_or_empty_profiles_use_python_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("power.toml");
        assert_eq!(load(&path), defaults());
        std::fs::write(&path, "[profiles]\n").unwrap();
        assert_eq!(load(&path), defaults());
    }
}
