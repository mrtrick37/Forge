//! Small offline preference models shared by desktop settings and native UIs.
//!
//! The corresponding Python modules also contain privileged application
//! helpers. This module intentionally ports the deterministic config and
//! rendering side only; service activation and system policy writes remain
//! with their existing owners.

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
fn system_path(filename: &str, explicit: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = explicit {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join(format!("kyth/{filename}"));
        }
    }
    PathBuf::from("/etc/kyth").join(filename)
}
fn parse_string(value: Option<&toml::Value>, default: &str) -> String {
    value
        .and_then(toml::Value::as_str)
        .unwrap_or(default)
        .to_string()
}
fn parse_bool(value: Option<&toml::Value>, default: bool) -> bool {
    value.and_then(toml::Value::as_bool).unwrap_or(default)
}
fn parse_i64(value: Option<&toml::Value>, default: i64) -> i64 {
    value.and_then(toml::Value::as_integer).unwrap_or(default)
}
fn parse_f64(value: Option<&toml::Value>, default: f64) -> f64 {
    value
        .and_then(toml::Value::as_float)
        .or_else(|| value.and_then(toml::Value::as_integer).map(|v| v as f64))
        .unwrap_or(default)
}
fn parse_root(path: impl AsRef<Path>) -> Option<toml::Value> {
    std::fs::read_to_string(path).ok()?.parse().ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontsConfig {
    pub hinting: String,
    pub antialias: String,
    pub dpi: i64,
    pub family: String,
}
impl Default for FontsConfig {
    fn default() -> Self {
        Self {
            hinting: "full".into(),
            antialias: "rgba".into(),
            dpi: 96,
            family: "Inter".into(),
        }
    }
}
pub fn fonts_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    user_path("fonts.toml", path)
}
pub fn load_fonts(path: impl AsRef<Path>) -> FontsConfig {
    let Some(value) = parse_root(path) else {
        return FontsConfig::default();
    };
    let hinting = parse_string(value.get("hinting"), "full");
    let antialias = parse_string(value.get("antialias"), "rgba");
    FontsConfig {
        hinting: matches!(hinting.as_str(), "full" | "medium" | "slight" | "none")
            .then_some(hinting)
            .unwrap_or_else(|| "full".into()),
        antialias: matches!(antialias.as_str(), "rgba" | "grayscale" | "none")
            .then_some(antialias)
            .unwrap_or_else(|| "rgba".into()),
        dpi: parse_i64(value.get("dpi"), 96).clamp(72, 300),
        family: parse_string(value.get("family"), "Inter"),
    }
}
pub fn save_fonts(path: impl AsRef<Path>, config: &FontsConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(path, &format!("# Kyth fonts rendering, offline\nhinting = {}\nantialias = {}\ndpi = {}\nfamily = {}\n", quote(&config.hinting), quote(&config.antialias), config.dpi, quote(&config.family)), Some(0o600))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleConfig {
    pub lang: String,
    pub ime: String,
    pub keymap: String,
}
impl Default for LocaleConfig {
    fn default() -> Self {
        Self {
            lang: "en_US.UTF-8".into(),
            ime: "fcitx5".into(),
            keymap: "us".into(),
        }
    }
}
pub fn locale_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    system_path("locale.toml", path)
}
pub fn load_locale(path: impl AsRef<Path>) -> LocaleConfig {
    let Some(value) = parse_root(path) else {
        return LocaleConfig::default();
    };
    let ime = parse_string(value.get("ime"), "fcitx5");
    LocaleConfig {
        lang: parse_string(value.get("lang"), "en_US.UTF-8"),
        ime: matches!(ime.as_str(), "fcitx5" | "ibus" | "none")
            .then_some(ime)
            .unwrap_or_else(|| "fcitx5".into()),
        keymap: parse_string(value.get("keymap"), "us"),
    }
}
pub fn save_locale(path: impl AsRef<Path>, config: &LocaleConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth locale + IME preset\nlang = {}\nime = {}\nkeymap = {}\n",
            quote(&config.lang),
            quote(&config.ime),
            quote(&config.keymap)
        ),
        Some(0o600),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OomConfig {
    pub default_mem_pressure_limit: String,
    pub gaming_preference: String,
}
impl Default for OomConfig {
    fn default() -> Self {
        Self {
            default_mem_pressure_limit: "50%".into(),
            gaming_preference: "avoid".into(),
        }
    }
}
pub fn oom_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    system_path("oom.toml", path)
}
pub fn load_oom(path: impl AsRef<Path>) -> OomConfig {
    parse_root(path)
        .map(|v| OomConfig {
            default_mem_pressure_limit: parse_string(v.get("default_mem_pressure_limit"), "50%"),
            gaming_preference: parse_string(v.get("gaming_preference"), "avoid"),
        })
        .unwrap_or_default()
}
pub fn save_oom(path: impl AsRef<Path>, config: &OomConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth OOMD tuned\ndefault_mem_pressure_limit = {}\ngaming_preference = {}\n",
            quote(&config.default_mem_pressure_limit),
            quote(&config.gaming_preference)
        ),
        Some(0o600),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OomGamingConfig {
    pub profile: String,
    pub limit: String,
}
impl Default for OomGamingConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            limit: "50%".into(),
        }
    }
}
pub fn oom_gaming_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    system_path("oom-gaming.toml", path)
}
pub fn load_oom_gaming(path: impl AsRef<Path>) -> OomGamingConfig {
    let Some(value) = parse_root(path) else {
        return OomGamingConfig::default();
    };
    let profile = parse_string(value.get("profile"), "balanced").to_ascii_lowercase();
    let profile = matches!(profile.as_str(), "balanced" | "gaming")
        .then_some(profile)
        .unwrap_or_else(|| "balanced".into());
    let default_limit = if profile == "gaming" { "75%" } else { "50%" };
    let limit = parse_string(value.get("limit"), default_limit);
    OomGamingConfig {
        profile,
        limit: if limit.ends_with('%') {
            limit
        } else {
            default_limit.into()
        },
    }
}
pub fn save_oom_gaming(path: impl AsRef<Path>, config: &OomGamingConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth OOM gaming — offline\nprofile = {}\nlimit = {}\n",
            quote(&config.profile),
            quote(&config.limit)
        ),
        Some(0o600),
    )
}
pub fn generate_oom_gaming(
    config: &OomGamingConfig,
    destination: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let destination = destination.as_ref();
    if config.profile != "gaming" {
        match std::fs::remove_file(destination) {
            Ok(()) | Err(_) => {}
        }
        return Ok(None);
    }
    crate::atomic_io::atomic_write_text(
        destination,
        &format!(
            "# Kyth OOM gaming — generated\n[Unit]\nManagedOOMMemoryPressureLimit={}\n",
            config.limit
        ),
        Some(0o644),
    )?;
    Ok(Some(destination.to_path_buf()))
}
pub fn oom_gaming_status(destination: impl AsRef<Path>) -> &'static str {
    if destination.as_ref().is_file() {
        "gaming"
    } else {
        "balanced"
    }
}

pub fn etc_overlay_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    system_path("etc-overlay.toml", path)
}
pub fn load_etc_overlay(path: impl AsRef<Path>) -> BTreeMap<String, String> {
    let Some(value) = parse_root(path) else {
        return BTreeMap::new();
    };
    value
        .get("files")
        .and_then(toml::Value::as_table)
        .map(|files| {
            files
                .iter()
                .filter_map(|(dest, content)| Some((dest.clone(), content.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}
pub fn save_etc_overlay(
    path: impl AsRef<Path>,
    files: &BTreeMap<String, String>,
) -> std::io::Result<()> {
    let mut lines = vec![
        "# Kyth etc overlay — offline staged /etc merge".to_string(),
        "[files]".to_string(),
    ];
    for (dest, content) in files {
        lines.push(format!("{} = {}", quote(dest), quote(content)));
    }
    crate::atomic_io::atomic_write_text(path, &format!("{}\n", lines.join("\n")), Some(0o600))
}

#[derive(Debug, Clone, PartialEq)]
pub struct SteamDeadzoneConfig {
    pub profile: String,
    pub deadzone: f64,
}
impl Default for SteamDeadzoneConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            deadzone: 0.15,
        }
    }
}
pub fn steam_deadzone_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    system_path("steam-deadzone.toml", path)
}
pub fn load_steam_deadzone(path: impl AsRef<Path>) -> SteamDeadzoneConfig {
    let Some(value) = parse_root(path) else {
        return SteamDeadzoneConfig::default();
    };
    let raw_profile = parse_string(value.get("profile"), "balanced").to_ascii_lowercase();
    let profile = matches!(raw_profile.as_str(), "balanced" | "gaming")
        .then_some(raw_profile)
        .unwrap_or_else(|| "balanced".into());
    let default = if profile == "gaming" { 0.05 } else { 0.15 };
    SteamDeadzoneConfig {
        profile,
        deadzone: parse_f64(value.get("deadzone"), default).clamp(0.0, 0.3),
    }
}
pub fn save_steam_deadzone(
    path: impl AsRef<Path>,
    config: &SteamDeadzoneConfig,
) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth steam deadzone — offline\nprofile = {}\ndeadzone = {}\n",
            quote(&config.profile),
            config.deadzone
        ),
        Some(0o600),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelinuxGamingConfig {
    pub profile: String,
    pub allow_execheap: bool,
}
impl Default for SelinuxGamingConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            allow_execheap: false,
        }
    }
}
pub fn selinux_gaming_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    system_path("selinux-gaming.toml", path)
}
pub fn load_selinux_gaming(path: impl AsRef<Path>) -> SelinuxGamingConfig {
    let Some(value) = parse_root(path) else {
        return SelinuxGamingConfig::default();
    };
    let raw_profile = parse_string(value.get("profile"), "balanced").to_ascii_lowercase();
    let profile = matches!(raw_profile.as_str(), "balanced" | "gaming")
        .then_some(raw_profile)
        .unwrap_or_else(|| "balanced".into());
    SelinuxGamingConfig {
        profile: profile.clone(),
        allow_execheap: parse_bool(value.get("allow_execheap"), profile == "gaming"),
    }
}
pub fn save_selinux_gaming(
    path: impl AsRef<Path>,
    config: &SelinuxGamingConfig,
) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth selinux gaming — offline\nprofile = {}\nallow_execheap = {}\n",
            quote(&config.profile),
            config.allow_execheap
        ),
        Some(0o600),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn font_config_matches_defaults_and_clamps() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("fonts.toml");
        std::fs::write(&path, "hinting = \"invalid\"\ndpi = 999\n").unwrap();
        assert_eq!(
            load_fonts(&path),
            FontsConfig {
                dpi: 300,
                ..Default::default()
            }
        );
    }

    #[test]
    fn overlay_round_trip_preserves_multiline_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("etc-overlay.toml");
        let files = BTreeMap::from([("etc/example.conf".into(), "one\ntwo\"three".into())]);
        save_etc_overlay(&path, &files).unwrap();
        assert_eq!(load_etc_overlay(&path), files);
    }

    #[test]
    fn gaming_defaults_drive_deadzone_and_selinux() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("preset.toml");
        std::fs::write(&path, "profile = \"gaming\"\n").unwrap();
        assert_eq!(load_steam_deadzone(&path).deadzone, 0.05);
        assert!(load_selinux_gaming(&path).allow_execheap);
    }
}
