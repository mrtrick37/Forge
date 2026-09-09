//! Offline audio/network preference models and deterministic renderers.

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
fn parse_int(value: Option<&toml::Value>) -> Option<i64> {
    value
        .and_then(toml::Value::as_integer)
        .or_else(|| value.and_then(toml::Value::as_float).map(|v| v as i64))
}

pub fn pipewire_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    user_path("pipewire-latency.toml", path)
}
pub fn load_pipewire(path: impl AsRef<Path>) -> BTreeMap<String, i64> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return BTreeMap::new();
    };
    value
        .get("apps")
        .and_then(toml::Value::as_table)
        .map(|apps| {
            apps.iter()
                .filter_map(|(name, value)| {
                    Some((name.clone(), parse_int(Some(value))?.clamp(16, 2048)))
                })
                .collect()
        })
        .unwrap_or_default()
}
pub fn save_pipewire(path: impl AsRef<Path>, apps: &BTreeMap<String, i64>) -> std::io::Result<()> {
    let mut lines = vec![
        "# Kyth PipeWire latency — app → quantum, offline".to_string(),
        "[apps]".to_string(),
    ];
    for (app, quantum) in apps {
        lines.push(format!("{} = {}", quote(app), quantum));
    }
    crate::atomic_io::atomic_write_text(path, &format!("{}\n", lines.join("\n")), Some(0o600))
}
pub fn quantum_for_app(app: &str, path: impl AsRef<Path>) -> Option<i64> {
    load_pipewire(path).remove(app)
}
pub fn pipewire_env_for_app(
    app: &str,
    rate: i64,
    path: impl AsRef<Path>,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    if let Some(quantum) = quantum_for_app(app, path) {
        env.insert("PIPEWIRE_LATENCY".into(), format!("{quantum}/{rate}"));
    }
    env
}
pub fn default_pipewire_quantum(apps: &BTreeMap<String, i64>) -> Option<i64> {
    apps.get("default")
        .copied()
        .or_else(|| apps.get("*").copied())
}
pub fn pipewire_dropin(quantum: i64) -> String {
    format!("# Kyth PipeWire latency — default quantum {quantum}\ncontext.properties = {{\n  default.clock.quantum = {quantum}\n}}\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkConfig {
    pub dns: String,
    pub doh: bool,
    pub firewall_zone: String,
}
impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            dns: "quad9".into(),
            doh: true,
            firewall_zone: "home".into(),
        }
    }
}
pub fn network_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    system_path("network.toml", path)
}
pub fn load_network(path: impl AsRef<Path>) -> NetworkConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return NetworkConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return NetworkConfig::default();
    };
    let dns = value
        .get("dns")
        .and_then(toml::Value::as_str)
        .unwrap_or("quad9");
    let zone = value
        .get("firewall_zone")
        .and_then(toml::Value::as_str)
        .unwrap_or("home");
    NetworkConfig {
        dns: matches!(dns, "quad9" | "cloudflare" | "off" | "google")
            .then_some(dns)
            .unwrap_or("quad9")
            .into(),
        doh: value
            .get("doh")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true),
        firewall_zone: matches!(zone, "home" | "public" | "work")
            .then_some(zone)
            .unwrap_or("home")
            .into(),
    }
}
pub fn save_network(path: impl AsRef<Path>, config: &NetworkConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(path, &format!("# Kyth network preset — DoT + firewalld, offline\ndns = {}\ndoh = {}\nfirewall_zone = {}\n", quote(&config.dns), config.doh, quote(&config.firewall_zone)), Some(0o600))
}
pub fn dns_server(config: &NetworkConfig) -> &'static str {
    match config.dns.as_str() {
        "cloudflare" => "1.1.1.1",
        "google" => "8.8.8.8",
        "off" => "",
        _ => "9.9.9.9",
    }
}
pub fn resolved_dropin(config: &NetworkConfig) -> String {
    format!(
        "[Resolve]\nDNS={}\nDNSOverTLS={}\n",
        dns_server(config),
        if config.doh { "opportunistic" } else { "no" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn pipewire_clamps_and_projects_default_and_named_apps() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("pipewire.toml");
        let apps = BTreeMap::from([("default".into(), 8), ("game".into(), 1024)]);
        save_pipewire(&path, &apps).unwrap();
        let loaded = load_pipewire(&path);
        assert_eq!(loaded["default"], 16);
        assert_eq!(
            pipewire_env_for_app("game", 48000, &path)["PIPEWIRE_LATENCY"],
            "1024/48000"
        );
        assert_eq!(default_pipewire_quantum(&loaded), Some(16));
    }

    #[test]
    fn network_defaults_and_rendering_are_stable() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("network.toml");
        save_network(
            &path,
            &NetworkConfig {
                dns: "cloudflare".into(),
                doh: false,
                firewall_zone: "public".into(),
            },
        )
        .unwrap();
        let config = load_network(&path);
        assert_eq!(dns_server(&config), "1.1.1.1");
        assert!(resolved_dropin(&config).contains("DNSOverTLS=no"));
        assert!(resolved_dropin(&NetworkConfig::default()).contains("DNSOverTLS=opportunistic"));
    }
}
