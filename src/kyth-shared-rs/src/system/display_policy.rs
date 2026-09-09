//! Offline VRR and night-colour preference model.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VrrOutput {
    pub vrr: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NightColor {
    pub enabled: bool,
    pub temperature: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VrrConfig {
    pub outputs: BTreeMap<String, VrrOutput>,
    pub night: NightColor,
}
impl Default for VrrConfig {
    fn default() -> Self {
        Self {
            outputs: BTreeMap::new(),
            night: NightColor {
                enabled: false,
                temperature: 4500,
            },
        }
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    PathBuf::from(
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
                    .join(".config")
            }),
    )
    .join("kyth/vrr.toml")
}
fn valid_vrr(value: &str) -> bool {
    matches!(value, "never" | "adaptive" | "always")
}
pub fn load(path: impl AsRef<Path>) -> VrrConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return VrrConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return VrrConfig::default();
    };
    let outputs = value
        .get("outputs")
        .and_then(toml::Value::as_table)
        .map(|items| {
            items
                .iter()
                .filter_map(|(name, value)| {
                    let table = value.as_table()?;
                    let vrr = table
                        .get("vrr")
                        .and_then(toml::Value::as_str)
                        .unwrap_or("adaptive");
                    Some((
                        name.clone(),
                        VrrOutput {
                            vrr: if valid_vrr(vrr) {
                                vrr.into()
                            } else {
                                "adaptive".into()
                            },
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let night = value.get("night").and_then(toml::Value::as_table);
    VrrConfig {
        outputs,
        night: NightColor {
            enabled: night
                .and_then(|table| table.get("enabled"))
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
            temperature: night
                .and_then(|table| table.get("temperature"))
                .and_then(toml::Value::as_integer)
                .unwrap_or(4500)
                .clamp(2000, 6500),
        },
    }
}
pub fn save(path: impl AsRef<Path>, config: &VrrConfig) -> std::io::Result<()> {
    let quote = |value: &str| toml::Value::String(value.to_string()).to_string();
    let mut lines = vec!["# Kyth VRR + night color".to_string()];
    for (name, output) in &config.outputs {
        lines.push(format!("[outputs.{}]", quote(name)));
        lines.push(format!("vrr = {}", quote(&output.vrr)));
        lines.push(String::new());
    }
    lines.push("[night]".into());
    lines.push(format!("enabled = {}", config.night.enabled));
    lines.push(format!("temperature = {}", config.night.temperature));
    crate::atomic_io::atomic_write_text(path, &format!("{}\n", lines.join("\n")), Some(0o600))
}
pub fn global_policy(outputs: &BTreeMap<String, VrrOutput>) -> &'static str {
    if outputs.values().any(|entry| entry.vrr == "always") {
        "2"
    } else if outputs.is_empty() || outputs.values().any(|entry| entry.vrr == "adaptive") {
        "1"
    } else {
        "0"
    }
}
pub fn policy_name(policy: &str) -> &'static str {
    match policy {
        "0" => "never",
        "2" => "always",
        _ => "adaptive",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn round_trip_clamps_night_colour_and_maps_policy() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vrr.toml");
        let config = VrrConfig {
            outputs: BTreeMap::from([(
                "HDMI-1".into(),
                VrrOutput {
                    vrr: "always".into(),
                },
            )]),
            night: NightColor {
                enabled: true,
                temperature: 7000,
            },
        };
        save(&path, &config).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.night.temperature, 6500);
        assert_eq!(global_policy(&loaded.outputs), "2");
        assert_eq!(policy_name("0"), "never");
    }
}
