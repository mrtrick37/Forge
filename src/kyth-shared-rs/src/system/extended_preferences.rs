//! Additional deterministic desktop and gaming preference helpers.

use std::collections::BTreeMap;
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
pub struct FanCurveConfig {
    pub points: Vec<(i64, i64)>,
    pub power_cap_w: i64,
}
impl Default for FanCurveConfig {
    fn default() -> Self {
        Self {
            points: vec![(40, 30), (70, 80), (85, 100)],
            power_cap_w: 0,
        }
    }
}
pub fn fan_curve_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    system_path("fan-curve.toml", path)
}
pub fn load_fan_curve(path: impl AsRef<Path>) -> FanCurveConfig {
    let Some(value) = parse(path) else {
        return FanCurveConfig::default();
    };
    let points = value
        .get("points")
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let pair = item.as_array()?;
                    Some((pair.first()?.as_integer()?, pair.get(1)?.as_integer()?))
                })
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| FanCurveConfig::default().points);
    FanCurveConfig {
        points,
        power_cap_w: value
            .get("power_cap_w")
            .and_then(toml::Value::as_integer)
            .unwrap_or(0)
            .clamp(0, 300),
    }
}
pub fn save_fan_curve(path: impl AsRef<Path>, config: &FanCurveConfig) -> std::io::Result<()> {
    let points = config
        .points
        .iter()
        .map(|(temp, pwm)| format!("[{temp}, {pwm}]"))
        .collect::<Vec<_>>()
        .join(", ");
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth fan curve — temp C -> pwm %, offline\npoints = [{points}]\npower_cap_w = {}\n",
            config.power_cap_w
        ),
        Some(0o600),
    )
}
pub fn pwm_for_temp(temp_c: f64, points: &[(i64, i64)]) -> i64 {
    let mut points = points.to_vec();
    points.sort_by_key(|point| point.0);
    if points.is_empty() {
        return 0;
    }
    if temp_c <= points[0].0 as f64 {
        return points[0].1;
    }
    if temp_c >= points[points.len() - 1].0 as f64 {
        return points[points.len() - 1].1;
    }
    for window in points.windows(2) {
        let (t0, p0) = window[0];
        let (t1, p1) = window[1];
        if temp_c >= t0 as f64 && temp_c <= t1 as f64 {
            let fraction = (temp_c - t0 as f64) / (1.max(t1 - t0) as f64);
            return (p0 as f64 + fraction * (p1 - p0) as f64) as i64;
        }
    }
    points.last().unwrap().1
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FcitxLatencyConfig {
    pub profile: String,
    pub latency_ms: i64,
}
impl Default for FcitxLatencyConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            latency_ms: 50,
        }
    }
}
pub fn fcitx_latency_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    system_path("fcitx-latency.toml", path)
}
pub fn load_fcitx_latency(path: impl AsRef<Path>) -> FcitxLatencyConfig {
    parse(path)
        .map(|v| {
            let profile = profile(v.get("profile"), &["balanced", "gaming"], "balanced");
            let default = if profile == "gaming" { 10 } else { 50 };
            FcitxLatencyConfig {
                profile,
                latency_ms: v
                    .get("latency_ms")
                    .and_then(toml::Value::as_integer)
                    .unwrap_or(default)
                    .clamp(5, 100),
            }
        })
        .unwrap_or_default()
}
pub fn save_fcitx_latency(
    path: impl AsRef<Path>,
    config: &FcitxLatencyConfig,
) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth fcitx5 latency — offline\nprofile = {}\nlatency_ms = {}\n",
            quote(&config.profile),
            config.latency_ms
        ),
        Some(0o600),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipewireGamingConfig {
    pub profile: String,
    pub quantum: i64,
}
impl Default for PipewireGamingConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            quantum: 128,
        }
    }
}
pub fn pipewire_gaming_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    system_path("pipewire-gaming.toml", path)
}
pub fn load_pipewire_gaming(path: impl AsRef<Path>) -> PipewireGamingConfig {
    parse(path)
        .map(|v| PipewireGamingConfig {
            profile: profile(v.get("profile"), &["balanced", "gaming"], "balanced"),
            quantum: v
                .get("quantum")
                .and_then(toml::Value::as_integer)
                .unwrap_or(128)
                .clamp(32, 2048),
        })
        .unwrap_or_default()
}
pub fn save_pipewire_gaming(
    path: impl AsRef<Path>,
    config: &PipewireGamingConfig,
) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth PipeWire gaming — offline\nprofile = {}\nquantum = {}\n",
            quote(&config.profile),
            config.quantum
        ),
        Some(0o600),
    )
}
pub fn generate_pipewire_gaming(
    config: &PipewireGamingConfig,
    destination: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let destination = destination.as_ref();
    if config.profile != "gaming" {
        match std::fs::remove_file(destination) {
            Ok(()) | Err(_) => {}
        }
        return Ok(None);
    }
    let content = r#"-- Kyth PipeWire gaming — generated
table.insert(alsa_monitor.rules, {
  matches = {{{ "node.name", "matches", "alsa_output.*" }}},
  apply_properties = {
    ["api.alsa.period-size"] = __Q__,
    ["api.alsa.headroom"] = __Q__,
  },
})
"#
    .replace("__Q__", &config.quantum.to_string());
    crate::atomic_io::atomic_write_text(destination, &content, Some(0o644))?;
    Ok(Some(destination.to_path_buf()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcieConfig {
    pub profile: String,
}
impl Default for PcieConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
        }
    }
}
pub fn load_pcie(path: impl AsRef<Path>) -> PcieConfig {
    parse(path)
        .map(|v| PcieConfig {
            profile: profile(v.get("profile"), &["balanced", "gaming"], "balanced"),
        })
        .unwrap_or_default()
}
pub fn save_pcie(path: impl AsRef<Path>, config: &PcieConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth PCIe ASPM — offline\nprofile = {}\n",
            quote(&config.profile)
        ),
        Some(0o600),
    )
}
pub fn generate_pcie(
    config: &PcieConfig,
    destination: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let destination = destination.as_ref();
    if config.profile != "gaming" {
        match std::fs::remove_file(destination) {
            Ok(()) | Err(_) => {}
        }
        return Ok(None);
    }
    crate::atomic_io::atomic_write_text(destination, "# Kyth PCIe ASPM gaming — generated\nACTION==\"add\", SUBSYSTEM==\"pci\", ATTR{link/l1_aspm}=\"0\"\n", Some(0o644))?;
    Ok(Some(destination.to_path_buf()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PsiConfig {
    pub profile: String,
}
impl Default for PsiConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
        }
    }
}
pub fn load_psi(path: impl AsRef<Path>) -> PsiConfig {
    parse(path)
        .map(|v| PsiConfig {
            profile: profile(v.get("profile"), &["balanced", "gaming"], "balanced"),
        })
        .unwrap_or_default()
}
pub fn save_psi(path: impl AsRef<Path>, config: &PsiConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth PSI gaming — offline\nprofile = {}\n",
            quote(&config.profile)
        ),
        Some(0o600),
    )
}
pub fn generate_psi(
    config: &PsiConfig,
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
        "# Kyth PSI gaming — generated\n[Slice]\nMemoryHigh=90%\nManagedOOMPreference=avoid\n",
        Some(0o644),
    )?;
    Ok(Some(destination.to_path_buf()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WineSyncConfig {
    pub mode: String,
}
impl Default for WineSyncConfig {
    fn default() -> Self {
        Self {
            mode: "auto".into(),
        }
    }
}
pub fn wine_sync_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    system_path("wine-sync.toml", path)
}
pub fn load_wine_sync(path: impl AsRef<Path>) -> WineSyncConfig {
    parse(path)
        .map(|v| WineSyncConfig {
            mode: profile(
                v.get("mode"),
                &["auto", "ntsync", "fsync", "esync", "off"],
                "auto",
            ),
        })
        .unwrap_or_default()
}
pub fn save_wine_sync(path: impl AsRef<Path>, config: &WineSyncConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth wine sync — offline\nmode = {}\n",
            quote(&config.mode)
        ),
        Some(0o600),
    )
}
pub fn wine_env_for_mode(
    mode: &str,
    ntsync_available: bool,
    futex2_available: bool,
) -> Option<String> {
    let selected = match mode {
        "off" => return None,
        "auto" if ntsync_available => "ntsync",
        "auto" if futex2_available => "fsync",
        "auto" => "esync",
        value => value,
    };
    let vars = match selected {
        "ntsync" => "WINEFSYNC=1\nNTSYNC=1\n",
        "fsync" => "WINEFSYNC=1\n",
        "esync" => "WINEESYNC=1\n",
        _ => "WINEFSYNC=1\n",
    };
    Some(format!("# Kyth wine sync — generated {selected}\n{vars}"))
}
pub fn wine_sync_status(content: Option<&str>) -> &'static str {
    let Some(content) = content else {
        return "off";
    };
    if content.contains("NTSYNC") {
        "ntsync"
    } else if content.contains("WINEFSYNC") {
        "fsync"
    } else if content.contains("WINEESYNC") {
        "esync"
    } else {
        "unknown"
    }
}
pub fn probe_wine_sync() -> (bool, bool) {
    let ntsync = Path::new("/dev/ntsync").exists() || Path::new("/sys/module/ntsync").exists();
    let futex2 = std::fs::read_to_string("/proc/version")
        .map(|value| value.contains("6."))
        .unwrap_or(false);
    (ntsync, futex2)
}
pub fn generate_wine_env(
    config: &WineSyncConfig,
    destination: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let destination = destination.as_ref();
    let (ntsync, futex2) = probe_wine_sync();
    let content = wine_env_for_mode(&config.mode, ntsync, futex2);
    if let Some(content) = content {
        crate::atomic_io::atomic_write_text(destination, &content, Some(0o644))?;
        Ok(Some(destination.to_path_buf()))
    } else {
        match std::fs::remove_file(destination) {
            Ok(()) | Err(_) => {}
        }
        Ok(None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimallocConfig {
    pub enabled: bool,
    pub global: bool,
    pub per_game: bool,
}
impl Default for MimallocConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            global: false,
            per_game: true,
        }
    }
}
pub fn load_mimalloc(path: impl AsRef<Path>) -> MimallocConfig {
    parse(path)
        .map(|v| MimallocConfig {
            enabled: bool_value(v.get("enabled"), false),
            global: bool_value(v.get("global"), false),
            per_game: bool_value(v.get("per_game"), true),
        })
        .unwrap_or_default()
}
pub fn save_mimalloc(path: impl AsRef<Path>, config: &MimallocConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(path, &format!("# Kyth mimalloc — offline, per-game wrapper\nenabled = {}\nglobal = {}\nper_game = {}\n", config.enabled, config.global, config.per_game), Some(0o600))
}
pub fn mimalloc_env(config: &MimallocConfig, library: &str) -> Option<String> {
    if !config.enabled || !config.global {
        return None;
    }
    Some(format!("# Kyth mimalloc — generated, global preload (opt-in)\nLD_PRELOAD={library}\nMIMALLOC_LARGE_OS_PAGES=1\n"))
}
pub fn find_mimalloc_library() -> String {
    [
        "/usr/lib64/libmimalloc.so.2",
        "/usr/lib64/libmimalloc.so",
        "/usr/lib/libmimalloc.so",
    ]
    .iter()
    .find(|path| Path::new(**path).is_file())
    .unwrap_or(&"/usr/lib64/libmimalloc.so.2")
    .to_string()
}
pub fn generate_mimalloc_env(
    config: &MimallocConfig,
    destination: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let destination = destination.as_ref();
    let content = mimalloc_env(config, &find_mimalloc_library());
    if let Some(content) = content {
        crate::atomic_io::atomic_write_text(destination, &content, Some(0o644))?;
        Ok(Some(destination.to_path_buf()))
    } else {
        match std::fs::remove_file(destination) {
            Ok(()) | Err(_) => {}
        }
        Ok(None)
    }
}
pub fn mimalloc_status(config: &MimallocConfig, environment_exists: bool) -> &'static str {
    if environment_exists {
        "global"
    } else if config.enabled {
        "per-game"
    } else {
        "off"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SccacheConfig {
    pub enabled: bool,
    pub size: String,
}
impl Default for SccacheConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            size: "10G".into(),
        }
    }
}
pub fn load_sccache(path: impl AsRef<Path>) -> SccacheConfig {
    parse(path)
        .map(|v| {
            let size = v.get("size").and_then(toml::Value::as_str).unwrap_or("10G");
            SccacheConfig {
                enabled: bool_value(v.get("enabled"), false),
                size: matches!(size, "5G" | "10G" | "20G" | "50G")
                    .then_some(size)
                    .unwrap_or("10G")
                    .into(),
            }
        })
        .unwrap_or_default()
}
pub fn save_sccache(path: impl AsRef<Path>, config: &SccacheConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth sccache — offline\nenabled = {}\nsize = {}\n",
            config.enabled,
            quote(&config.size)
        ),
        Some(0o600),
    )
}
pub fn sccache_env(config: &SccacheConfig) -> Option<String> {
    config.enabled.then(|| {
        format!(
            "# Kyth sccache — generated\nSCCACHE_DIR=/var/cache/sccache\nSCCACHE_CACHE_SIZE={}\n",
            config.size
        )
    })
}
pub fn sccache_service(config: &SccacheConfig) -> Option<String> {
    config.enabled.then(|| format!("[Unit]\nDescription=Kyth sccache server — Rust/C cache\nAfter=network.target\n[Service]\nType=simple\nEnvironment=SCCACHE_DIR=/var/cache/sccache\nEnvironment=SCCACHE_CACHE_SIZE={}\nExecStart=/usr/bin/sccache --start-server\nExecStop=/usr/bin/sccache --stop-server\nRestart=on-failure\n[Install]\nWantedBy=multi-user.target\n", config.size))
}
pub fn generate_sccache(
    config: &SccacheConfig,
    environment: impl AsRef<Path>,
    service: impl AsRef<Path>,
) -> std::io::Result<Option<PathBuf>> {
    let environment = environment.as_ref();
    let service = service.as_ref();
    let (Some(environment_content), Some(service_content)) =
        (sccache_env(config), sccache_service(config))
    else {
        for path in [environment, service] {
            match std::fs::remove_file(path) {
                Ok(()) | Err(_) => {}
            }
        }
        return Ok(None);
    };
    crate::atomic_io::atomic_write_text(environment, &environment_content, Some(0o644))?;
    crate::atomic_io::atomic_write_text(service, &service_content, Some(0o644))?;
    Ok(Some(environment.to_path_buf()))
}
pub fn sccache_status(environment: impl AsRef<Path>) -> &'static str {
    if environment.as_ref().is_file() {
        "enabled"
    } else {
        "off"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderSizeConfig {
    pub mode: String,
    pub size: String,
}
impl Default for ShaderSizeConfig {
    fn default() -> Self {
        Self {
            mode: "auto".into(),
            size: "2G".into(),
        }
    }
}
pub fn load_shader_size(path: impl AsRef<Path>) -> ShaderSizeConfig {
    parse(path)
        .map(|v| {
            let mode = v
                .get("mode")
                .and_then(toml::Value::as_str)
                .unwrap_or("auto");
            let size = v.get("size").and_then(toml::Value::as_str).unwrap_or("2G");
            ShaderSizeConfig {
                mode: matches!(mode, "auto" | "manual")
                    .then_some(mode)
                    .unwrap_or("auto")
                    .into(),
                size: matches!(size, "1G" | "2G" | "4G" | "8G")
                    .then_some(size)
                    .unwrap_or("2G")
                    .into(),
            }
        })
        .unwrap_or_default()
}
pub fn save_shader_size(path: impl AsRef<Path>, config: &ShaderSizeConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth shader cache size — offline\nmode = {}\nsize = {}\n",
            quote(&config.mode),
            quote(&config.size)
        ),
        Some(0o600),
    )
}
pub fn resolve_shader_size(config: &ShaderSizeConfig, vram_gb: u64) -> String {
    if config.mode == "manual" {
        config.size.clone()
    } else if vram_gb >= 8 {
        "4G".into()
    } else {
        "2G".into()
    }
}
pub fn shader_size_env(size: &str) -> String {
    format!("# Kyth shader cache size — generated\nMESA_SHADER_CACHE_MAX_SIZE={size}\n__GL_SHADER_DISK_CACHE_SIZE={size}\n")
}
pub fn generate_shader_size(
    config: &ShaderSizeConfig,
    vram_gb: u64,
    destination: impl AsRef<Path>,
) -> std::io::Result<PathBuf> {
    let destination = destination.as_ref();
    let size = resolve_shader_size(config, vram_gb);
    crate::atomic_io::atomic_write_text(destination, &shader_size_env(&size), Some(0o644))?;
    Ok(destination.to_path_buf())
}
pub fn shader_size_status(destination: impl AsRef<Path>) -> &'static str {
    if destination.as_ref().is_file() {
        "enabled"
    } else {
        "off"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtrfsPerfConfig {
    pub profile: String,
    pub compress: String,
}
impl Default for BtrfsPerfConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            compress: "zstd:1".into(),
        }
    }
}
pub fn load_btrfs_perf(path: impl AsRef<Path>) -> BtrfsPerfConfig {
    parse(path)
        .map(|v| {
            let compress = v
                .get("compress")
                .and_then(toml::Value::as_str)
                .unwrap_or("zstd:1");
            BtrfsPerfConfig {
                profile: profile(v.get("profile"), &["balanced", "kyth"], "balanced"),
                compress: matches!(compress, "zstd:1" | "zstd:3" | "zstd" | "lzo" | "off")
                    .then_some(compress)
                    .unwrap_or("zstd:1")
                    .into(),
            }
        })
        .unwrap_or_default()
}
pub fn save_btrfs_perf(path: impl AsRef<Path>, config: &BtrfsPerfConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth btrfs perf — offline\nprofile = {}\ncompress = {}\n",
            quote(&config.profile),
            quote(&config.compress)
        ),
        Some(0o600),
    )
}
pub fn btrfs_mount_options(compress: &str) -> String {
    let compress = if compress == "zstd" {
        "zstd:1"
    } else {
        compress
    };
    if compress == "off" {
        "noatime,space_cache=v2,commit=120".into()
    } else {
        format!("compress-force={compress},noatime,space_cache=v2,commit=120")
    }
}
pub fn btrfs_perf_dropin(config: &BtrfsPerfConfig) -> Option<String> {
    (config.profile == "kyth").then(|| {
        format!(
            "# Kyth btrfs perf — generated\n[Mount]\nOptions={}\n",
            btrfs_mount_options(&config.compress)
        )
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamingCfsConfig {
    pub profile: String,
}
impl Default for GamingCfsConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
        }
    }
}
pub fn load_gaming_cfs(path: impl AsRef<Path>) -> GamingCfsConfig {
    parse(path)
        .map(|v| GamingCfsConfig {
            profile: profile(v.get("profile"), &["balanced", "gaming"], "balanced"),
        })
        .unwrap_or_default()
}
pub fn save_gaming_cfs(path: impl AsRef<Path>, config: &GamingCfsConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth gaming CFS — offline\nprofile = {}\n",
            quote(&config.profile)
        ),
        Some(0o600),
    )
}
pub fn gaming_cfs_dropin(config: &GamingCfsConfig) -> Option<&'static str> {
    (config.profile == "gaming").then_some("# Kyth gaming CFS burst — generated\n[Slice]\nCPUQuota=400%\nCPUWeight=800\nIOWeight=800\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EppAcConfig {
    pub enabled: bool,
}
impl Default for EppAcConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}
pub fn load_epp_ac(path: impl AsRef<Path>) -> EppAcConfig {
    parse(path)
        .map(|v| EppAcConfig {
            enabled: bool_value(v.get("enabled"), true),
        })
        .unwrap_or_default()
}
pub fn save_epp_ac(path: impl AsRef<Path>, config: &EppAcConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!("# Kyth EPP AC — offline\nenabled = {}\n", config.enabled),
        Some(0o600),
    )
}
pub fn epp_ac_rule(config: &EppAcConfig) -> Option<&'static str> {
    config.enabled.then_some("# Kyth EPP AC — generated\nSUBSYSTEM==\"power_supply\", ATTR{online}==\"1\", RUN+=\"/usr/bin/sh -c 'echo performance > /sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference'\"\nSUBSYSTEM==\"power_supply\", ATTR{online}==\"0\", RUN+=\"/usr/bin/sh -c 'echo balance_performance > /sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference'\"\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThpConfig {
    pub profile: String,
    pub scan_sleep_ms: i64,
}
impl Default for ThpConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            scan_sleep_ms: 10000,
        }
    }
}
pub fn load_thp(path: impl AsRef<Path>) -> ThpConfig {
    parse(path)
        .map(|v| ThpConfig {
            profile: profile(v.get("profile"), &["balanced", "kyth"], "balanced"),
            scan_sleep_ms: v
                .get("scan_sleep_ms")
                .and_then(toml::Value::as_integer)
                .unwrap_or(10000)
                .clamp(1000, 60000),
        })
        .unwrap_or_default()
}
pub fn save_thp(path: impl AsRef<Path>, config: &ThpConfig) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!(
            "# Kyth THP — offline\nprofile = {}\nscan_sleep_ms = {}\n",
            quote(&config.profile),
            config.scan_sleep_ms
        ),
        Some(0o600),
    )
}
pub fn thp_dropin(config: &ThpConfig) -> Option<String> {
    (config.profile == "kyth").then(|| format!("# Kyth THP — generated\nvm.compaction_proactiveness = 0\nkernel.khugepaged_scan_sleep_millisecs = {}\nkernel.khugepaged_alloc_sleep_millisecs = 60000\nkernel.khugepaged_max_ptes_none = 511\n", config.scan_sleep_ms))
}
pub fn thp_collapse_dropin(gaming: bool) -> Option<&'static str> {
    gaming.then_some("# Kyth THP collapse gaming — generated\nkernel.khugepaged_defrag=0\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelinuxConfig {
    pub permissive: Vec<String>,
    pub booleans: BTreeMap<String, bool>,
}
impl Default for SelinuxConfig {
    fn default() -> Self {
        Self {
            permissive: Vec::new(),
            booleans: BTreeMap::new(),
        }
    }
}
pub fn load_selinux(path: impl AsRef<Path>) -> SelinuxConfig {
    parse(path)
        .map(|v| SelinuxConfig {
            permissive: v
                .get("permissive")
                .and_then(toml::Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(toml::Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            booleans: v
                .get("booleans")
                .and_then(toml::Value::as_table)
                .map(|table| {
                    table
                        .iter()
                        .filter_map(|(name, value)| Some((name.clone(), value.as_bool()?)))
                        .collect()
                })
                .unwrap_or_default(),
        })
        .unwrap_or_default()
}
pub fn save_selinux(path: impl AsRef<Path>, config: &SelinuxConfig) -> std::io::Result<()> {
    let permissive = config
        .permissive
        .iter()
        .map(|value| quote(value))
        .collect::<Vec<_>>()
        .join(", ");
    let mut lines = vec![
        "# Kyth SELinux preset, offline".to_string(),
        format!("permissive = [{permissive}]"),
        "[booleans]".to_string(),
    ];
    for (name, enabled) in &config.booleans {
        lines.push(format!("{} = {}", quote(name), enabled));
    }
    crate::atomic_io::atomic_write_text(path, &format!("{}\n", lines.join("\n")), Some(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn fan_curve_interpolates_sorted_points() {
        assert_eq!(pwm_for_temp(55.0, &[(70, 80), (40, 30), (85, 100)]), 55);
        assert_eq!(pwm_for_temp(20.0, &[(40, 30)]), 30);
    }

    #[test]
    fn generated_profiles_are_reversible() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("pcie.rules");
        generate_pcie(
            &PcieConfig {
                profile: "gaming".into(),
            },
            &path,
        )
        .unwrap();
        assert!(path.exists());
        generate_pcie(&PcieConfig::default(), &path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn wine_and_shader_projections_are_safe() {
        assert_eq!(wine_sync_status(Some("WINEFSYNC=1\n")), "fsync");
        assert!(wine_env_for_mode("auto", true, false)
            .unwrap()
            .contains("NTSYNC"));
        assert_eq!(resolve_shader_size(&ShaderSizeConfig::default(), 12), "4G");
    }

    #[test]
    fn btrfs_and_thp_renderers_keep_mutation_outside_the_model() {
        let btrfs = BtrfsPerfConfig {
            profile: "kyth".into(),
            compress: "zstd".into(),
        };
        assert!(btrfs_perf_dropin(&btrfs)
            .unwrap()
            .contains("compress-force=zstd:1"));
        assert!(thp_dropin(&ThpConfig {
            profile: "kyth".into(),
            scan_sleep_ms: 2000
        })
        .unwrap()
        .contains("2000"));
        assert!(thp_collapse_dropin(true).is_some());
    }
}
