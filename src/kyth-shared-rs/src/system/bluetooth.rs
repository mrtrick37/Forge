//! Offline Bluetooth LE Audio per-device presets.

use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BluetoothPreset {
    pub codec: String,
    pub latency: String,
}

impl Default for BluetoothPreset {
    fn default() -> Self {
        Self {
            codec: "LC3".into(),
            latency: "low".into(),
        }
    }
}

pub fn load(path: impl AsRef<Path>) -> BTreeMap<String, BluetoothPreset> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return BTreeMap::new();
    };
    value
        .get("devices")
        .and_then(toml::Value::as_table)
        .map(|devices| {
            devices
                .iter()
                .filter_map(|(address, value)| {
                    let entry = value.as_table()?;
                    Some((
                        address.clone(),
                        BluetoothPreset {
                            codec: entry
                                .get("codec")
                                .and_then(toml::Value::as_str)
                                .unwrap_or("LC3")
                                .into(),
                            latency: entry
                                .get("latency")
                                .and_then(toml::Value::as_str)
                                .unwrap_or("low")
                                .into(),
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn save(
    path: impl AsRef<Path>,
    devices: &BTreeMap<String, BluetoothPreset>,
) -> std::io::Result<()> {
    let mut text = String::from("# Kyth Bluetooth LE Audio per-device\n");
    for (address, preset) in devices {
        text.push_str(&format!(
            "[devices.{address:?}]\ncodec = {:?}\nlatency = {:?}\n\n",
            preset.codec, preset.latency
        ));
    }
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_defaults_for_partial_device_entries() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("bluetooth.toml");
        std::fs::write(&path, "[devices.aa-bb]\ncodec = \"opus\"\n").unwrap();
        assert_eq!(
            load(&path)["aa-bb"],
            BluetoothPreset {
                codec: "opus".into(),
                latency: "low".into()
            }
        );
    }

    #[test]
    fn saves_sorted_device_presets() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("bluetooth.toml");
        let mut devices = BTreeMap::new();
        devices.insert("aa-bb".into(), BluetoothPreset::default());
        save(&path, &devices).unwrap();
        assert!(std::fs::read_to_string(path)
            .unwrap()
            .contains("[devices.\"aa-bb\"]"));
    }
}
