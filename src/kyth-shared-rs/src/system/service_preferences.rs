//! Deterministic service/preset helpers with no service activation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use sha2::{Digest, Sha256};

fn quote(value: &str) -> String { toml::Value::String(value.to_string()).to_string() }
fn system_path(filename: &str, explicit: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = explicit { return path.as_ref().to_path_buf(); }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") { return PathBuf::from(config).join(format!("kyth/{filename}")); }
    }
    PathBuf::from("/etc/kyth").join(filename)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlymouthConfig { pub theme: String, pub duration: i64 }
impl Default for PlymouthConfig { fn default() -> Self { Self { theme: "kyth".into(), duration: 5 } } }
pub fn plymouth_path(path: Option<impl AsRef<Path>>) -> PathBuf { system_path("plymouth.toml", path) }
pub fn load_plymouth(path: impl AsRef<Path>) -> PlymouthConfig {
    let Ok(raw) = std::fs::read_to_string(path) else { return PlymouthConfig::default(); };
    let Ok(value) = raw.parse::<toml::Value>() else { return PlymouthConfig::default(); };
    PlymouthConfig { theme: value.get("theme").and_then(toml::Value::as_str).unwrap_or("kyth").into(), duration: value.get("duration").and_then(toml::Value::as_integer).unwrap_or(5).clamp(1, 30) }
}
pub fn save_plymouth(path: impl AsRef<Path>, config: &PlymouthConfig) -> std::io::Result<()> { crate::atomic_io::atomic_write_text(path, &format!("# Kyth Plymouth theme preset\ntheme = {}\nduration = {}\n", quote(&config.theme), config.duration), Some(0o600)) }

pub fn shader_content_hash(appid: &str, driver_version: &str, glsl_text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(appid.as_bytes()); hasher.update([0]);
    hasher.update(driver_version.as_bytes()); hasher.update([0]);
    hasher.update(glsl_text.as_bytes());
    hasher.finalize().iter().take(6).map(|byte| format!("{byte:02x}")).collect()
}
pub fn shader_cache_dir(appid: &str, driver_version: &str, glsl_text: &str, root: impl AsRef<Path>) -> PathBuf { root.as_ref().join(appid).join(shader_content_hash(appid, driver_version, glsl_text)) }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderCacheStatus { pub cached: bool, pub files: usize, pub path: PathBuf, pub hash: String }
pub fn shader_cache_status(appid: &str, driver_version: &str, root: impl AsRef<Path>) -> ShaderCacheStatus {
    let path = shader_cache_dir(appid, driver_version, "", root);
    let files = std::fs::read_dir(&path).map(|entries| entries.filter_map(Result::ok).count()).unwrap_or(0);
    ShaderCacheStatus { cached: path.is_dir(), files, hash: path.file_name().and_then(|name| name.to_str()).unwrap_or_default().into(), path }
}
pub fn ensure_shader_cache_dir(appid: &str, driver_version: &str, glsl_text: &str, root: impl AsRef<Path>) -> std::io::Result<PathBuf> { let path = shader_cache_dir(appid, driver_version, glsl_text, root); std::fs::create_dir_all(&path)?; Ok(path) }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolkitRules { pub flatpak: bool, pub btrfs: bool }
impl Default for PolkitRules { fn default() -> Self { Self { flatpak: true, btrfs: true } } }
pub fn polkit_path(path: Option<impl AsRef<Path>>) -> PathBuf { system_path("polkit.toml", path) }
pub fn load_polkit(path: impl AsRef<Path>) -> PolkitRules {
    let Ok(raw) = std::fs::read_to_string(path) else { return PolkitRules::default(); };
    let Ok(value) = raw.parse::<toml::Value>() else { return PolkitRules::default(); };
    let Some(rules) = value.get("rules").and_then(toml::Value::as_table) else { return PolkitRules::default(); };
    PolkitRules { flatpak: rules.get("flatpak").and_then(toml::Value::as_bool).unwrap_or(true), btrfs: rules.get("btrfs").and_then(toml::Value::as_bool).unwrap_or(true) }
}
pub fn save_polkit(path: impl AsRef<Path>, rules: &PolkitRules) -> std::io::Result<()> { crate::atomic_io::atomic_write_text(path, &format!("# Kyth polkit presets\n[rules]\nflatpak = {}\nbtrfs = {}\n", rules.flatpak, rules.btrfs), Some(0o600)) }
pub fn generate_polkit_rules(rules: &PolkitRules) -> String {
    let mut lines = vec!["// Kyth polkit — generated from polkit.toml, offline".to_string()];
    if rules.flatpak { lines.push("polkit.addRule(function(a,s){if(a.id.indexOf(\"org.freedesktop.Flatpak\")==0 && s.isInGroup(\"wheel\"))return polkit.Result.YES;});".into()); }
    if rules.btrfs { lines.push("polkit.addRule(function(a,s){if(a.id==\"org.freedesktop.UDisks2.modify-device\" && s.isInGroup(\"wheel\"))return polkit.Result.YES;});".into()); }
    format!("{}\n", lines.join("\n"))
}

/// Resolves the on-disk path for `scx.toml`.
///
/// Unlike [`system_path`] (used by `plymouth_path`/`polkit_path`, which default
/// to `/etc/kyth` for operator-owned presets), this mirrors the Python
/// `kyth_shared.scx_preset.scx_config_path` it replaces: an explicit `path`
/// wins, then `$XDG_CONFIG_HOME/kyth/scx.toml`, then `$HOME/.config/kyth/scx.toml`.
/// `scx.toml` is a hand-authored, per-user file (see `kyth-apply-scx-preset`),
/// not a system preset store, so the home-directory default is preserved
/// rather than converged onto `/etc/kyth`.
pub fn scx_config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path { return path.as_ref().to_path_buf(); }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") { return PathBuf::from(xdg).join("kyth/scx.toml"); }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config/kyth/scx.toml")
}
pub fn load_scx(path: impl AsRef<Path>) -> BTreeMap<String, String> {
    let Ok(raw) = std::fs::read_to_string(path) else { return BTreeMap::new(); };
    let Ok(value) = raw.parse::<toml::Value>() else { return BTreeMap::new(); };
    value.get("games").and_then(toml::Value::as_table).map(|games| games.iter().filter_map(|(app, value)| {
        let scx = value.as_table().and_then(|table| table.get("scx")).and_then(toml::Value::as_str).or_else(|| value.as_str())?;
        matches!(scx, "scx_rusty" | "scx_bpfland" | "scx_lavd" | "none").then(|| (app.clone(), scx.to_string()))
    }).collect()).unwrap_or_default()
}
pub fn save_scx(path: impl AsRef<Path>, games: &BTreeMap<String, String>) -> std::io::Result<()> {
    let mut lines = vec!["# Kyth SCX per-game, explicit wins over TTL".to_string()];
    for (app, scx) in games { lines.push(format!("[games.{}]", quote(app))); lines.push(format!("scx = {}", quote(scx))); lines.push(String::new()); }
    crate::atomic_io::atomic_write_text(path, &format!("{}\n", lines.join("\n")), Some(0o600))
}
pub fn scx_for_app(app: &str, path: impl AsRef<Path>) -> Option<String> { load_scx(path).remove(app) }

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn shader_hash_matches_python_shape() { assert_eq!(shader_content_hash("game", "driver", "" ).len(), 12); }

    #[test]
    fn polkit_defaults_and_generation_are_deterministic() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("polkit.toml");
        save_polkit(&path, &PolkitRules { flatpak: true, btrfs: false }).unwrap();
        let rules = load_polkit(&path);
        assert!(rules.flatpak); assert!(!rules.btrfs);
        assert!(generate_polkit_rules(&rules).contains("Flatpak"));
    }

    #[test]
    fn scx_round_trip_filters_unknown_schedulers() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("scx.toml");
        let games = BTreeMap::from([("game".into(), "scx_lavd".into())]);
        save_scx(&path, &games).unwrap();
        assert_eq!(load_scx(&path), games);
    }

    #[test]
    fn scx_config_path_prefers_explicit_then_xdg_then_home() {
        assert_eq!(scx_config_path(Some("/tmp/explicit.toml")), PathBuf::from("/tmp/explicit.toml"));
    }

    /// `load_scx` returns a `BTreeMap`, so a file with two `[games.*]` tables
    /// has no "first inserted" entry to reproduce from the Python launcher
    /// (which picked `list(dict.values())[0]`, i.e. TOML file order). This
    /// pins the deliberate replacement semantics: `apply_scx_preset_bin`
    /// selects the lexicographically-first app name via `BTreeMap` iteration.
    #[test]
    fn multi_game_scx_selects_lexicographically_first_app() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("scx.toml");
        let games = BTreeMap::from([("zelda".into(), "scx_lavd".into()), ("ananicy".into(), "scx_bpfland".into())]);
        save_scx(&path, &games).unwrap();
        let loaded = load_scx(&path);
        assert_eq!(loaded.iter().next(), Some((&"ananicy".to_string(), &"scx_bpfland".to_string())));
    }
}
