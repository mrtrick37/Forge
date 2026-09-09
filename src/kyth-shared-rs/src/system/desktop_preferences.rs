//! Offline desktop preference models and command projections.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn quote(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}
fn user_path(filename: &str, explicit: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = explicit {
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
    .join(format!("kyth/{filename}"))
}
fn parse(path: impl AsRef<Path>) -> Option<toml::Value> {
    std::fs::read_to_string(path).ok()?.parse().ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatpakOverride {
    pub filesystem: String,
    pub sockets: String,
    pub devices: String,
}
impl Default for FlatpakOverride {
    fn default() -> Self {
        Self {
            filesystem: String::new(),
            sockets: String::new(),
            devices: String::new(),
        }
    }
}
pub fn flatpak_overrides_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    user_path("flatpak-overrides.toml", path)
}
pub fn load_flatpak_overrides(path: impl AsRef<Path>) -> BTreeMap<String, FlatpakOverride> {
    let Some(value) = parse(path) else {
        return BTreeMap::new();
    };
    value
        .get("overrides")
        .and_then(toml::Value::as_table)
        .map(|items| {
            items
                .iter()
                .filter_map(|(appid, value)| {
                    let table = value.as_table()?;
                    Some((
                        appid.clone(),
                        FlatpakOverride {
                            filesystem: table
                                .get("filesystem")
                                .and_then(toml::Value::as_str)
                                .unwrap_or("")
                                .into(),
                            sockets: table
                                .get("sockets")
                                .and_then(toml::Value::as_str)
                                .unwrap_or("")
                                .into(),
                            devices: table
                                .get("devices")
                                .and_then(toml::Value::as_str)
                                .unwrap_or("")
                                .into(),
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}
pub fn save_flatpak_overrides(
    path: impl AsRef<Path>,
    overrides: &BTreeMap<String, FlatpakOverride>,
) -> std::io::Result<()> {
    let mut lines = vec!["# Kyth flatpak overrides — offline declarative".to_string()];
    for (appid, entry) in overrides {
        lines.push(format!("[overrides.{}]", quote(appid)));
        if !entry.filesystem.is_empty() {
            lines.push(format!("filesystem = {}", quote(&entry.filesystem)));
        }
        if !entry.sockets.is_empty() {
            lines.push(format!("sockets = {}", quote(&entry.sockets)));
        }
        if !entry.devices.is_empty() {
            lines.push(format!("devices = {}", quote(&entry.devices)));
        }
        lines.push(String::new());
    }
    crate::atomic_io::atomic_write_text(path, &format!("{}\n", lines.join("\n")), Some(0o600))
}
pub fn flatpak_override_args(entry: &FlatpakOverride) -> Vec<String> {
    let mut args = Vec::new();
    if !entry.filesystem.is_empty() {
        args.push(format!("--filesystem={}", entry.filesystem));
    }
    for socket in entry
        .sockets
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        args.push(format!("--socket={socket}"));
    }
    for device in entry
        .devices
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        args.push(format!("--device={device}"));
    }
    args
}
pub fn flatpak_override_for(appid: &str, path: impl AsRef<Path>) -> FlatpakOverride {
    load_flatpak_overrides(path)
        .remove(appid)
        .unwrap_or_default()
}

fn scalar_string(value: &toml::Value) -> String {
    match value {
        toml::Value::String(value) => value.clone(),
        toml::Value::Boolean(value) => {
            if *value {
                "True".into()
            } else {
                "False".into()
            }
        }
        _ => value.to_string(),
    }
}
fn flatten_table(
    table: &toml::map::Map<String, toml::Value>,
    prefix: &str,
    output: &mut BTreeMap<String, BTreeMap<String, String>>,
) {
    let mut scalars = BTreeMap::new();
    for (key, value) in table {
        let name = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        if let Some(child) = value.as_table() {
            flatten_table(child, &name, output);
        } else if !prefix.is_empty() {
            scalars.insert(key.clone(), scalar_string(value));
        }
    }
    if !scalars.is_empty() && !prefix.is_empty() {
        output.insert(prefix.to_string(), scalars);
    }
}
pub fn flatten_plasma_sections(value: &toml::Value) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut output = BTreeMap::new();
    if let Some(table) = value.as_table() {
        flatten_table(table, "", &mut output);
    }
    output
}
pub fn load_plasma(path: impl AsRef<Path>) -> BTreeMap<String, BTreeMap<String, String>> {
    parse(path)
        .map(|value| flatten_plasma_sections(&value))
        .unwrap_or_default()
}
pub fn plasma_config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    user_path("plasma.toml", path)
}
pub fn save_plasma(
    path: impl AsRef<Path>,
    sections: &BTreeMap<String, BTreeMap<String, String>>,
) -> std::io::Result<()> {
    let mut lines = vec!["# Kyth Plasma drift — declarative, offline".to_string()];
    for (section, values) in sections {
        lines.push(format!("[{section}]"));
        for (key, value) in values {
            lines.push(format!("{} = {}", key, quote(value)));
        }
        lines.push(String::new());
    }
    crate::atomic_io::atomic_write_text(path, &format!("{}\n", lines.join("\n")), Some(0o600))
}
pub fn parse_plasma_section(section: &str) -> Option<(String, Vec<String>)> {
    let parts = section
        .split('.')
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return None;
    }
    if parts.len() == 1 {
        Some((parts[0].clone(), vec!["General".into()]))
    } else {
        Some((parts[0].clone(), parts[1..].to_vec()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSnapConfig {
    pub layout: String,
    pub win_z: bool,
    pub electric: bool,
}
impl Default for WindowSnapConfig {
    fn default() -> Self {
        Self {
            layout: "2x2".into(),
            win_z: true,
            electric: true,
        }
    }
}
pub fn window_snap_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    user_path("window-snap.toml", path)
}
pub fn load_window_snap(path: impl AsRef<Path>) -> WindowSnapConfig {
    parse(path)
        .map(|v| WindowSnapConfig {
            layout: {
                let layout = v
                    .get("layout")
                    .and_then(toml::Value::as_str)
                    .unwrap_or("2x2");
                matches!(layout, "2x2" | "3col" | "off")
                    .then_some(layout)
                    .unwrap_or("2x2")
                    .into()
            },
            win_z: v
                .get("win_z")
                .and_then(toml::Value::as_bool)
                .unwrap_or(true),
            electric: v
                .get("electric")
                .and_then(toml::Value::as_bool)
                .unwrap_or(true),
        })
        .unwrap_or_default()
}
pub fn save_window_snap(path: impl AsRef<Path>, config: &WindowSnapConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth window snap — Win+Arrow, offline\nlayout = {}\nwin_z = {}\nelectric = {}\n",
            quote(&config.layout),
            config.win_z,
            config.electric
        ),
        Some(0o600),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn flatpak_overrides_round_trip_and_project_args() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("overrides.toml");
        let entry = FlatpakOverride {
            filesystem: "home".into(),
            sockets: "wayland; pulseaudio".into(),
            devices: "all".into(),
        };
        let values = BTreeMap::from([("org.example.App".into(), entry.clone())]);
        save_flatpak_overrides(&path, &values).unwrap();
        assert_eq!(load_flatpak_overrides(&path), values);
        assert_eq!(
            flatpak_override_args(&entry),
            vec![
                "--filesystem=home",
                "--socket=wayland",
                "--socket=pulseaudio",
                "--device=all"
            ]
        );
    }

    #[test]
    fn plasma_flattening_preserves_nested_sections() {
        let value: toml::Value = "[kwinrc.Compositing]\nAllowTearing = false\n[kwinrc.Containments.1.General]\nfoo = \"bar\"\ntop = \"ignored\"\n".parse().unwrap();
        let sections = flatten_plasma_sections(&value);
        assert_eq!(sections["kwinrc.Compositing"]["AllowTearing"], "False");
        assert_eq!(sections["kwinrc.Containments.1.General"]["foo"], "bar");
        assert!(!sections.contains_key("top"));
    }
}
