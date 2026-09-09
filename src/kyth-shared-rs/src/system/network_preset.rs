//! Declarative network preset (DoT + firewalld zone), offline.
//!
//! Mirrors `kyth_shared.network_preset`: validated load with safe defaults,
//! atomic `resolved.conf.d` drop-in with backup/rollback, and the TTL
//! marker. Only the `*_bin.rs` entry point touches the live filesystem.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkPreset {
    pub dns: String,
    pub doh: bool,
    pub firewall_zone: String,
}

impl Default for NetworkPreset {
    fn default() -> Self {
        Self {
            dns: "quad9".into(),
            doh: true,
            firewall_zone: "home".into(),
        }
    }
}

pub const RESOLVED_DROPIN: &str = "etc/systemd/resolved.conf.d/50-kyth.conf";
pub const TTL_PATH: &str = "/run/kyth-network-ttl";
pub const TTL_SECS: u64 = 30;

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(xdg).join("kyth/network.toml");
        }
    }
    PathBuf::from("/etc/kyth/network.toml")
}

fn validated(value: &toml::Value) -> NetworkPreset {
    let dns = value
        .get("dns")
        .and_then(toml::Value::as_str)
        .unwrap_or("quad9");
    let dns = matches!(dns, "quad9" | "cloudflare" | "off" | "google")
        .then(|| dns.to_string())
        .unwrap_or_else(|| "quad9".into());
    let zone = value
        .get("firewall_zone")
        .and_then(toml::Value::as_str)
        .unwrap_or("home");
    let firewall_zone = matches!(zone, "home" | "public" | "work")
        .then(|| zone.to_string())
        .unwrap_or_else(|| "home".into());
    NetworkPreset {
        dns,
        doh: value
            .get("doh")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true),
        firewall_zone,
    }
}

pub fn load(path: impl AsRef<Path>) -> NetworkPreset {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return NetworkPreset::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return NetworkPreset::default();
    };
    validated(&value)
}

pub fn dns_ip(preset: &NetworkPreset) -> &'static str {
    match preset.dns.as_str() {
        "cloudflare" => "1.1.1.1",
        "google" => "8.8.8.8",
        "off" => "",
        _ => "9.9.9.9",
    }
}

pub fn render_resolved_conf(preset: &NetworkPreset) -> String {
    // Corporate DHCP DNS servers commonly do not expose DNS-over-TLS.  Use
    // resolved's opportunistic mode so encrypted DNS remains preferred when
    // available without breaking per-link enterprise resolvers.
    format!(
        "[Resolve]\nDNS={}\nDNSOverTLS={}\n",
        dns_ip(preset),
        if preset.doh { "opportunistic" } else { "no" }
    )
}

/// Writes the drop-in under `root` with backup/rollback, exactly as the
/// Python launcher did (half-written DNS state is rolled back, never kept).
pub fn apply_preset(preset: &NetworkPreset, root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let dest = root.join(RESOLVED_DROPIN);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let backup = std::fs::read(&dest).ok();
    let write = (|| -> std::io::Result<()> {
        let tmp = dest.with_extension("tmp");
        std::fs::write(&tmp, render_resolved_conf(preset))?;
        std::fs::rename(&tmp, &dest)?;
        Ok(())
    })();
    if let Err(error) = write {
        match backup {
            None => {
                let _ = std::fs::remove_file(&dest);
            }
            Some(bytes) => {
                let _ = std::fs::write(&dest, bytes);
            }
        }
        return Err(error);
    }
    Ok(vec![dest])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn defaults_and_validates_preset_values() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("network.toml");
        assert_eq!(load(&missing), NetworkPreset::default());
        let path = dir.path().join("bad.toml");
        std::fs::write(
            &path,
            "dns = \"evil\"\ndoh = false\nfirewall_zone = \"dmz\"\n",
        )
        .unwrap();
        assert_eq!(
            load(&path),
            NetworkPreset {
                dns: "quad9".into(),
                doh: false,
                firewall_zone: "home".into()
            }
        );
    }

    #[test]
    fn renders_resolved_conf_like_python_launcher() {
        let preset = NetworkPreset::default();
        assert_eq!(
            render_resolved_conf(&preset),
            "[Resolve]\nDNS=9.9.9.9\nDNSOverTLS=opportunistic\n"
        );
        let off = NetworkPreset {
            dns: "off".into(),
            doh: false,
            firewall_zone: "public".into(),
        };
        assert_eq!(
            render_resolved_conf(&off),
            "[Resolve]\nDNS=\nDNSOverTLS=no\n"
        );
    }

    #[test]
    fn applies_dropin_atomically_with_rollback() {
        let dir = tempdir().unwrap();
        let preset = NetworkPreset::default();
        let written = apply_preset(&preset, dir.path()).unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(
            std::fs::read_to_string(&written[0]).unwrap(),
            "[Resolve]\nDNS=9.9.9.9\nDNSOverTLS=opportunistic\n"
        );
    }
}
