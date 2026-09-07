//! Window snap parity: Win+Arrow shortcuts via `kwriteconfig`.
//!
//! Mirrors `kyth_shared.window_snap`: the `ElectricBorder` write is the
//! only noted one; the three `kglobalshortcutsrc` writes are best-effort
//! fire-and-forget with no notes. Only the `*_bin.rs` entry point executes
//! processes.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapConfig {
    pub layout: String,
    pub win_z: bool,
    pub electric: bool,
}

impl Default for SnapConfig {
    fn default() -> Self { Self { layout: "2x2".into(), win_z: true, electric: true } }
}

pub const TTL_PATH: &str = "/run/kyth-snap-ttl";
pub const TTL_SECS: u64 = 30;

pub const SHORTCUTS: [(&str, &str); 3] = [
    ("Window Quick Tile Left", "Meta+Left"),
    ("Window Quick Tile Right", "Meta+Right"),
    ("Window Maximize", "Meta+Up"),
];

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("kyth/window-snap.toml");
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".config/kyth/window-snap.toml")
}

pub fn load(path: impl AsRef<Path>) -> SnapConfig {
    let Ok(raw) = std::fs::read_to_string(path) else { return SnapConfig::default(); };
    let Ok(value) = raw.parse::<toml::Value>() else { return SnapConfig::default(); };
    let layout = value.get("layout").and_then(toml::Value::as_str).unwrap_or("2x2");
    SnapConfig {
        layout: matches!(layout, "2x2" | "3col" | "off").then(|| layout.to_string()).unwrap_or_else(|| "2x2".into()),
        win_z: value.get("win_z").and_then(toml::Value::as_bool).unwrap_or(true),
        electric: value.get("electric").and_then(toml::Value::as_bool).unwrap_or(true),
    }
}

pub fn electric_border_argv(binary: &str, electric: bool) -> Vec<String> {
    vec![
        binary.to_string(),
        "--file".to_string(),
        "kwinrc".to_string(),
        "--group".to_string(),
        "Windows".to_string(),
        "--key".to_string(),
        "ElectricBorder".to_string(),
        "--type".to_string(),
        "bool".to_string(),
        if electric { "true".to_string() } else { "false".to_string() },
    ]
}

pub fn shortcut_argv(binary: &str, action: &str, key: &str) -> Vec<String> {
    vec![
        binary.to_string(),
        "--file".to_string(),
        "kglobalshortcutsrc".to_string(),
        "--group".to_string(),
        "kwin".to_string(),
        "--key".to_string(),
        action.to_string(),
        format!("{key},none,{action}"),
    ]
}

pub fn kwriteconfig_candidates() -> [&'static str; 3] {
    ["kwriteconfig6", "kwriteconfig5", "kwriteconfig"]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn validates_layout_and_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("window-snap.toml");
        std::fs::write(&path, "layout = \"off\"\nwin_z = false\n").unwrap();
        assert_eq!(load(&path), SnapConfig { layout: "off".into(), win_z: false, electric: true });
        let bad = dir.path().join("bad.toml");
        std::fs::write(&bad, "layout = \"4x4\"\n").unwrap();
        assert_eq!(load(&bad).layout, "2x2");
        assert_eq!(load(dir.path().join("missing.toml")), SnapConfig::default());
    }

    #[test]
    fn projects_snap_argv() {
        assert_eq!(
            electric_border_argv("kwriteconfig6", true),
            vec!["kwriteconfig6", "--file", "kwinrc", "--group", "Windows", "--key", "ElectricBorder", "--type", "bool", "true"]
        );
        assert_eq!(
            shortcut_argv("kwriteconfig6", "Window Maximize", "Meta+Up"),
            vec!["kwriteconfig6", "--file", "kglobalshortcutsrc", "--group", "kwin", "--key", "Window Maximize", "Meta+Up,none,Window Maximize"]
        );
    }
}
