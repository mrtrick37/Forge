//! VRR + Night Color: `vrr.toml` per-output store with a KWin apply path.
//!
//! Mirrors `kyth_shared.vrr`: `adaptive|always|never` maps onto global
//! `[Wayland] VrrPolicy` (1/2/0) with per-output `kscreen-doctor` overrides
//! when a live session is available. Only the `*_bin.rs` entry point
//! executes processes.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VrrConfig {
    pub outputs: BTreeMap<String, String>,
    pub night_enabled: bool,
    pub night_temperature: i64,
}

impl Default for VrrConfig {
    fn default() -> Self {
        Self { outputs: BTreeMap::new(), night_enabled: false, night_temperature: 4500 }
    }
}

pub const TTL_PATH: &str = "/run/kyth-vrr-ttl";
pub const TTL_SECS: u64 = 30;

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("kyth/vrr.toml");
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".config/kyth/vrr.toml")
}

fn validated_mode(value: Option<&toml::Value>) -> String {
    let mode = value.and_then(toml::Value::as_str).unwrap_or("adaptive");
    matches!(mode, "never" | "adaptive" | "always").then(|| mode.to_string()).unwrap_or_else(|| "adaptive".into())
}

pub fn load(path: impl AsRef<Path>) -> VrrConfig {
    let Ok(raw) = std::fs::read_to_string(path) else { return VrrConfig::default(); };
    let Ok(value) = raw.parse::<toml::Value>() else { return VrrConfig::default(); };
    let mut outputs = BTreeMap::new();
    if let Some(table) = value.get("outputs").and_then(toml::Value::as_table) {
        for (conn, entry) in table {
            let Some(entry) = entry.as_table() else { continue };
            outputs.insert(conn.clone(), validated_mode(entry.get("vrr")));
        }
    }
    let night = value.get("night").and_then(toml::Value::as_table);
    VrrConfig {
        outputs,
        night_enabled: night.and_then(|n| n.get("enabled")).and_then(toml::Value::as_bool).unwrap_or(false),
        night_temperature: night
            .and_then(|n| n.get("temperature"))
            .and_then(|t| t.as_integer().or_else(|| t.as_float().map(|v| v as i64)))
            .unwrap_or(4500)
            .clamp(2000, 6500),
    }
}

pub fn policy_for_mode(mode: &str) -> &'static str {
    match mode {
        "never" => "0",
        "always" => "2",
        _ => "1",
    }
}

pub fn mode_for_policy(policy: &str) -> &str {
    match policy {
        "0" => "never",
        "2" => "always",
        _ => "adaptive",
    }
}

/// Picks the global VrrPolicy: Always if any output wants it, else Adaptive
/// if any (or when empty), else Never — exactly as upstream.
pub fn global_policy(outputs: &BTreeMap<String, String>) -> &'static str {
    let modes: HashSet<&str> = outputs.values().map(String::as_str).collect();
    if modes.contains("always") {
        return "2";
    }
    if modes.contains("adaptive") || modes.is_empty() {
        return "1";
    }
    "0"
}

pub fn kwin_argv(binary: &str, group: &str, key: &str, value: &str, value_type: Option<&str>) -> Vec<String> {
    let mut argv =
        vec![binary.to_string(), "--file".to_string(), "kwinrc".to_string(), "--group".to_string(), group.to_string(), "--key".to_string(), key.to_string()];
    if let Some(value_type) = value_type {
        argv.extend(["--type".to_string(), value_type.to_string()]);
    }
    argv.push(value.to_string());
    argv
}

/// Maps a config mode onto the `kscreen-doctor` vrrpolicy name (unknown
/// modes fall back to `automatic`, as upstream).
pub fn doctor_mode(mode: &str) -> &str {
    match mode {
        "never" => "never",
        "always" => "always",
        _ => "automatic",
    }
}

pub fn per_output_argv(conn: &str, mode: &str) -> Vec<String> {
    vec!["kscreen-doctor".to_string(), format!("output.{conn}.vrrpolicy.{}", doctor_mode(mode))]
}

pub fn is_output_name_valid(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn validates_modes_and_clamps_temperature() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vrr.toml");
        std::fs::write(&path, "[outputs.HDMI-1]\nvrr = \"always\"\n[outputs.DP-1]\nvrr = \"bogus\"\n[night]\nenabled = true\ntemperature = 99999\n").unwrap();
        let config = load(&path);
        assert_eq!(config.outputs.get("HDMI-1"), Some(&"always".to_string()));
        assert_eq!(config.outputs.get("DP-1"), Some(&"adaptive".to_string()));
        assert!(config.night_enabled);
        assert_eq!(config.night_temperature, 6500);
        assert_eq!(load(dir.path().join("missing.toml")), VrrConfig::default());
    }

    #[test]
    fn picks_global_policy_like_python() {
        assert_eq!(global_policy(&BTreeMap::new()), "1");
        assert_eq!(global_policy(&BTreeMap::from([("a".to_string(), "never".to_string())])), "0");
        assert_eq!(
            global_policy(&BTreeMap::from([("a".to_string(), "adaptive".to_string()), ("b".to_string(), "never".to_string())])),
            "1"
        );
        assert_eq!(
            global_policy(&BTreeMap::from([("a".to_string(), "always".to_string()), ("b".to_string(), "never".to_string())])),
            "2"
        );
    }

    #[test]
    fn projects_kwin_and_doctor_argv() {
        assert_eq!(
            kwin_argv("kwriteconfig6", "Wayland", "VrrPolicy", "1", None),
            vec!["kwriteconfig6", "--file", "kwinrc", "--group", "Wayland", "--key", "VrrPolicy", "1"]
        );
        assert_eq!(per_output_argv("DP-1", "adaptive"), vec!["kscreen-doctor", "output.DP-1.vrrpolicy.automatic"]);
        assert_eq!(per_output_argv("DP-1", "bogus"), vec!["kscreen-doctor", "output.DP-1.vrrpolicy.automatic"]);
    }
}
