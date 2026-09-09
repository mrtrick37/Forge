//! Offline runtime preference files and generated snippets.

use std::path::{Path, PathBuf};

fn quote(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
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
fn parse(path: impl AsRef<Path>) -> Option<toml::Value> {
    std::fs::read_to_string(path).ok()?.parse().ok()
}
fn profile(value: Option<&toml::Value>, allowed: &[&str], default: &str) -> String {
    let value = value
        .and_then(toml::Value::as_str)
        .unwrap_or(default)
        .to_ascii_lowercase();
    allowed
        .contains(&value.as_str())
        .then_some(value)
        .unwrap_or_else(|| default.into())
}
fn bool_value(value: Option<&toml::Value>, default: bool) -> bool {
    value.and_then(toml::Value::as_bool).unwrap_or(default)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrimConfig {
    pub profile: String,
    pub weekly: bool,
}
impl Default for TrimConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            weekly: true,
        }
    }
}
pub fn trim_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    system_path("trim.toml", path)
}
pub fn load_trim(path: impl AsRef<Path>) -> TrimConfig {
    parse(path)
        .map(|v| TrimConfig {
            profile: profile(v.get("profile"), &["balanced", "kyth"], "balanced"),
            weekly: bool_value(v.get("weekly"), true),
        })
        .unwrap_or_default()
}
pub fn save_trim(path: impl AsRef<Path>, config: &TrimConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth trim — offline\nprofile = {}\nweekly = {}\n",
            quote(&config.profile),
            config.weekly
        ),
        Some(0o600),
    )
}
pub fn generate_trim_marker(
    config: &TrimConfig,
    marker: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let marker = marker.as_ref();
    if config.profile != "kyth" {
        match std::fs::remove_file(marker) {
            Ok(()) | Err(_) => {}
        }
        return Ok(None);
    }
    crate::atomic_io::atomic_write_text(marker, "kyth-nodiscard,weekly\n", Some(0o644))?;
    Ok(Some(marker.to_path_buf()))
}
pub fn trim_status(marker: impl AsRef<Path>) -> &'static str {
    if marker.as_ref().exists() {
        "kyth"
    } else {
        "balanced"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UksmdConfig {
    pub enabled: bool,
    pub max_cpu_percent: i64,
}
impl Default for UksmdConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_cpu_percent: 20,
        }
    }
}
pub fn uksmd_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    system_path("uksmd.toml", path)
}
pub fn load_uksmd(path: impl AsRef<Path>) -> UksmdConfig {
    parse(path)
        .map(|v| UksmdConfig {
            enabled: bool_value(v.get("enabled"), false),
            max_cpu_percent: v
                .get("max_cpu_percent")
                .and_then(toml::Value::as_integer)
                .unwrap_or(20)
                .clamp(5, 80),
        })
        .unwrap_or_default()
}
pub fn save_uksmd(path: impl AsRef<Path>, config: &UksmdConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth uksmd — offline, opt-in\nenabled = {}\nmax_cpu_percent = {}\n",
            config.enabled, config.max_cpu_percent
        ),
        Some(0o600),
    )
}
pub fn uksmd_suggested(meminfo: impl AsRef<Path>) -> bool {
    std::fs::read_to_string(meminfo)
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("MemTotal:"))
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|value| value.parse::<f64>().ok())
        })
        .is_some_and(|kb| kb / 1024.0 / 1024.0 >= 15.5)
}
pub fn generate_uksmd(
    config: &UksmdConfig,
    destination: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let destination = destination.as_ref();
    if !config.enabled {
        if destination.exists()
            && std::fs::read_to_string(destination)
                .ok()
                .is_some_and(|text| text.contains("Kyth"))
        {
            std::fs::remove_file(destination)?;
        }
        return Ok(None);
    }
    crate::atomic_io::atomic_write_text(destination, &format!("# Kyth uksmd — generated\n[daemon]\nmax_cpu_percent = {}\nscan_sleep_millisecs = 200\n", config.max_cpu_percent), Some(0o644))?;
    Ok(Some(destination.to_path_buf()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalConfig {
    pub perf: bool,
    pub system_max_use: String,
    pub runtime_max_use: String,
}
impl Default for JournalConfig {
    fn default() -> Self {
        Self {
            perf: false,
            system_max_use: "500M".into(),
            runtime_max_use: "128M".into(),
        }
    }
}
pub fn journal_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    system_path("journal.toml", path)
}
pub fn load_journal(path: impl AsRef<Path>) -> JournalConfig {
    let Some(value) = parse(path) else {
        return JournalConfig::default();
    };
    let perf = bool_value(value.get("perf"), false);
    let default_system = if perf { "200M" } else { "500M" };
    let default_runtime = if perf { "64M" } else { "128M" };
    let system = value
        .get("system_max_use")
        .and_then(toml::Value::as_str)
        .filter(|v| !v.is_empty() && v.len() <= 16)
        .unwrap_or(default_system);
    let runtime = value
        .get("runtime_max_use")
        .and_then(toml::Value::as_str)
        .filter(|v| !v.is_empty() && v.len() <= 16)
        .unwrap_or(default_runtime);
    JournalConfig {
        perf,
        system_max_use: system.into(),
        runtime_max_use: runtime.into(),
    }
}
pub fn save_journal(path: impl AsRef<Path>, config: &JournalConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth journal slim — offline\nperf = {}\nsystem_max_use = {}\nruntime_max_use = {}\n",
            config.perf,
            quote(&config.system_max_use),
            quote(&config.runtime_max_use)
        ),
        Some(0o600),
    )
}
pub fn generate_journal(
    config: &JournalConfig,
    destination: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let destination = destination.as_ref();
    if !config.perf {
        match std::fs::remove_file(destination) {
            Ok(()) | Err(_) => {}
        }
        return Ok(None);
    }
    crate::atomic_io::atomic_write_text(destination, &format!("# Kyth journal perf — generated, disable via journal.toml perf=false\n[Journal]\nSystemMaxUse={}\nRuntimeMaxUse={}\nMaxRetentionSec=14day\nForwardToSyslog=no\nCompress=yes\n", config.system_max_use, config.runtime_max_use), Some(0o644))?;
    Ok(Some(destination.to_path_buf()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrqConfig {
    pub profile: String,
    pub isolated_cpus: String,
}
impl Default for IrqConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            isolated_cpus: String::new(),
        }
    }
}
pub fn irq_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    system_path("irq.toml", path)
}
pub fn load_irq(path: impl AsRef<Path>) -> IrqConfig {
    parse(path)
        .map(|v| {
            let cpus = v
                .get("isolated_cpus")
                .and_then(toml::Value::as_str)
                .unwrap_or("")
                .trim();
            IrqConfig {
                profile: profile(v.get("profile"), &["balanced", "kyth"], "balanced"),
                isolated_cpus: (!cpus.is_empty()
                    && cpus
                        .chars()
                        .all(|c| c.is_ascii_digit() || c == ',' || c == '-'))
                .then_some(cpus)
                .unwrap_or("")
                .into(),
            }
        })
        .unwrap_or_default()
}
pub fn save_irq(path: impl AsRef<Path>, config: &IrqConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth IRQ affinity — offline\nprofile = {}\nisolated_cpus = {}\n",
            quote(&config.profile),
            quote(&config.isolated_cpus)
        ),
        Some(0o600),
    )
}
pub fn generate_irq(
    config: &IrqConfig,
    destination: impl AsRef<Path>,
    cpus: &str,
) -> std::io::Result<Option<PathBuf>> {
    let destination = destination.as_ref();
    if config.profile != "kyth" {
        match std::fs::remove_file(destination) {
            Ok(()) | Err(_) => {}
        }
        return Ok(None);
    }
    let banned = if config.isolated_cpus.is_empty() {
        if cpus.is_empty() {
            "1"
        } else {
            cpus
        }
    } else {
        &config.isolated_cpus
    };
    crate::atomic_io::atomic_write_text(destination, &format!("# Kyth IRQ affinity — generated\n[Service]\nExecStart=\nExecStart=/usr/sbin/irqbalance --banirq=0 --banned-cpus={banned}\n"), Some(0o644))?;
    Ok(Some(destination.to_path_buf()))
}
pub fn irq_status(destination: impl AsRef<Path>) -> &'static str {
    if destination.as_ref().is_file() {
        "kyth"
    } else {
        "balanced"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FscacheConfig {
    pub enabled: bool,
}
impl Default for FscacheConfig {
    fn default() -> Self {
        Self { enabled: false }
    }
}
pub fn fscache_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    system_path("fscache.toml", path)
}
pub fn load_fscache(path: impl AsRef<Path>) -> FscacheConfig {
    parse(path)
        .map(|v| FscacheConfig {
            enabled: bool_value(v.get("enabled"), false),
        })
        .unwrap_or_default()
}
pub fn save_fscache(path: impl AsRef<Path>, config: &FscacheConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!("# Kyth fscache — offline\nenabled = {}\n", config.enabled),
        Some(0o600),
    )
}
pub fn generate_fscache(
    config: &FscacheConfig,
    destination: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let destination = destination.as_ref();
    if !config.enabled {
        match std::fs::remove_file(destination) {
            Ok(()) | Err(_) => {}
        }
        return Ok(None);
    }
    crate::atomic_io::atomic_write_text(
        destination,
        "# Kyth fscache — generated\ndir /var/cache/fscache cachefiles:0x3f 10G\n",
        Some(0o644),
    )?;
    Ok(Some(destination.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn generated_runtime_files_are_reversible() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("journal.conf");
        let config = JournalConfig {
            perf: true,
            ..Default::default()
        };
        assert!(generate_journal(&config, &path).unwrap().is_some());
        assert!(path.exists());
        assert!(generate_journal(&JournalConfig::default(), &path)
            .unwrap()
            .is_none());
        assert!(!path.exists());
    }

    #[test]
    fn config_models_clamp_and_sanitize() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("uksmd.toml");
        std::fs::write(&path, "enabled = true\nmax_cpu_percent = 100\n").unwrap();
        assert_eq!(
            load_uksmd(&path),
            UksmdConfig {
                enabled: true,
                max_cpu_percent: 80
            }
        );
        let irq = dir.path().join("irq.toml");
        std::fs::write(&irq, "profile = \"kyth\"\nisolated_cpus = \"0,2-3\"\n").unwrap();
        assert_eq!(load_irq(&irq).isolated_cpus, "0,2-3");
    }
}
