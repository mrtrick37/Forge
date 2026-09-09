//! Network-latency preference persistence and explicit sysctl drop-in rendering.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetLatencyConfig {
    pub enabled: bool,
    pub tcp_fastopen: i64,
    pub bbr: bool,
}

impl Default for NetLatencyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tcp_fastopen: 3,
            bbr: true,
        }
    }
}

fn normalize(config: NetLatencyConfig) -> NetLatencyConfig {
    NetLatencyConfig {
        enabled: config.enabled,
        tcp_fastopen: config.tcp_fastopen.clamp(0, 3),
        bbr: config.bbr,
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/net-latency.toml");
        }
    }
    PathBuf::from("/etc/kyth/net-latency.toml")
}

pub fn load(path: impl AsRef<Path>) -> NetLatencyConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return NetLatencyConfig::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return NetLatencyConfig::default();
    };
    normalize(NetLatencyConfig {
        enabled: value
            .get("enabled")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false),
        tcp_fastopen: value
            .get("tcp_fastopen")
            .and_then(toml::Value::as_integer)
            .unwrap_or(3),
        bbr: value
            .get("bbr")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true),
    })
}

pub fn save(path: impl AsRef<Path>, config: &NetLatencyConfig) -> std::io::Result<()> {
    let config = normalize(config.clone());
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth net latency — offline, gaming opt-in\nenabled = {}\ntcp_fastopen = {}\nbbr = {}\n",
            config.enabled, config.tcp_fastopen, config.bbr
        ),
        Some(0o600),
    )
}

pub fn generate(
    config: &NetLatencyConfig,
    destination: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let destination = destination.as_ref();
    let config = normalize(config.clone());
    if !config.enabled {
        match std::fs::remove_file(destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        return Ok(None);
    }
    let mut content =
        String::from("# Kyth net latency — generated, remove by disabling net-latency.toml\n");
    if config.bbr {
        content.push_str("net.ipv4.tcp_congestion_control = bbr\nnet.core.default_qdisc = fq\n");
    }
    content.push_str(&format!(
        "net.ipv4.tcp_fastopen = {}\nnet.ipv4.tcp_ecn = 1\nnet.ipv4.tcp_slow_start_after_idle = 0\nnet.core.rmem_max = 16777216\nnet.core.wmem_max = 16777216\nnet.ipv4.tcp_rmem = 4096 87380 16777216\nnet.ipv4.tcp_wmem = 4096 65536 16777216\n",
        config.tcp_fastopen
    ));
    crate::atomic_io::atomic_write_text(destination, &content, Some(0o644))?;
    Ok(Some(destination.to_path_buf()))
}

pub fn status(destination: impl AsRef<Path>) -> bool {
    destination.as_ref().is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clamps_network_config_and_renders_reversible_drop_in() {
        let directory = tempdir().unwrap();
        let config_path = directory.path().join("net-latency.toml");
        let drop_in = directory.path().join("net-latency.conf");
        std::fs::write(
            &config_path,
            "enabled = true\ntcp_fastopen = 99\nbbr = false\n",
        )
        .unwrap();
        let config = load(&config_path);
        assert_eq!(config.tcp_fastopen, 3);
        generate(&config, &drop_in).unwrap();
        let content = std::fs::read_to_string(&drop_in).unwrap();
        assert!(!content.contains("default_qdisc"));
        assert!(content.contains("tcp_fastopen = 3"));
        generate(&NetLatencyConfig::default(), &drop_in).unwrap();
        assert!(!drop_in.exists());
    }
}
