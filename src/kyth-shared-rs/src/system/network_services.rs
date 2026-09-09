//! Offline cloud-drive and Tailscale preference models.

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

pub fn load_cloud(path: impl AsRef<Path>) -> BTreeMap<String, String> {
    let Some(value) = parse(path) else {
        return BTreeMap::new();
    };
    value
        .get("drives")
        .and_then(toml::Value::as_table)
        .map(|drives| {
            drives
                .iter()
                .filter_map(|(name, value)| {
                    Some((
                        name.clone(),
                        value
                            .as_table()?
                            .get("remote")
                            .and_then(toml::Value::as_str)
                            .unwrap_or("")
                            .into(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}
pub fn cloud_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    user_path("cloud.toml", path)
}
pub fn save_cloud(
    path: impl AsRef<Path>,
    drives: &BTreeMap<String, String>,
) -> std::io::Result<()> {
    let mut lines = vec!["# Kyth Cloud Drive — rclone mount + kio network:/".to_string()];
    for (name, remote) in drives {
        lines.push(format!("[drives.{}]", quote(name)));
        lines.push(format!("remote = {}", quote(remote)));
        lines.push(String::new());
    }
    crate::atomic_io::atomic_write_text(path, &format!("{}\n", lines.join("\n")), Some(0o600))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailscaleConfig {
    pub tailnet: String,
    pub exit_node: String,
    pub accept_routes: bool,
}
impl Default for TailscaleConfig {
    fn default() -> Self {
        Self {
            tailnet: String::new(),
            exit_node: String::new(),
            accept_routes: false,
        }
    }
}
pub fn tailscale_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    user_path("tailscale.toml", path)
}
pub fn load_tailscale(path: impl AsRef<Path>) -> TailscaleConfig {
    parse(path)
        .map(|v| TailscaleConfig {
            tailnet: v
                .get("tailnet")
                .and_then(toml::Value::as_str)
                .unwrap_or("")
                .into(),
            exit_node: v
                .get("exit_node")
                .and_then(toml::Value::as_str)
                .unwrap_or("")
                .into(),
            accept_routes: v
                .get("accept_routes")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
        })
        .unwrap_or_default()
}
pub fn save_tailscale(path: impl AsRef<Path>, config: &TailscaleConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(path, &format!("# Kyth Tailscale mesh, offline hash-gated\ntailnet = {}\nexit_node = {}\naccept_routes = {}\n", quote(&config.tailnet), quote(&config.exit_node), config.accept_routes), Some(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn cloud_round_trip_preserves_quoted_drive_names() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("cloud.toml");
        let drives = BTreeMap::from([("Work \"NAS\"".into(), "nas:home".into())]);
        save_cloud(&path, &drives).unwrap();
        assert_eq!(load_cloud(&path), drives);
    }

    #[test]
    fn tailscale_loads_safe_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("tailscale.toml");
        save_tailscale(
            &path,
            &TailscaleConfig {
                tailnet: "example".into(),
                accept_routes: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(load_tailscale(&path).tailnet, "example");
        assert!(load_tailscale(&path).accept_routes);
    }
}
