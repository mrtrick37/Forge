//! Pure scheduler-arbiter configuration and desired-state calculation.
//!
//! The Python arbiter is the single writer for SCX/BORE placement. This
//! module ports the policy boundary only: service/process detection,
//! gamemode.ini rewriting, and activation remain caller-owned.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_CONFIG_PATH: &str = "/etc/kyth/sched-arbiter.toml";
pub const DEFAULT_FLAG_PATH: &str = "/run/kyth/sched-arbiter.json";
pub const KERNEL_FLAVOR_PATH: &str = "/usr/share/kyth/kernel-flavor";
pub const DEFAULT_GAMEMODE_INI: &str = "/etc/gamemode.ini";

const VALID_CHOICES: [&str; 4] = ["auto", "scx_rusty", "bore", "balanced"];

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path { return path.as_ref().to_path_buf(); }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") { return PathBuf::from(config).join("kyth/sched-arbiter.toml"); }
    }
    PathBuf::from(DEFAULT_CONFIG_PATH)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArbiterConfig {
    pub chosen: String,
    pub allow_ananicy_pin: bool,
    pub gamemode_pin: bool,
}

impl Default for ArbiterConfig {
    fn default() -> Self {
        Self {
            chosen: "auto".into(),
            allow_ananicy_pin: false,
            gamemode_pin: false,
        }
    }
}

impl ArbiterConfig {
    pub fn normalized(chosen: impl AsRef<str>, allow_ananicy_pin: bool, gamemode_pin: bool) -> Self {
        let mut chosen = chosen.as_ref().to_ascii_lowercase();
        if chosen == "none" {
            chosen = "balanced".into();
        }
        if !VALID_CHOICES.contains(&chosen.as_str()) {
            chosen = "auto".into();
        }
        Self { chosen, allow_ananicy_pin, gamemode_pin }
    }

    pub fn from_value(value: &Value) -> Self {
        let object = value.as_object();
        Self::normalized(
            object.and_then(|map| map.get("chosen")).and_then(Value::as_str).unwrap_or("auto"),
            object.and_then(|map| map.get("allow_ananicy_pin")).and_then(Value::as_bool).unwrap_or(false),
            object.and_then(|map| map.get("gamemode_pin")).and_then(Value::as_bool).unwrap_or(false),
        )
    }

    pub fn load(path: impl AsRef<Path>) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
            .map(|value| Self::from_value(&toml_to_json(&value)))
            .unwrap_or_default()
    }

    pub fn to_toml(&self) -> String {
        format!(
            "# Kyth scheduler arbiter — single writer for placement\n\
             # chosen: auto (detect SCX), scx_rusty, bore, balanced\n\
             chosen = \"{}\"\n\
             allow_ananicy_pin = {}\n\
             gamemode_pin = {}\n",
            self.chosen, self.allow_ananicy_pin, self.gamemode_pin
        )
    }

    pub fn as_value(&self) -> Value {
        json!({
            "chosen": self.chosen,
            "allow_ananicy_pin": self.allow_ananicy_pin,
            "gamemode_pin": self.gamemode_pin,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesiredState {
    pub chosen: String,
    pub active: String,
    pub scx_active: bool,
    pub bore_available: bool,
    pub gamemode_pin: bool,
    pub allow_ananicy_pin: bool,
}

pub fn desired_state(config: &ArbiterConfig, scx_active: bool, bore_available: bool) -> DesiredState {
    let active = match config.chosen.as_str() {
        "auto" if scx_active => "scx_rusty",
        "auto" if bore_available => "bore",
        "auto" => "balanced",
        chosen => chosen,
    };
    let (gamemode_pin, allow_ananicy_pin) = if active == "scx_rusty" || scx_active {
        (false, false)
    } else if active == "bore" {
        (config.gamemode_pin, config.allow_ananicy_pin)
    } else {
        (false, false)
    };
    DesiredState {
        chosen: config.chosen.clone(),
        active: active.into(),
        scx_active,
        bore_available,
        gamemode_pin,
        allow_ananicy_pin,
    }
}

/// Detect sched-ext using the same bounded service/process checks as the
/// legacy arbiter. Callers decide how the result affects their mutation.
pub fn detect_scx_active() -> bool {
    for service in ["scx_loader.service", "scx.service"] {
        let argv = vec!["systemctl".into(), "is-active".into(), "--quiet".into(), service.into()];
        if let Ok(output) = crate::system::process::run_bounded(&argv, Duration::from_secs(2)) {
            if output.status.success() { return true; }
        }
    }
    let argv = vec!["pgrep".into(), "-x".into(), "scx_rusty".into()];
    crate::system::process::run_bounded(&argv, Duration::from_secs(2))
        .map(|output| output.status.success() && !output.stdout.is_empty())
        .unwrap_or(false)
}

impl DesiredState {
    pub fn as_value(&self) -> Value {
        json!({
            "chosen": self.chosen,
            "active": self.active,
            "scx_active": self.scx_active,
            "bore_available": self.bore_available,
            "gamemode_pin": self.gamemode_pin,
            "allow_ananicy_pin": self.allow_ananicy_pin,
        })
    }
}

pub fn active_from_flag(value: &Value) -> String {
    value
        .get("active")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string()
}

pub fn flag_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path { return path.as_ref().to_path_buf(); }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") { return PathBuf::from(runtime).join("sched-arbiter.json"); }
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") { return PathBuf::from(config).join("kyth/sched-arbiter.json"); }
    }
    PathBuf::from(DEFAULT_FLAG_PATH)
}

/// True when the kernel flavor marks a BORE-capable kernel.
pub fn bore_available_in(path: &Path) -> bool {
    match std::fs::read_to_string(path) {
        Ok(text) => matches!(text.trim().to_ascii_lowercase().as_str(), "cachy" | "cachyos"),
        Err(_) => false,
    }
}

pub fn bore_available() -> bool {
    bore_available_in(Path::new(KERNEL_FLAVOR_PATH))
}

/// Desired state from the default config and live detection, mirroring
/// `_desired_state()` with no arguments.
pub fn current_desired_state() -> DesiredState {
    let config = ArbiterConfig::load(config_path(None::<PathBuf>));
    desired_state(&config, detect_scx_active(), bore_available())
}

/// Sync one gamemode.ini `pin_cores` line to the arbiter decision, touching
/// only `[cpu]`-section content like the Python rewrite. Returns true when
/// the file was rewritten.
pub fn sync_gamemode_pin(ini: &Path, pin: bool) -> bool {
    if !ini.is_file() {
        return false;
    }
    let Ok(text) = std::fs::read_to_string(ini) else { return false };
    let desired = if pin { "yes" } else { "no" };
    let Ok(pin_line) = Regex::new(r"(?m)^\s*pin_cores\s*=.*$") else { return false };
    let matched = pin_line.is_match(&text);
    let updated = if matched {
        pin_line.replace_all(&text, format!("pin_cores = {desired}")).into_owned()
    } else if text.contains("[cpu]") {
        text.replacen("[cpu]", &format!("[cpu]\npin_cores = {desired}"), 1)
    } else {
        return false;
    };
    if updated == text {
        return false;
    }
    std::fs::write(ini, updated).is_ok()
}

/// Regenerate the flag file and sync gamemode.ini, mirroring
/// `generate_arbiter()`. Errors propagate; game-launch swallows them.
pub fn generate_arbiter_to(flag: &Path, gamemode_ini: &Path) -> std::io::Result<PathBuf> {
    let state = current_desired_state();
    if let Some(parent) = flag.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = serde_json::to_string_pretty(&state.as_value())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    text.push('\n');
    let tmp = flag.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, flag)?;
    sync_gamemode_pin(gamemode_ini, state.gamemode_pin);
    Ok(flag.to_path_buf())
}

pub fn generate_arbiter() -> std::io::Result<PathBuf> {
    generate_arbiter_to(&flag_path(None::<PathBuf>), Path::new(DEFAULT_GAMEMODE_INI))
}

pub fn save_config(path: impl AsRef<Path>, config: &ArbiterConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(path, &config.to_toml(), Some(0o600))
}

pub fn write_flag(path: impl AsRef<Path>, state: &DesiredState) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(path, &serde_json::to_string_pretty(state).unwrap_or_else(|_| "{}".into()), Some(0o644))
}

fn toml_to_json(value: &toml::Value) -> Value {
    match value {
        toml::Value::String(value) => Value::String(value.clone()),
        toml::Value::Integer(value) => json!(value),
        toml::Value::Float(value) => json!(value),
        toml::Value::Boolean(value) => Value::Bool(*value),
        toml::Value::Datetime(value) => Value::String(value.to_string()),
        toml::Value::Array(values) => Value::Array(values.iter().map(toml_to_json).collect()),
        toml::Value::Table(values) => Value::Object(
            values.iter().map(|(key, value)| (key.clone(), toml_to_json(value))).collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_bore_flavor_and_syncs_gamemode_pin() {
        let dir = tempfile::tempdir().unwrap();
        let flavor = dir.path().join("kernel-flavor");
        std::fs::write(&flavor, "CachyOS\n").unwrap();
        assert!(bore_available_in(&flavor));
        std::fs::write(&flavor, "fedora\n").unwrap();
        assert!(!bore_available_in(&flavor));
        assert!(!bore_available_in(&dir.path().join("missing")));
        let ini = dir.path().join("gamemode.ini");
        std::fs::write(&ini, "[general]\nrenice = 10\n[cpu]\n  pin_cores = yes\n").unwrap();
        assert!(sync_gamemode_pin(&ini, false));
        let text = std::fs::read_to_string(&ini).unwrap();
        assert!(text.contains("pin_cores = no"));
        assert!(!sync_gamemode_pin(&ini, false));
        std::fs::write(&ini, "[general]\n[cpu]\n").unwrap();
        assert!(sync_gamemode_pin(&ini, true));
        assert!(std::fs::read_to_string(&ini).unwrap().contains("[cpu]\npin_cores = yes"));
        assert!(!sync_gamemode_pin(&dir.path().join("missing.ini"), true));
    }

    #[test]
    fn normalizes_legacy_and_unknown_choices() {
        assert_eq!(ArbiterConfig::normalized("NONE", true, true).chosen, "balanced");
        assert_eq!(ArbiterConfig::normalized("surprise", true, true).chosen, "auto");
        assert_eq!(ArbiterConfig::default(), ArbiterConfig::normalized("auto", false, false));
    }

    #[test]
    fn loads_toml_and_round_trips_projection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sched-arbiter.toml");
        std::fs::write(&path, "chosen = \"bore\"\nallow_ananicy_pin = true\ngamemode_pin = true\n").unwrap();
        let config = ArbiterConfig::load(&path);
        assert_eq!(config.chosen, "bore");
        assert!(config.allow_ananicy_pin);
        assert!(config.to_toml().contains("gamemode_pin = true"));
    }

    #[test]
    fn auto_selects_single_writer_and_disables_competing_pinning() {
        let config = ArbiterConfig::normalized("auto", true, true);
        let scx = desired_state(&config, true, true);
        assert_eq!(scx.active, "scx_rusty");
        assert!(!scx.gamemode_pin);
        assert!(!scx.allow_ananicy_pin);

        let bore = desired_state(&config, false, true);
        assert_eq!(bore.active, "bore");
        assert!(bore.gamemode_pin);
        assert!(bore.allow_ananicy_pin);

        let balanced = desired_state(&config, false, false);
        assert_eq!(balanced.active, "balanced");
        assert!(!balanced.gamemode_pin);
    }

    #[test]
    fn explicit_bore_still_yields_to_active_scx() {
        let config = ArbiterConfig::normalized("bore", true, true);
        let state = desired_state(&config, true, false);
        assert_eq!(state.active, "bore");
        assert!(!state.gamemode_pin);
        assert!(!state.allow_ananicy_pin);
    }

    #[test]
    fn flag_status_has_safe_unknown_fallback() {
        assert_eq!(active_from_flag(&json!({"active":"scx_rusty"})), "scx_rusty");
        assert_eq!(active_from_flag(&json!({})), "unknown");
    }
}
