//! Offline per-game policy and kernel-argument drift models.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn quote(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}
fn user_path(filename: &str, explicit: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = explicit {
        return path.as_ref().to_path_buf();
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join(format!("kyth/{filename}"));
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
        .join(format!(".config/kyth/{filename}"))
}

const VALID_GAME_PROFILES: &[&str] = &[
    "low-latency",
    "balanced",
    "battery",
    "quality",
    "hdr",
    "sharp",
    "latency",
];
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameProfile {
    pub profile: String,
    pub hdr: bool,
}
impl Default for GameProfile {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            hdr: false,
        }
    }
}
pub fn game_profile_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    user_path("gaming-per-game.toml", path)
}
pub fn load_game_profiles(path: impl AsRef<Path>) -> BTreeMap<String, GameProfile> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return BTreeMap::new();
    };
    value
        .get("games")
        .and_then(toml::Value::as_table)
        .map(|games| {
            games
                .iter()
                .filter_map(|(appid, value)| {
                    let table = value.as_table()?;
                    let raw_profile = table
                        .get("profile")
                        .and_then(toml::Value::as_str)
                        .unwrap_or("balanced");
                    let profile = VALID_GAME_PROFILES
                        .contains(&raw_profile)
                        .then_some(raw_profile)
                        .unwrap_or("balanced");
                    Some((
                        appid.clone(),
                        GameProfile {
                            profile: profile.into(),
                            hdr: table
                                .get("hdr")
                                .and_then(toml::Value::as_bool)
                                .unwrap_or(false),
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}
pub fn save_game_profiles(
    path: impl AsRef<Path>,
    games: &BTreeMap<String, GameProfile>,
) -> std::io::Result<()> {
    let mut lines = vec!["# Kyth per-game gaming config — HDR + latency profile".to_string()];
    for (appid, config) in games {
        lines.push(format!("[games.{}]", quote(appid)));
        lines.push(format!("profile = {}", quote(&config.profile)));
        lines.push(format!("hdr = {}", config.hdr));
        lines.push(String::new());
    }
    crate::atomic_io::atomic_write_text(path, &format!("{}\n", lines.join("\n")), Some(0o600))
}
pub fn profile_for_appid(appid: &str, path: impl AsRef<Path>) -> GameProfile {
    load_game_profiles(path).remove(appid).unwrap_or_default()
}
pub fn hdr_env_for_appid(appid: &str, path: impl AsRef<Path>) -> BTreeMap<String, String> {
    if profile_for_appid(appid, path).hdr {
        BTreeMap::from([("KYTH_HDR".into(), "1".into())])
    } else {
        BTreeMap::new()
    }
}

pub fn latency_profile(profile: &str) -> &'static str {
    match profile {
        "latency" => "low-latency",
        "quality" | "hdr" | "sharp" => "balanced",
        "low-latency" => "low-latency",
        "balanced" => "balanced",
        "battery" => "battery",
        _ => "balanced",
    }
}

/// Project a stored per-game profile into the minimal launch environment.
/// This is deliberately an environment map, not a process launcher.
pub fn gaming_launch_env_for_appid(
    appid: &str,
    path: impl AsRef<Path>,
) -> BTreeMap<String, String> {
    let config = profile_for_appid(appid, path);
    let mut env = match latency_profile(&config.profile) {
        "low-latency" => BTreeMap::from([
            ("LOW_LATENCY_LAYER".into(), "1".into()),
            ("MANGOHUD".into(), "1".into()),
        ]),
        "balanced" => BTreeMap::from([("MANGOHUD".into(), "1".into())]),
        _ => BTreeMap::new(),
    };
    if config.hdr {
        env.insert("KYTH_HDR".into(), "1".into());
    }
    env
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KargsConfig {
    pub profile: String,
    pub custom_add: Vec<String>,
    pub custom_remove: Vec<String>,
}
impl Default for KargsConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            custom_add: Vec::new(),
            custom_remove: Vec::new(),
        }
    }
}
pub fn kargs_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/kargs.toml");
        }
    }
    PathBuf::from("/etc/kyth/kargs.toml")
}
fn string_array(value: Option<&toml::Value>) -> Vec<String> {
    value
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    toml::Value::String(value) => Some(value.clone()),
                    toml::Value::Integer(value) => Some(value.to_string()),
                    toml::Value::Float(value) => Some(value.to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}
pub fn load_kargs(path: impl AsRef<Path>) -> KargsConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return KargsConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return KargsConfig::default();
    };
    let profile = value
        .get("profile")
        .and_then(toml::Value::as_str)
        .unwrap_or("balanced");
    KargsConfig {
        profile: matches!(profile, "balanced" | "performance" | "gaming")
            .then_some(profile)
            .unwrap_or("balanced")
            .into(),
        custom_add: string_array(value.get("custom_add")),
        custom_remove: string_array(value.get("custom_remove")),
    }
}
pub fn save_kargs(path: impl AsRef<Path>, config: &KargsConfig) -> std::io::Result<()> {
    let adds = config
        .custom_add
        .iter()
        .map(|value| quote(value))
        .collect::<Vec<_>>()
        .join(", ");
    let removes = config
        .custom_remove
        .iter()
        .map(|value| quote(value))
        .collect::<Vec<_>>()
        .join(", ");
    crate::atomic_io::atomic_write_text(path, &format!("# Kyth kargs perf profile — offline, revertible\nprofile = {}\ncustom_add = [{}]\ncustom_remove = [{}]\n", quote(&config.profile), adds, removes), Some(0o600))
}
pub fn desired_kargs(config: &KargsConfig) -> Vec<String> {
    let mut args = match config.profile.as_str() {
        "performance" => vec![
            "amd_pstate=active",
            "preempt=full",
            "transparent_hugepage=madvise",
        ],
        "gaming" => vec![
            "amd_pstate=active",
            "preempt=full",
            "transparent_hugepage=madvise",
            "mitigations=off",
        ],
        _ => Vec::new(),
    }
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();
    for arg in &config.custom_add {
        if !arg.is_empty() && !args.contains(arg) {
            args.push(arg.clone());
        }
    }
    args
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KargsDrift {
    pub missing: Vec<String>,
    pub extra: Vec<String>,
    pub desired: Vec<String>,
    pub profile: String,
}
pub fn kargs_drift(config: &KargsConfig, cmdline: &str) -> KargsDrift {
    let desired = desired_kargs(config);
    KargsDrift {
        missing: desired
            .iter()
            .filter(|arg| !cmdline.contains(arg.as_str()))
            .cloned()
            .collect(),
        extra: config
            .custom_remove
            .iter()
            .filter(|arg| !arg.is_empty() && cmdline.contains(arg.as_str()))
            .cloned()
            .collect(),
        desired,
        profile: config.profile.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn game_profiles_round_trip_and_project_hdr() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("gaming.toml");
        let games = BTreeMap::from([(
            "123\"4".into(),
            GameProfile {
                profile: "latency".into(),
                hdr: true,
            },
        )]);
        save_game_profiles(&path, &games).unwrap();
        assert_eq!(load_game_profiles(&path), games);
        assert_eq!(hdr_env_for_appid("123\"4", &path)["KYTH_HDR"], "1");
        assert_eq!(
            gaming_launch_env_for_appid("123\"4", &path)["LOW_LATENCY_LAYER"],
            "1"
        );
        assert_eq!(
            gaming_launch_env_for_appid("123\"4", &path)["MANGOHUD"],
            "1"
        );
    }

    #[test]
    fn maps_ui_profiles_to_launch_profiles() {
        assert_eq!(latency_profile("quality"), "balanced");
        assert_eq!(latency_profile("latency"), "low-latency");
        assert_eq!(latency_profile("unknown"), "balanced");
    }

    #[test]
    fn kargs_drift_is_read_only_and_deduplicates_custom_adds() {
        let config = KargsConfig {
            profile: "gaming".into(),
            custom_add: vec!["foo=1".into(), "amd_pstate=active".into()],
            custom_remove: vec!["quiet".into()],
        };
        let drift = kargs_drift(&config, "amd_pstate=active quiet");
        assert_eq!(
            drift.missing,
            vec![
                "preempt=full",
                "transparent_hugepage=madvise",
                "mitigations=off",
                "foo=1"
            ]
        );
        assert_eq!(drift.extra, vec!["quiet"]);
    }
}
