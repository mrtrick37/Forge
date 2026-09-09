//! Native dispatcher for migrated tunable profiles.
//!
//! The native binary owns the registry's sysctl and module-specific profiles.
//! The compatibility dispatcher remains available as a rollback fixture for
//! older images, but no registered tunable depends on it at runtime.

use kyth_shared::system::{
    ananicy, bore, btrfs_autotune, btrfs_perf, distrobox_cache,
    extended_preferences::{self, ThpConfig},
    flatpak_prefetch, flatpak_trim, gaming_kargs, gaming_master, gpu_power, hdr_per_game,
    hdr_store, net_latency, numa, overlay, perf_audit, perf_gate, podman_btrfs, preference_presets,
    readahead, scheduler_arbiter, shader_tmpfs, sysctl_profiles, system_audit, telemetry_opt,
    tunable_registry,
    tuning_profile::Profile,
    windows_verify, work_cache, zswap,
};
use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

fn invoked_name() -> String {
    env::args()
        .next()
        .and_then(|path| {
            Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
}

fn resolve_name(argv0: &str, args: &[String]) -> Result<(String, Vec<String>), String> {
    if argv0 == "kyth-tunable-rs" || argv0 == "kyth-tunable" {
        let Some(name) = args.first() else {
            return Err("Usage: kyth-tunable <tunable> [status|gaming|balanced|apply]".into());
        };
        return Ok((name.clone(), args[1..].to_vec()));
    }
    Ok((
        argv0.strip_prefix("kyth-").unwrap_or(argv0).to_string(),
        args.to_vec(),
    ))
}

fn test_mode() -> bool {
    env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1")
        && env::var_os("XDG_CONFIG_HOME").is_some()
}

fn native_sysctl(name: &str) -> bool {
    let config = format!("{name}.toml");
    sysctl_profiles::known_profiles().any(|(candidate, _)| candidate == config)
}

fn native_bespoke(name: &str) -> bool {
    matches!(name, "bore" | "net-tune" | "thp-tune" | "zswap")
}

fn native_other(name: &str) -> bool {
    matches!(
        name,
        "ananicy"
            | "btrfs-autotune"
            | "btrfs-tune"
            | "epp-ac"
            | "gaming-cfs"
            | "gaming-master"
            | "pcie"
            | "pipewire-gaming"
            | "psi-gaming"
            | "mimalloc"
            | "mimalloc-run"
            | "sccache"
            | "shader-cache-size"
            | "wine-sync"
            | "kwin-latency"
            | "distrobox-cache"
            | "flatpak-prefetch"
            | "flatpak-trim"
            | "readahead"
            | "trim-tune"
            | "uksmd"
            | "irq-tune"
            | "fscache"
            | "journal-tune"
            | "io-tune"
            | "podman-overlay"
            | "podman-btrfs"
            | "gpu-power"
            | "numa"
            | "selinux-gaming"
            | "shader-tmpfs"
            | "steam-deadzone"
            | "work-cache"
            | "telemetry-opt"
            | "perf-gate"
            | "gaming-audit"
            | "system-audit"
            | "fcitx-latency"
            | "boot-timeout"
            | "kargs-apply"
            | "sched-arbiter"
            | "oom-gaming"
            | "windows-verify"
            | "hdr-store"
            | "hdr-per-game"
    )
}

fn native_implemented(name: &str) -> bool {
    native_sysctl(name) || native_bespoke(name) || native_other(name)
}

fn native_tunable_names() -> Vec<String> {
    tunable_registry::list_tunables(None::<&Path>)
        .into_iter()
        .filter(|spec| native_implemented(&spec.name))
        .map(|spec| spec.name)
        .collect()
}

fn run_sysctl_system() {
    if test_mode() {
        return;
    }
    let argv = ["sysctl".to_string(), "--system".to_string()];
    let _ = kyth_shared::system::process::run_bounded(&argv, Duration::from_secs(15));
}

fn ensure_root(name: &str, args: &[String]) -> Result<(), ExitCode> {
    if test_mode() {
        return Ok(());
    }
    if unsafe { libc::geteuid() } == 0 {
        return Ok(());
    }
    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new("sudo");
    command.args(["-A", &format!("/usr/bin/kyth-{name}")]);
    command.args(args);
    let error = command.exec();
    eprintln!("kyth-{name}: cannot acquire root: {error}");
    Err(ExitCode::from(1))
}

fn generated_path(test_subdirectory: &str, filename: &str, production: &str) -> PathBuf {
    if test_mode() {
        if let Some(config) = env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config)
                .join("kyth")
                .join(test_subdirectory)
                .join(filename);
        }
    }
    PathBuf::from(production)
}

fn dispatch_zswap(action: &str) -> ExitCode {
    let config_path = zswap::config_path(None::<&Path>);
    let sysctl_path = generated_path(
        "sysctl.d",
        "99-kyth-zswap.conf",
        "/etc/sysctl.d/99-kyth-zswap.conf",
    );
    let modprobe_path = generated_path(
        "modprobe.d",
        "99-kyth-zswap.conf",
        "/etc/modprobe.d/99-kyth-zswap.conf",
    );
    match action {
        "status" => {
            let config = zswap::load(&config_path);
            let active = zswap::status(&sysctl_path);
            println!("profile={} active={} kind=sysctl", config.profile, active);
            ExitCode::SUCCESS
        }
        "gaming" | "balanced" => {
            if let Err(code) = ensure_root("zswap", &[action.to_string()]) {
                return code;
            }
            let mut config = zswap::load(&config_path);
            config.profile = if action == "gaming" {
                "kyth"
            } else {
                "balanced"
            }
            .into();
            if let Err(error) = zswap::save(&config_path, &config)
                .and_then(|_| zswap::generate(&config, &sysctl_path, &modprobe_path).map(|_| ()))
            {
                eprintln!("kyth-zswap: {error}");
                return ExitCode::from(1);
            }
            run_sysctl_system();
            println!("zswap {action}");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("zswap", &[action.to_string()]) {
                return code;
            }
            let config = zswap::load(&config_path);
            if let Err(error) = zswap::generate(&config, &sysctl_path, &modprobe_path) {
                eprintln!("kyth-zswap: {error}");
                return ExitCode::from(1);
            }
            run_sysctl_system();
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-zswap [status|gaming|balanced|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_bore(action: &str) -> ExitCode {
    let config_path = bore::config_path(None::<&Path>);
    let drop_in = generated_path(
        "sysctl.d",
        "99-kyth-bore.conf",
        "/etc/sysctl.d/99-kyth-bore.conf",
    );
    match action {
        "status" => {
            let config = bore::load(&config_path);
            println!(
                "profile={} active={} kind=sysctl",
                config.profile,
                bore::status(&drop_in)
            );
            ExitCode::SUCCESS
        }
        "gaming" | "balanced" => {
            if let Err(code) = ensure_root("bore", &[action.to_string()]) {
                return code;
            }
            let mut config = bore::load(&config_path);
            config.profile = if action == "gaming" {
                "gaming"
            } else {
                "balanced"
            }
            .into();
            let scx_active = !test_mode() && scheduler_arbiter::detect_scx_active();
            if let Err(error) = bore::save(&config_path, &config)
                .and_then(|_| bore::generate(&config, &drop_in, scx_active).map(|_| ()))
            {
                eprintln!("kyth-bore: {error}");
                return ExitCode::from(1);
            }
            run_sysctl_system();
            println!("bore {action}");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("bore", &[action.to_string()]) {
                return code;
            }
            let config = bore::load(&config_path);
            let scx_active = !test_mode() && scheduler_arbiter::detect_scx_active();
            if let Err(error) = bore::generate(&config, &drop_in, scx_active) {
                eprintln!("kyth-bore: {error}");
                return ExitCode::from(1);
            }
            run_sysctl_system();
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-bore [status|gaming|balanced|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_net_tune(action: &str) -> ExitCode {
    let config_path = net_latency::config_path(None::<&Path>);
    let drop_in = generated_path(
        "sysctl.d",
        "99-kyth-net-latency.conf",
        "/etc/sysctl.d/99-kyth-net-latency.conf",
    );
    match action {
        "status" => {
            let config = net_latency::load(&config_path);
            println!(
                "profile={} active={} kind=sysctl",
                if config.enabled { "gaming" } else { "balanced" },
                net_latency::status(&drop_in)
            );
            ExitCode::SUCCESS
        }
        "gaming" | "balanced" => {
            if let Err(code) = ensure_root("net-tune", &[action.to_string()]) {
                return code;
            }
            let mut config = net_latency::load(&config_path);
            config.enabled = action == "gaming";
            if let Err(error) = net_latency::save(&config_path, &config)
                .and_then(|_| net_latency::generate(&config, &drop_in).map(|_| ()))
            {
                eprintln!("kyth-net-tune: {error}");
                return ExitCode::from(1);
            }
            run_sysctl_system();
            println!("net-tune {action}");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("net-tune", &[action.to_string()]) {
                return code;
            }
            let config = net_latency::load(&config_path);
            if let Err(error) = net_latency::generate(&config, &drop_in) {
                eprintln!("kyth-net-tune: {error}");
                return ExitCode::from(1);
            }
            run_sysctl_system();
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-net-tune [status|gaming|balanced|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_ananicy(action: &str) -> ExitCode {
    let config_path = ananicy::config_path(None::<&Path>);
    let rule = generated_path(
        "ananicy.d",
        "99-kyth-gaming.conf",
        "/etc/ananicy.d/99-kyth-gaming.conf",
    );
    match action {
        "status" => {
            let config = ananicy::load(&config_path);
            println!(
                "profile={} active={} kind=other",
                config.profile,
                ananicy::status(&rule)
            );
            ExitCode::SUCCESS
        }
        "gaming" | "balanced" => {
            if let Err(code) = ensure_root("ananicy", &[action.to_string()]) {
                return code;
            }
            let mut config = ananicy::load(&config_path);
            config.profile = if action == "gaming" {
                "kyth"
            } else {
                "balanced"
            }
            .into();
            if let Err(error) = ananicy::save(&config_path, &config)
                .and_then(|_| ananicy::generate(&config, &rule).map(|_| ()))
            {
                eprintln!("kyth-ananicy: {error}");
                return ExitCode::from(1);
            }
            println!("ananicy {action}");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("ananicy", &[action.to_string()]) {
                return code;
            }
            let config = ananicy::load(&config_path);
            if let Err(error) = ananicy::generate(&config, &rule) {
                eprintln!("kyth-ananicy: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-ananicy [status|gaming|balanced|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_btrfs_autotune(action: &str) -> ExitCode {
    let config_path = btrfs_autotune::config_path(None::<&Path>);
    let script = generated_path(
        "libexec",
        "kyth-btrfs-autotune",
        "/usr/libexec/kyth-btrfs-autotune",
    );
    match action {
        "status" => {
            let config = btrfs_autotune::load(&config_path);
            println!(
                "enabled={} active={} kind=other",
                config.enabled,
                btrfs_autotune::status(&script)
            );
            ExitCode::SUCCESS
        }
        "gaming" | "balanced" => {
            if let Err(code) = ensure_root("btrfs-autotune", &[action.to_string()]) {
                return code;
            }
            let mut config = btrfs_autotune::load(&config_path);
            config.enabled = action == "gaming";
            if let Err(error) = btrfs_autotune::save(&config_path, config)
                .and_then(|_| btrfs_autotune::generate(config, &script).map(|_| ()))
            {
                eprintln!("kyth-btrfs-autotune: {error}");
                return ExitCode::from(1);
            }
            println!("btrfs-autotune {action}");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("btrfs-autotune", &[action.to_string()]) {
                return code;
            }
            let config = btrfs_autotune::load(&config_path);
            if let Err(error) = btrfs_autotune::generate(config, &script) {
                eprintln!("kyth-btrfs-autotune: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-btrfs-autotune [status|gaming|balanced|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_btrfs_tune(action: &str) -> ExitCode {
    let config_path = btrfs_perf::config_path(None::<&Path>);
    let root_dropin = generated_path(
        "systemd/root.mount.d",
        "99-kyth-btrfs.conf",
        btrfs_perf::DEFAULT_DROP_IN,
    );
    let generate_destination = test_mode().then_some(root_dropin.as_path());
    match action {
        "status" => {
            let config = btrfs_perf::load(&config_path);
            println!(
                "profile={} active={} kind=other",
                config.profile,
                btrfs_perf::status(&root_dropin)
            );
            ExitCode::SUCCESS
        }
        "gaming" | "balanced" => {
            if let Err(code) = ensure_root("btrfs-tune", &[action.to_string()]) {
                return code;
            }
            let mut config = btrfs_perf::load(&config_path);
            config.profile = if action == "gaming" {
                "kyth"
            } else {
                "balanced"
            }
            .into();
            if let Err(error) = btrfs_perf::save(&config_path, &config)
                .and_then(|_| btrfs_perf::generate(&config, generate_destination).map(|_| ()))
            {
                eprintln!("kyth-btrfs-tune: {error}");
                return ExitCode::from(1);
            }
            println!("btrfs-tune {action}");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("btrfs-tune", &[action.to_string()]) {
                return code;
            }
            let config = btrfs_perf::load(&config_path);
            if let Err(error) = btrfs_perf::generate(&config, generate_destination) {
                eprintln!("kyth-btrfs-tune: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-btrfs-tune [status|gaming|balanced|apply]");
            ExitCode::from(1)
        }
    }
}

fn module_config_path(filename: &str, production: &str) -> PathBuf {
    if test_mode() {
        if let Some(config) = env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth").join(filename);
        }
    }
    PathBuf::from(production)
}

fn write_optional(path: &Path, content: Option<&str>) -> std::io::Result<()> {
    if let Some(content) = content {
        kyth_shared::atomic_io::atomic_write_text(path, content, Some(0o644))?;
    } else {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn dispatch_distrobox_cache(action: &str) -> ExitCode {
    let config_path = distrobox_cache::config_path(None::<&Path>);
    let tmpfiles = generated_path(
        "tmpfiles.d",
        "99-kyth-distrobox.conf",
        "/etc/tmpfiles.d/99-kyth-distrobox.conf",
    );
    let service = generated_path(
        "systemd",
        "kyth-distrobox-cache.service",
        "/etc/systemd/system/kyth-distrobox-cache.service",
    );
    match action {
        "status" => {
            let config = distrobox_cache::load(&config_path);
            println!(
                "enabled={} size={} ccache_size={} active={} kind=other",
                config.enabled,
                config.size,
                config.ccache_size,
                distrobox_cache::status(&service)
            );
            ExitCode::SUCCESS
        }
        "on" | "gaming" => {
            if let Err(code) = ensure_root("distrobox-cache", &[action.to_string()]) {
                return code;
            }
            let mut config = distrobox_cache::load(&config_path);
            config.enabled = true;
            if let Err(error) = distrobox_cache::save(&config_path, &config)
                .and_then(|_| distrobox_cache::generate(&config, &tmpfiles, &service).map(|_| ()))
            {
                eprintln!("kyth-distrobox-cache: {error}");
                return ExitCode::from(1);
            }
            println!("distrobox cache on");
            ExitCode::SUCCESS
        }
        "off" | "balanced" => {
            if let Err(code) = ensure_root("distrobox-cache", &[action.to_string()]) {
                return code;
            }
            let mut config = distrobox_cache::load(&config_path);
            config.enabled = false;
            if let Err(error) = distrobox_cache::save(&config_path, &config)
                .and_then(|_| distrobox_cache::generate(&config, &tmpfiles, &service).map(|_| ()))
            {
                eprintln!("kyth-distrobox-cache: {error}");
                return ExitCode::from(1);
            }
            println!("distrobox cache off");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("distrobox-cache", &[action.to_string()]) {
                return code;
            }
            let config = distrobox_cache::load(&config_path);
            if let Err(error) = distrobox_cache::generate(&config, &tmpfiles, &service) {
                eprintln!("kyth-distrobox-cache: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-distrobox-cache [status|on|off|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_flatpak_prefetch(action: &str) -> ExitCode {
    let config_path = flatpak_prefetch::config_path(None::<&Path>);
    let service = generated_path(
        "systemd",
        "flatpak-prefetch.service",
        "/etc/systemd/system/flatpak-prefetch.service",
    );
    let timer = generated_path(
        "systemd",
        "flatpak-prefetch.timer",
        "/etc/systemd/system/flatpak-prefetch.timer",
    );
    match action {
        "status" => {
            let config = flatpak_prefetch::load(&config_path);
            println!(
                "enabled={} time={} active={} kind=other",
                config.enabled,
                config.time,
                flatpak_prefetch::status(&service)
            );
            ExitCode::SUCCESS
        }
        "on" | "gaming" => {
            if let Err(code) = ensure_root("flatpak-prefetch", &[action.to_string()]) {
                return code;
            }
            let mut config = flatpak_prefetch::load(&config_path);
            config.enabled = true;
            if let Err(error) = flatpak_prefetch::save(&config_path, &config)
                .and_then(|_| flatpak_prefetch::generate(&config, &service, &timer).map(|_| ()))
            {
                eprintln!("kyth-flatpak-prefetch: {error}");
                return ExitCode::from(1);
            }
            println!("flatpak prefetch on");
            ExitCode::SUCCESS
        }
        "off" | "balanced" => {
            if let Err(code) = ensure_root("flatpak-prefetch", &[action.to_string()]) {
                return code;
            }
            let mut config = flatpak_prefetch::load(&config_path);
            config.enabled = false;
            if let Err(error) = flatpak_prefetch::save(&config_path, &config)
                .and_then(|_| flatpak_prefetch::generate(&config, &service, &timer).map(|_| ()))
            {
                eprintln!("kyth-flatpak-prefetch: {error}");
                return ExitCode::from(1);
            }
            println!("flatpak prefetch off");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("flatpak-prefetch", &[action.to_string()]) {
                return code;
            }
            let config = flatpak_prefetch::load(&config_path);
            if let Err(error) = flatpak_prefetch::generate(&config, &service, &timer) {
                eprintln!("kyth-flatpak-prefetch: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-flatpak-prefetch [status|on|off|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_flatpak_trim(action: &str) -> ExitCode {
    let config_path = flatpak_trim::config_path(None::<&Path>);
    let service = generated_path(
        "systemd",
        "kyth-flatpak-trim.service",
        "/etc/systemd/system/kyth-flatpak-trim.service",
    );
    let timer = generated_path(
        "systemd",
        "kyth-flatpak-trim.timer",
        "/etc/systemd/system/kyth-flatpak-trim.timer",
    );
    match action {
        "status" => {
            let config = flatpak_trim::load(&config_path);
            println!(
                "enabled={} active={} kind=other",
                config.enabled,
                flatpak_trim::status(&service)
            );
            ExitCode::SUCCESS
        }
        "on" | "gaming" => {
            if let Err(code) = ensure_root("flatpak-trim", &[action.to_string()]) {
                return code;
            }
            let config = flatpak_trim::FlatpakTrimConfig { enabled: true };
            if let Err(error) = flatpak_trim::save(&config_path, config)
                .and_then(|_| flatpak_trim::generate(config, &service, &timer).map(|_| ()))
            {
                eprintln!("kyth-flatpak-trim: {error}");
                return ExitCode::from(1);
            }
            println!("flatpak trim on");
            ExitCode::SUCCESS
        }
        "off" | "balanced" => {
            if let Err(code) = ensure_root("flatpak-trim", &[action.to_string()]) {
                return code;
            }
            let config = flatpak_trim::FlatpakTrimConfig { enabled: false };
            if let Err(error) = flatpak_trim::save(&config_path, config)
                .and_then(|_| flatpak_trim::generate(config, &service, &timer).map(|_| ()))
            {
                eprintln!("kyth-flatpak-trim: {error}");
                return ExitCode::from(1);
            }
            println!("flatpak trim off");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("flatpak-trim", &[action.to_string()]) {
                return code;
            }
            let config = flatpak_trim::load(&config_path);
            if let Err(error) = flatpak_trim::generate(config, &service, &timer) {
                eprintln!("kyth-flatpak-trim: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-flatpak-trim [status|on|off|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_readahead(action: &str) -> ExitCode {
    let config_path = readahead::config_path(None::<&Path>);
    match action {
        "status" => {
            let config = readahead::load(&config_path);
            let active = if config.enabled { "enabled" } else { "off" };
            println!(
                "enabled={} size_mb={} active={} kind=other",
                config.enabled, config.size_mb, active
            );
            ExitCode::SUCCESS
        }
        "gaming" => {
            if let Err(code) = ensure_root("readahead", &[action.to_string()]) {
                return code;
            }
            let mut config = readahead::load(&config_path);
            config.enabled = true;
            if let Err(error) = readahead::save(&config_path, &config) {
                eprintln!("kyth-readahead: {error}");
                return ExitCode::from(1);
            }
            println!("readahead gaming");
            ExitCode::SUCCESS
        }
        "balanced" => {
            if let Err(code) = ensure_root("readahead", &[action.to_string()]) {
                return code;
            }
            let mut config = readahead::load(&config_path);
            config.enabled = false;
            if let Err(error) = readahead::save(&config_path, &config) {
                eprintln!("kyth-readahead: {error}");
                return ExitCode::from(1);
            }
            println!("readahead balanced");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("readahead", &[action.to_string()]) {
                return code;
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-readahead [status|gaming|balanced|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_trim_tune(action: &str) -> ExitCode {
    let config_path = kyth_shared::system::runtime_preferences::trim_path(None::<&Path>);
    let marker = generated_path("run", "kyth-trim-profile", "/run/kyth-trim-profile");
    match action {
        "status" => {
            let config = kyth_shared::system::runtime_preferences::load_trim(&config_path);
            println!(
                "profile={} weekly={} active={} kind=other",
                config.profile,
                config.weekly,
                kyth_shared::system::runtime_preferences::trim_status(&marker)
            );
            ExitCode::SUCCESS
        }
        "gaming" => {
            if let Err(code) = ensure_root("trim-tune", &[action.to_string()]) {
                return code;
            }
            let mut config = kyth_shared::system::runtime_preferences::load_trim(&config_path);
            config.profile = "kyth".into();
            if let Err(error) = kyth_shared::system::runtime_preferences::save_trim(
                &config_path,
                &config,
            )
            .and_then(|_| {
                kyth_shared::system::runtime_preferences::generate_trim_marker(&config, &marker)
                    .map(|_| ())
            }) {
                eprintln!("kyth-trim-tune: {error}");
                return ExitCode::from(1);
            }
            println!("trim-tune gaming");
            ExitCode::SUCCESS
        }
        "balanced" => {
            if let Err(code) = ensure_root("trim-tune", &[action.to_string()]) {
                return code;
            }
            let mut config = kyth_shared::system::runtime_preferences::load_trim(&config_path);
            config.profile = "balanced".into();
            if let Err(error) = kyth_shared::system::runtime_preferences::save_trim(
                &config_path,
                &config,
            )
            .and_then(|_| {
                kyth_shared::system::runtime_preferences::generate_trim_marker(&config, &marker)
                    .map(|_| ())
            }) {
                eprintln!("kyth-trim-tune: {error}");
                return ExitCode::from(1);
            }
            println!("trim-tune balanced");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("trim-tune", &[action.to_string()]) {
                return code;
            }
            let config = kyth_shared::system::runtime_preferences::load_trim(&config_path);
            if let Err(error) =
                kyth_shared::system::runtime_preferences::generate_trim_marker(&config, &marker)
            {
                eprintln!("kyth-trim-tune: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-trim-tune [status|gaming|balanced|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_uksmd(action: &str) -> ExitCode {
    let config_path = kyth_shared::system::runtime_preferences::uksmd_path(None::<&Path>);
    let destination = generated_path("etc", "uksmd.conf", "/etc/uksmd.conf");
    match action {
        "status" => {
            let config = kyth_shared::system::runtime_preferences::load_uksmd(&config_path);
            println!(
                "enabled={} max_cpu_percent={} suggested={} active={} kind=other",
                config.enabled,
                config.max_cpu_percent,
                kyth_shared::system::runtime_preferences::uksmd_suggested("/proc/meminfo"),
                if destination.is_file() {
                    "enabled"
                } else {
                    "off"
                }
            );
            ExitCode::SUCCESS
        }
        "gaming" => {
            if let Err(code) = ensure_root("uksmd", &[action.to_string()]) {
                return code;
            }
            let mut config = kyth_shared::system::runtime_preferences::load_uksmd(&config_path);
            config.enabled = true;
            if let Err(error) =
                kyth_shared::system::runtime_preferences::save_uksmd(&config_path, &config)
                    .and_then(|_| {
                        kyth_shared::system::runtime_preferences::generate_uksmd(
                            &config,
                            &destination,
                        )
                        .map(|_| ())
                    })
            {
                eprintln!("kyth-uksmd: {error}");
                return ExitCode::from(1);
            }
            println!("uksmd gaming");
            ExitCode::SUCCESS
        }
        "balanced" => {
            if let Err(code) = ensure_root("uksmd", &[action.to_string()]) {
                return code;
            }
            let mut config = kyth_shared::system::runtime_preferences::load_uksmd(&config_path);
            config.enabled = false;
            if let Err(error) =
                kyth_shared::system::runtime_preferences::save_uksmd(&config_path, &config)
                    .and_then(|_| {
                        kyth_shared::system::runtime_preferences::generate_uksmd(
                            &config,
                            &destination,
                        )
                        .map(|_| ())
                    })
            {
                eprintln!("kyth-uksmd: {error}");
                return ExitCode::from(1);
            }
            println!("uksmd balanced");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("uksmd", &[action.to_string()]) {
                return code;
            }
            let config = kyth_shared::system::runtime_preferences::load_uksmd(&config_path);
            if let Err(error) =
                kyth_shared::system::runtime_preferences::generate_uksmd(&config, &destination)
            {
                eprintln!("kyth-uksmd: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-uksmd [status|gaming|balanced|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_irq_tune(action: &str) -> ExitCode {
    let config_path = kyth_shared::system::runtime_preferences::irq_path(None::<&Path>);
    let dropin = generated_path(
        "systemd/irqbalance.service.d",
        "99-kyth-irq.conf",
        "/etc/systemd/system/irqbalance.service.d/99-kyth-irq.conf",
    );
    match action {
        "status" => {
            let config = kyth_shared::system::runtime_preferences::load_irq(&config_path);
            println!(
                "profile={} isolated_cpus={} active={} kind=other",
                config.profile,
                config.isolated_cpus,
                kyth_shared::system::runtime_preferences::irq_status(&dropin)
            );
            ExitCode::SUCCESS
        }
        "gaming" => {
            if let Err(code) = ensure_root("irq-tune", &[action.to_string()]) {
                return code;
            }
            let mut config = kyth_shared::system::runtime_preferences::load_irq(&config_path);
            config.profile = "kyth".into();
            if let Err(error) =
                kyth_shared::system::runtime_preferences::save_irq(&config_path, &config).and_then(
                    |_| {
                        kyth_shared::system::runtime_preferences::generate_irq(&config, &dropin, "")
                            .map(|_| ())
                    },
                )
            {
                eprintln!("kyth-irq-tune: {error}");
                return ExitCode::from(1);
            }
            println!("irq-tune gaming");
            ExitCode::SUCCESS
        }
        "balanced" => {
            if let Err(code) = ensure_root("irq-tune", &[action.to_string()]) {
                return code;
            }
            let mut config = kyth_shared::system::runtime_preferences::load_irq(&config_path);
            config.profile = "balanced".into();
            if let Err(error) =
                kyth_shared::system::runtime_preferences::save_irq(&config_path, &config).and_then(
                    |_| {
                        kyth_shared::system::runtime_preferences::generate_irq(&config, &dropin, "")
                            .map(|_| ())
                    },
                )
            {
                eprintln!("kyth-irq-tune: {error}");
                return ExitCode::from(1);
            }
            println!("irq-tune balanced");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("irq-tune", &[action.to_string()]) {
                return code;
            }
            let config = kyth_shared::system::runtime_preferences::load_irq(&config_path);
            if let Err(error) =
                kyth_shared::system::runtime_preferences::generate_irq(&config, &dropin, "")
            {
                eprintln!("kyth-irq-tune: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-irq-tune [status|gaming|balanced|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_fscache(action: &str) -> ExitCode {
    let config_path = kyth_shared::system::runtime_preferences::fscache_path(None::<&Path>);
    let destination = generated_path(
        "cachefilesd.conf.d",
        "99-kyth-fscache.conf",
        "/etc/cachefilesd.conf.d/99-kyth-fscache.conf",
    );
    match action {
        "status" => {
            let config = kyth_shared::system::runtime_preferences::load_fscache(&config_path);
            println!(
                "enabled={} active={} kind=other",
                config.enabled,
                if destination.is_file() {
                    "enabled"
                } else {
                    "off"
                }
            );
            ExitCode::SUCCESS
        }
        "gaming" => {
            if let Err(code) = ensure_root("fscache", &[action.to_string()]) {
                return code;
            }
            let config = kyth_shared::system::runtime_preferences::FscacheConfig { enabled: true };
            if let Err(error) =
                kyth_shared::system::runtime_preferences::save_fscache(&config_path, &config)
                    .and_then(|_| {
                        kyth_shared::system::runtime_preferences::generate_fscache(
                            &config,
                            &destination,
                        )
                        .map(|_| ())
                    })
            {
                eprintln!("kyth-fscache: {error}");
                return ExitCode::from(1);
            }
            println!("fscache gaming");
            ExitCode::SUCCESS
        }
        "balanced" => {
            if let Err(code) = ensure_root("fscache", &[action.to_string()]) {
                return code;
            }
            let config = kyth_shared::system::runtime_preferences::FscacheConfig { enabled: false };
            if let Err(error) =
                kyth_shared::system::runtime_preferences::save_fscache(&config_path, &config)
                    .and_then(|_| {
                        kyth_shared::system::runtime_preferences::generate_fscache(
                            &config,
                            &destination,
                        )
                        .map(|_| ())
                    })
            {
                eprintln!("kyth-fscache: {error}");
                return ExitCode::from(1);
            }
            println!("fscache balanced");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("fscache", &[action.to_string()]) {
                return code;
            }
            let config = kyth_shared::system::runtime_preferences::load_fscache(&config_path);
            if let Err(error) =
                kyth_shared::system::runtime_preferences::generate_fscache(&config, &destination)
            {
                eprintln!("kyth-fscache: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-fscache [gaming|balanced|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_journal_tune(action: &str) -> ExitCode {
    let config_path = kyth_shared::system::runtime_preferences::journal_path(None::<&Path>);
    let destination = generated_path(
        "systemd/journald.conf.d",
        "99-kyth-perf.conf",
        "/etc/systemd/journald.conf.d/99-kyth-perf.conf",
    );
    match action {
        "status" => {
            let config = kyth_shared::system::runtime_preferences::load_journal(&config_path);
            println!(
                "perf={} system_max_use={} runtime_max_use={} active={} kind=other",
                config.perf,
                config.system_max_use,
                config.runtime_max_use,
                if destination.is_file() {
                    "kyth"
                } else {
                    "balanced"
                }
            );
            ExitCode::SUCCESS
        }
        "gaming" => {
            if let Err(code) = ensure_root("journal-tune", &[action.to_string()]) {
                return code;
            }
            let mut config = kyth_shared::system::runtime_preferences::load_journal(&config_path);
            config.perf = true;
            if let Err(error) =
                kyth_shared::system::runtime_preferences::save_journal(&config_path, &config)
                    .and_then(|_| {
                        kyth_shared::system::runtime_preferences::generate_journal(
                            &config,
                            &destination,
                        )
                        .map(|_| ())
                    })
            {
                eprintln!("kyth-journal-tune: {error}");
                return ExitCode::from(1);
            }
            println!("journal-tune gaming");
            ExitCode::SUCCESS
        }
        "balanced" => {
            if let Err(code) = ensure_root("journal-tune", &[action.to_string()]) {
                return code;
            }
            let mut config = kyth_shared::system::runtime_preferences::load_journal(&config_path);
            config.perf = false;
            if let Err(error) =
                kyth_shared::system::runtime_preferences::save_journal(&config_path, &config)
                    .and_then(|_| {
                        kyth_shared::system::runtime_preferences::generate_journal(
                            &config,
                            &destination,
                        )
                        .map(|_| ())
                    })
            {
                eprintln!("kyth-journal-tune: {error}");
                return ExitCode::from(1);
            }
            println!("journal-tune balanced");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("journal-tune", &[action.to_string()]) {
                return code;
            }
            let config = kyth_shared::system::runtime_preferences::load_journal(&config_path);
            if let Err(error) =
                kyth_shared::system::runtime_preferences::generate_journal(&config, &destination)
            {
                eprintln!("kyth-journal-tune: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-journal-tune [gaming|balanced|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_io_tune(action: &str) -> ExitCode {
    let config_path = kyth_shared::system::io_tune::config_path(None::<&Path>);
    let destination = generated_path(
        "udev/rules.d",
        "61-kyth-io-tune.rules",
        "/etc/udev/rules.d/61-kyth-io-tune.rules",
    );
    match action {
        "status" => {
            let config = kyth_shared::system::io_tune::load(&config_path);
            println!(
                "profile={} read_ahead_kb={} active={} kind=other",
                config.profile,
                config.read_ahead_kb,
                kyth_shared::system::io_tune::status(&destination)
            );
            ExitCode::SUCCESS
        }
        "gaming" => {
            if let Err(code) = ensure_root("io-tune", &[action.to_string()]) {
                return code;
            }
            let mut config = kyth_shared::system::io_tune::load(&config_path);
            config.profile = "kyth".into();
            if let Err(error) =
                kyth_shared::system::io_tune::save(&config_path, &config).and_then(|_| {
                    kyth_shared::system::io_tune::generate(&config, &destination).map(|_| ())
                })
            {
                eprintln!("kyth-io-tune: {error}");
                return ExitCode::from(1);
            }
            println!("io-tune gaming");
            ExitCode::SUCCESS
        }
        "balanced" => {
            if let Err(code) = ensure_root("io-tune", &[action.to_string()]) {
                return code;
            }
            let mut config = kyth_shared::system::io_tune::load(&config_path);
            config.profile = "balanced".into();
            if let Err(error) =
                kyth_shared::system::io_tune::save(&config_path, &config).and_then(|_| {
                    kyth_shared::system::io_tune::generate(&config, &destination).map(|_| ())
                })
            {
                eprintln!("kyth-io-tune: {error}");
                return ExitCode::from(1);
            }
            println!("io-tune balanced");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("io-tune", &[action.to_string()]) {
                return code;
            }
            let config = kyth_shared::system::io_tune::load(&config_path);
            if let Err(error) = kyth_shared::system::io_tune::generate(&config, &destination) {
                eprintln!("kyth-io-tune: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-io-tune [gaming|balanced|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn podman_on_btrfs() -> bool {
    std::fs::read_to_string("/proc/mounts")
        .ok()
        .is_some_and(|mounts| {
            mounts
                .lines()
                .any(|line| line.split_whitespace().nth(2) == Some("btrfs"))
        })
}

fn dispatch_podman_overlay(action: &str) -> ExitCode {
    let config_path = overlay::config_path(None::<&Path>);
    let destination = generated_path(
        "containers/storage.conf.d",
        "99-kyth-overlay.conf",
        "/etc/containers/storage.conf.d/99-kyth-overlay.conf",
    );
    let on_btrfs = podman_on_btrfs();
    match action {
        "status" => {
            let config = overlay::load(&config_path);
            println!(
                "metacopy={} resolved={} active={} kind=other",
                config.as_str(),
                overlay::resolve(config, on_btrfs).as_str(),
                if destination.is_file() { "on" } else { "off" }
            );
            ExitCode::SUCCESS
        }
        "on" | "gaming" => {
            if let Err(code) = ensure_root("podman-overlay", &[action.to_string()]) {
                return code;
            }
            if let Err(error) = overlay::save(&config_path, overlay::Metacopy::On).and_then(|_| {
                overlay::generate(overlay::Metacopy::On, on_btrfs, &destination).map(|_| ())
            }) {
                eprintln!("kyth-podman-overlay: {error}");
                return ExitCode::from(1);
            }
            println!("podman-overlay on");
            ExitCode::SUCCESS
        }
        "off" => {
            if let Err(code) = ensure_root("podman-overlay", &[action.to_string()]) {
                return code;
            }
            if let Err(error) = overlay::save(&config_path, overlay::Metacopy::Off).and_then(|_| {
                overlay::generate(overlay::Metacopy::Off, on_btrfs, &destination).map(|_| ())
            }) {
                eprintln!("kyth-podman-overlay: {error}");
                return ExitCode::from(1);
            }
            println!("podman-overlay off");
            ExitCode::SUCCESS
        }
        "auto" | "balanced" => {
            if let Err(code) = ensure_root("podman-overlay", &[action.to_string()]) {
                return code;
            }
            if let Err(error) =
                overlay::save(&config_path, overlay::Metacopy::Auto).and_then(|_| {
                    overlay::generate(overlay::Metacopy::Auto, on_btrfs, &destination).map(|_| ())
                })
            {
                eprintln!("kyth-podman-overlay: {error}");
                return ExitCode::from(1);
            }
            println!("podman-overlay auto");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("podman-overlay", &[action.to_string()]) {
                return code;
            }
            let config = overlay::load(&config_path);
            if let Err(error) = overlay::generate(config, on_btrfs, &destination) {
                eprintln!("kyth-podman-overlay: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-podman-overlay [on|off|auto|gaming|balanced|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_podman_btrfs(action: &str) -> ExitCode {
    let config_path = podman_btrfs::config_path(None::<&Path>);
    let destination = generated_path(
        "containers/storage.conf.d",
        "99-kyth-btrfs.conf",
        "/etc/containers/storage.conf.d/99-kyth-btrfs.conf",
    );
    let on_btrfs = podman_on_btrfs();
    match action {
        "status" => {
            let mode = podman_btrfs::load(&config_path);
            println!(
                "mode={} resolved={} active={} kind=other",
                mode.as_str(),
                podman_btrfs::resolve(mode, on_btrfs).as_str(),
                podman_btrfs::status(&destination, on_btrfs)
            );
            ExitCode::SUCCESS
        }
        "btrfs" | "gaming" => {
            if let Err(code) = ensure_root("podman-btrfs", &[action.to_string()]) {
                return code;
            }
            if let Err(error) = podman_btrfs::save(&config_path, podman_btrfs::PodmanMode::Btrfs)
                .and_then(|_| {
                    podman_btrfs::generate(podman_btrfs::PodmanMode::Btrfs, on_btrfs, &destination)
                        .map(|_| ())
                })
            {
                eprintln!("kyth-podman-btrfs: {error}");
                return ExitCode::from(1);
            }
            println!("podman-btrfs btrfs");
            ExitCode::SUCCESS
        }
        "overlay" | "off" => {
            if let Err(code) = ensure_root("podman-btrfs", &[action.to_string()]) {
                return code;
            }
            let mode = if action == "off" {
                podman_btrfs::PodmanMode::Off
            } else {
                podman_btrfs::PodmanMode::Overlay
            };
            if let Err(error) = podman_btrfs::save(&config_path, mode)
                .and_then(|_| podman_btrfs::generate(mode, on_btrfs, &destination).map(|_| ()))
            {
                eprintln!("kyth-podman-btrfs: {error}");
                return ExitCode::from(1);
            }
            println!("podman-btrfs {}", mode.as_str());
            ExitCode::SUCCESS
        }
        "auto" | "balanced" => {
            if let Err(code) = ensure_root("podman-btrfs", &[action.to_string()]) {
                return code;
            }
            if let Err(error) = podman_btrfs::save(&config_path, podman_btrfs::PodmanMode::Auto)
                .and_then(|_| {
                    podman_btrfs::generate(podman_btrfs::PodmanMode::Auto, on_btrfs, &destination)
                        .map(|_| ())
                })
            {
                eprintln!("kyth-podman-btrfs: {error}");
                return ExitCode::from(1);
            }
            println!("podman-btrfs auto");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("podman-btrfs", &[action.to_string()]) {
                return code;
            }
            let mode = podman_btrfs::load(&config_path);
            if let Err(error) = podman_btrfs::generate(mode, on_btrfs, &destination) {
                eprintln!("kyth-podman-btrfs: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "Usage: kyth-podman-btrfs [btrfs|overlay|off|auto|gaming|balanced|apply|status]"
            );
            ExitCode::from(1)
        }
    }
}

fn dispatch_gpu_power(action: &str) -> ExitCode {
    let config_path = gpu_power::config_path(None::<&Path>);
    match action {
        "status" => {
            let config = gpu_power::load(&config_path);
            println!(
                "profile={} dpm={} active={} kind=other",
                config.profile,
                config.dpm,
                gpu_power::status(&config_path)
            );
            ExitCode::SUCCESS
        }
        "gaming" | "high" => {
            if let Err(code) = ensure_root("gpu-power", &[action.to_string()]) {
                return code;
            }
            let config = gpu_power::GpuPowerConfig {
                profile: "kyth".into(),
                dpm: "high".into(),
            };
            if let Err(error) = gpu_power::save(&config_path, &config) {
                eprintln!("kyth-gpu-power: {error}");
                return ExitCode::from(1);
            }
            println!("gpu-power gaming");
            ExitCode::SUCCESS
        }
        "balanced" | "auto" | "low" => {
            if let Err(code) = ensure_root("gpu-power", &[action.to_string()]) {
                return code;
            }
            let dpm = if action == "low" { "low" } else { "auto" };
            let config = gpu_power::GpuPowerConfig {
                profile: "balanced".into(),
                dpm: dpm.into(),
            };
            if let Err(error) = gpu_power::save(&config_path, &config) {
                eprintln!("kyth-gpu-power: {error}");
                return ExitCode::from(1);
            }
            println!("gpu-power balanced");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("gpu-power", &[action.to_string()]) {
                return code;
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-gpu-power [gaming|balanced|auto|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_numa(action: &str) -> ExitCode {
    let config_path = numa::config_path(None::<&Path>);
    match action {
        "status" => {
            let config = numa::load(&config_path);
            println!(
                "profile={} cpus={} effective_cpus={} kind=other",
                config.profile,
                config.cpus,
                numa::effective_cpus(&config, None)
            );
            ExitCode::SUCCESS
        }
        "gaming" => {
            if let Err(code) = ensure_root("numa", &[action.to_string()]) {
                return code;
            }
            let mut config = numa::load(&config_path);
            config.profile = "gaming".into();
            if let Err(error) = numa::save(&config_path, &config) {
                eprintln!("kyth-numa: {error}");
                return ExitCode::from(1);
            }
            println!("numa gaming");
            ExitCode::SUCCESS
        }
        "balanced" | "off" => {
            if let Err(code) = ensure_root("numa", &[action.to_string()]) {
                return code;
            }
            let mut config = numa::load(&config_path);
            config.profile = "balanced".into();
            if let Err(error) = numa::save(&config_path, &config) {
                eprintln!("kyth-numa: {error}");
                return ExitCode::from(1);
            }
            println!("numa balanced");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("numa", &[action.to_string()]) {
                return code;
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-numa [gaming|balanced|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_selinux_gaming(action: &str) -> ExitCode {
    let config_path = preference_presets::selinux_gaming_path(None::<&Path>);
    match action {
        "status" => {
            let config = preference_presets::load_selinux_gaming(&config_path);
            println!(
                "profile={} allow_execheap={} kind=other",
                config.profile, config.allow_execheap
            );
            ExitCode::SUCCESS
        }
        "gaming" => {
            if let Err(code) = ensure_root("selinux-gaming", &[action.to_string()]) {
                return code;
            }
            let config = preference_presets::SelinuxGamingConfig {
                profile: "gaming".into(),
                allow_execheap: true,
            };
            if let Err(error) = preference_presets::save_selinux_gaming(&config_path, &config) {
                eprintln!("kyth-selinux-gaming: {error}");
                return ExitCode::from(1);
            }
            println!("selinux-gaming gaming");
            ExitCode::SUCCESS
        }
        "balanced" | "off" => {
            if let Err(code) = ensure_root("selinux-gaming", &[action.to_string()]) {
                return code;
            }
            let config = preference_presets::SelinuxGamingConfig::default();
            if let Err(error) = preference_presets::save_selinux_gaming(&config_path, &config) {
                eprintln!("kyth-selinux-gaming: {error}");
                return ExitCode::from(1);
            }
            println!("selinux-gaming balanced");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("selinux-gaming", &[action.to_string()]) {
                return code;
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-selinux-gaming [gaming|balanced|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_shader_tmpfs(action: &str) -> ExitCode {
    let config_path = shader_tmpfs::config_path(None::<&Path>);
    let tmpfiles = generated_path(
        "tmpfiles.d",
        "99-kyth-shader.conf",
        "/etc/tmpfiles.d/99-kyth-shader.conf",
    );
    let service = generated_path(
        "systemd",
        "kyth-shader-tmpfs.service",
        "/etc/systemd/system/kyth-shader-tmpfs.service",
    );
    match action {
        "status" => {
            let config = shader_tmpfs::load(&config_path);
            println!(
                "enabled={} size={} active={} kind=other",
                config.enabled,
                config.size,
                if service.is_file() { "enabled" } else { "off" }
            );
            ExitCode::SUCCESS
        }
        "gaming" | "on" => {
            if let Err(code) = ensure_root("shader-tmpfs", &[action.to_string()]) {
                return code;
            }
            let mut config = shader_tmpfs::load(&config_path);
            config.enabled = true;
            if let Err(error) = shader_tmpfs::save(&config_path, &config)
                .and_then(|_| shader_tmpfs::generate(&config, &tmpfiles, &service).map(|_| ()))
            {
                eprintln!("kyth-shader-tmpfs: {error}");
                return ExitCode::from(1);
            }
            println!("shader-tmpfs on");
            ExitCode::SUCCESS
        }
        "balanced" | "off" => {
            if let Err(code) = ensure_root("shader-tmpfs", &[action.to_string()]) {
                return code;
            }
            let config = shader_tmpfs::ShaderTmpfsConfig::default();
            if let Err(error) = shader_tmpfs::save(&config_path, &config)
                .and_then(|_| shader_tmpfs::generate(&config, &tmpfiles, &service).map(|_| ()))
            {
                eprintln!("kyth-shader-tmpfs: {error}");
                return ExitCode::from(1);
            }
            println!("shader-tmpfs off");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("shader-tmpfs", &[action.to_string()]) {
                return code;
            }
            let config = shader_tmpfs::load(&config_path);
            if let Err(error) = shader_tmpfs::generate(&config, &tmpfiles, &service) {
                eprintln!("kyth-shader-tmpfs: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-shader-tmpfs [gaming|balanced|on|off|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_steam_deadzone(action: &str) -> ExitCode {
    let config_path = preference_presets::steam_deadzone_path(None::<&Path>);
    match action {
        "status" => {
            let config = preference_presets::load_steam_deadzone(&config_path);
            println!(
                "profile={} deadzone={} kind=other",
                config.profile, config.deadzone
            );
            ExitCode::SUCCESS
        }
        "gaming" => {
            if let Err(code) = ensure_root("steam-deadzone", &[action.to_string()]) {
                return code;
            }
            let config = preference_presets::SteamDeadzoneConfig {
                profile: "gaming".into(),
                deadzone: 0.05,
            };
            if let Err(error) = preference_presets::save_steam_deadzone(&config_path, &config) {
                eprintln!("kyth-steam-deadzone: {error}");
                return ExitCode::from(1);
            }
            println!("steam-deadzone gaming");
            ExitCode::SUCCESS
        }
        "balanced" | "off" => {
            if let Err(code) = ensure_root("steam-deadzone", &[action.to_string()]) {
                return code;
            }
            let config = preference_presets::SteamDeadzoneConfig::default();
            if let Err(error) = preference_presets::save_steam_deadzone(&config_path, &config) {
                eprintln!("kyth-steam-deadzone: {error}");
                return ExitCode::from(1);
            }
            println!("steam-deadzone balanced");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("steam-deadzone", &[action.to_string()]) {
                return code;
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-steam-deadzone [gaming|balanced|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_hdr_store(action: &str) -> ExitCode {
    let config_path = hdr_store::config_path(None::<&Path>);
    match action {
        "status" => {
            let config = hdr_store::load(&config_path);
            println!("preserve={} kind=other", config.preserve);
            ExitCode::SUCCESS
        }
        "gaming" | "on" => {
            if let Err(code) = ensure_root("hdr-store", &[action.to_string()]) {
                return code;
            }
            if let Err(error) =
                hdr_store::save(&config_path, hdr_store::HdrStoreConfig { preserve: true })
            {
                eprintln!("kyth-hdr-store: {error}");
                return ExitCode::from(1);
            }
            println!("hdr-store on");
            ExitCode::SUCCESS
        }
        "balanced" | "off" => {
            if let Err(code) = ensure_root("hdr-store", &[action.to_string()]) {
                return code;
            }
            if let Err(error) =
                hdr_store::save(&config_path, hdr_store::HdrStoreConfig { preserve: false })
            {
                eprintln!("kyth-hdr-store: {error}");
                return ExitCode::from(1);
            }
            println!("hdr-store off");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("hdr-store", &[action.to_string()]) {
                return code;
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-hdr-store [gaming|balanced|on|off|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_hdr_per_game(action: &str) -> ExitCode {
    let config_path = hdr_per_game::config_path(None::<&Path>);
    match action {
        "status" => {
            let games = hdr_per_game::load(&config_path);
            println!("games={} kind=other", games.len());
            ExitCode::SUCCESS
        }
        "gaming" | "balanced" | "apply" => {
            println!("hdr-per-game {action}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-hdr-per-game [gaming|balanced|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_work_cache(action: &str) -> ExitCode {
    let config_path = work_cache::config_path(None::<&Path>);
    let tmpfiles = generated_path(
        "tmpfiles.d",
        "99-kyth-work-cache.conf",
        "/etc/tmpfiles.d/99-kyth-work-cache.conf",
    );
    let service = generated_path(
        "systemd",
        "kyth-work-cache.service",
        "/etc/systemd/system/kyth-work-cache.service",
    );
    match action {
        "status" => {
            let config = work_cache::load(&config_path);
            println!(
                "enabled={} size={} active={} kind=other",
                config.enabled,
                config.size,
                work_cache::status(&service)
            );
            ExitCode::SUCCESS
        }
        "gaming" | "on" => {
            if let Err(code) = ensure_root("work-cache", &[action.to_string()]) {
                return code;
            }
            let mut config = work_cache::load(&config_path);
            config.enabled = true;
            if let Err(error) = work_cache::save(&config_path, &config)
                .and_then(|_| work_cache::generate(&config, &tmpfiles, &service).map(|_| ()))
            {
                eprintln!("kyth-work-cache: {error}");
                return ExitCode::from(1);
            }
            println!("work-cache on");
            ExitCode::SUCCESS
        }
        "balanced" | "off" => {
            if let Err(code) = ensure_root("work-cache", &[action.to_string()]) {
                return code;
            }
            let config = work_cache::WorkCacheConfig::default();
            if let Err(error) = work_cache::save(&config_path, &config)
                .and_then(|_| work_cache::generate(&config, &tmpfiles, &service).map(|_| ()))
            {
                eprintln!("kyth-work-cache: {error}");
                return ExitCode::from(1);
            }
            println!("work-cache off");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("work-cache", &[action.to_string()]) {
                return code;
            }
            let config = work_cache::load(&config_path);
            if let Err(error) = work_cache::generate(&config, &tmpfiles, &service) {
                eprintln!("kyth-work-cache: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-work-cache [gaming|balanced|on|off|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_telemetry_opt(action: &str) -> ExitCode {
    let config_path = telemetry_opt::config_path(None::<&Path>);
    match action {
        "status" => {
            let config = telemetry_opt::load(&config_path);
            println!(
                "enabled={} collectors={} effective={} kind=other",
                config.enabled,
                config.collectors.len(),
                telemetry_opt::effective_collectors(
                    &config,
                    &["cpu", "gpu", "memory", "disk", "network"]
                )
                .len()
            );
            ExitCode::SUCCESS
        }
        "gaming" | "on" => {
            if let Err(code) = ensure_root("telemetry-opt", &[action.to_string()]) {
                return code;
            }
            let mut config = telemetry_opt::load(&config_path);
            config.enabled = true;
            if let Err(error) = telemetry_opt::save(&config_path, &config) {
                eprintln!("kyth-telemetry-opt: {error}");
                return ExitCode::from(1);
            }
            println!("telemetry-opt on");
            ExitCode::SUCCESS
        }
        "balanced" | "off" => {
            if let Err(code) = ensure_root("telemetry-opt", &[action.to_string()]) {
                return code;
            }
            if let Err(error) = telemetry_opt::purge(&config_path) {
                eprintln!("kyth-telemetry-opt: {error}");
                return ExitCode::from(1);
            }
            println!("telemetry-opt off");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("telemetry-opt", &[action.to_string()]) {
                return code;
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-telemetry-opt [gaming|balanced|on|off|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_perf_gate(action: &str) -> ExitCode {
    let config_path = perf_gate::config_path(None::<&Path>);
    match action {
        "status" => {
            let config = perf_gate::load(&config_path);
            println!(
                "enabled={} threshold={} kind=other",
                config.enabled, config.threshold
            );
            ExitCode::SUCCESS
        }
        "gaming" | "on" => {
            if let Err(code) = ensure_root("perf-gate", &[action.to_string()]) {
                return code;
            }
            if let Err(error) = perf_gate::save(
                &config_path,
                perf_gate::PerfGateConfig {
                    enabled: true,
                    ..perf_gate::load(&config_path)
                },
            ) {
                eprintln!("kyth-perf-gate: {error}");
                return ExitCode::from(1);
            }
            println!("perf-gate on");
            ExitCode::SUCCESS
        }
        "balanced" | "off" => {
            if let Err(code) = ensure_root("perf-gate", &[action.to_string()]) {
                return code;
            }
            if let Err(error) = perf_gate::save(
                &config_path,
                perf_gate::PerfGateConfig {
                    enabled: false,
                    ..perf_gate::load(&config_path)
                },
            ) {
                eprintln!("kyth-perf-gate: {error}");
                return ExitCode::from(1);
            }
            println!("perf-gate off");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("perf-gate", &[action.to_string()]) {
                return code;
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-perf-gate [gaming|balanced|on|off|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_gaming_audit(action: &str) -> ExitCode {
    match action {
        "status" | "apply" | "gaming" | "balanced" => {
            println!("{}", perf_audit::format_audit(&serde_json::json!({})));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-gaming-audit [status|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_system_audit(action: &str) -> ExitCode {
    match action {
        "status" | "apply" | "gaming" | "balanced" => {
            let report = system_audit::summarize(&serde_json::json!({}), 0, None);
            println!("{}", system_audit::format_audit(&report));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-system-audit [status|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_fcitx_latency(action: &str) -> ExitCode {
    let config_path = extended_preferences::fcitx_latency_path(None::<&Path>);
    match action {
        "status" => {
            let config = extended_preferences::load_fcitx_latency(&config_path);
            println!(
                "profile={} latency_ms={} kind=other",
                config.profile, config.latency_ms
            );
            ExitCode::SUCCESS
        }
        "gaming" => {
            if let Err(code) = ensure_root("fcitx-latency", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::FcitxLatencyConfig {
                profile: "gaming".into(),
                latency_ms: 10,
            };
            if let Err(error) = extended_preferences::save_fcitx_latency(&config_path, &config) {
                eprintln!("kyth-fcitx-latency: {error}");
                return ExitCode::from(1);
            }
            println!("fcitx-latency gaming");
            ExitCode::SUCCESS
        }
        "balanced" | "off" => {
            if let Err(code) = ensure_root("fcitx-latency", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::FcitxLatencyConfig::default();
            if let Err(error) = extended_preferences::save_fcitx_latency(&config_path, &config) {
                eprintln!("kyth-fcitx-latency: {error}");
                return ExitCode::from(1);
            }
            println!("fcitx-latency balanced");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("fcitx-latency", &[action.to_string()]) {
                return code;
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-fcitx-latency [gaming|balanced|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_boot_timeout(action: &str) -> ExitCode {
    let config_path = kyth_shared::system::boot_loader::loader_config_path(None::<&Path>);
    let destination = generated_path("boot", "loader.conf", "/boot/loader/loader.conf");
    match action {
        "status" => {
            let config = kyth_shared::system::boot_loader::load_loader(&config_path);
            println!(
                "fast={} timeout={} active={} kind=other",
                config.fast,
                config.timeout,
                kyth_shared::system::boot_loader::loader_status(&destination)
            );
            ExitCode::SUCCESS
        }
        "gaming" => {
            if let Err(code) = ensure_root("boot-timeout", &[action.to_string()]) {
                return code;
            }
            let config = kyth_shared::system::boot_loader::LoaderConfig {
                fast: true,
                timeout: 0,
            };
            if let Err(error) = kyth_shared::system::boot_loader::save_loader(&config_path, &config)
                .and_then(|_| {
                    kyth_shared::system::boot_loader::generate_loader_conf(&config, &destination)
                        .map(|_| ())
                })
            {
                eprintln!("kyth-boot-timeout: {error}");
                return ExitCode::from(1);
            }
            println!("boot-timeout gaming");
            ExitCode::SUCCESS
        }
        "balanced" | "off" => {
            if let Err(code) = ensure_root("boot-timeout", &[action.to_string()]) {
                return code;
            }
            let config = kyth_shared::system::boot_loader::LoaderConfig::default();
            if let Err(error) = kyth_shared::system::boot_loader::save_loader(&config_path, &config)
                .and_then(|_| {
                    kyth_shared::system::boot_loader::generate_loader_conf(&config, &destination)
                        .map(|_| ())
                })
            {
                eprintln!("kyth-boot-timeout: {error}");
                return ExitCode::from(1);
            }
            println!("boot-timeout balanced");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("boot-timeout", &[action.to_string()]) {
                return code;
            }
            let config = kyth_shared::system::boot_loader::load_loader(&config_path);
            if let Err(error) =
                kyth_shared::system::boot_loader::generate_loader_conf(&config, &destination)
            {
                eprintln!("kyth-boot-timeout: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-boot-timeout [gaming|balanced|off|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_kargs_apply(action: &str) -> ExitCode {
    let config_path = gaming_kargs::kargs_path(None::<&Path>);
    match action {
        "status" => {
            let config = gaming_kargs::load_kargs(&config_path);
            let cmdline = std::fs::read_to_string("/proc/cmdline").unwrap_or_default();
            let drift = gaming_kargs::kargs_drift(&config, &cmdline);
            println!(
                "profile={} missing={} extra={} desired={} kind=other",
                config.profile,
                drift.missing.len(),
                drift.extra.len(),
                drift.desired.len()
            );
            ExitCode::SUCCESS
        }
        "gaming" | "performance" | "balanced" => {
            if let Err(code) = ensure_root("kargs-apply", &[action.to_string()]) {
                return code;
            }
            let mut config = gaming_kargs::load_kargs(&config_path);
            config.profile = if action == "gaming" {
                "gaming"
            } else if action == "performance" {
                "performance"
            } else {
                "balanced"
            }
            .into();
            if let Err(error) = gaming_kargs::save_kargs(&config_path, &config) {
                eprintln!("kyth-kargs-apply: {error}");
                return ExitCode::from(1);
            }
            println!("kargs-apply {}", config.profile);
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("kargs-apply", &[action.to_string()]) {
                return code;
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-kargs-apply [gaming|performance|balanced|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn scheduler_bore_available() -> bool {
    std::fs::read_to_string("/usr/share/kyth/kernel-flavor")
        .ok()
        .is_some_and(|flavor| {
            matches!(
                flavor.trim().to_ascii_lowercase().as_str(),
                "cachy" | "cachyos"
            )
        })
}

fn dispatch_sched_arbiter(action: &str) -> ExitCode {
    let config_path = scheduler_arbiter::config_path(None::<&Path>);
    let flag_path = scheduler_arbiter::flag_path(None::<&Path>);
    match action {
        "status" => {
            let config = scheduler_arbiter::ArbiterConfig::load(config_path);
            let active = std::fs::read_to_string(&flag_path)
                .ok()
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .map(|value| scheduler_arbiter::active_from_flag(&value))
                .unwrap_or_else(|| config.chosen.clone());
            println!(
                "chosen={} active={} scx_active={} bore_available={} kind=other",
                config.chosen,
                active,
                scheduler_arbiter::detect_scx_active(),
                scheduler_bore_available()
            );
            ExitCode::SUCCESS
        }
        "gaming" => {
            if let Err(code) = ensure_root("sched-arbiter", &[action.to_string()]) {
                return code;
            }
            let config = scheduler_arbiter::ArbiterConfig::normalized("auto", false, false);
            let state = scheduler_arbiter::desired_state(
                &config,
                scheduler_arbiter::detect_scx_active(),
                scheduler_bore_available(),
            );
            if let Err(error) = scheduler_arbiter::save_config(config_path, &config)
                .and_then(|_| scheduler_arbiter::write_flag(&flag_path, &state))
            {
                eprintln!("kyth-sched-arbiter: {error}");
                return ExitCode::from(1);
            }
            println!("sched-arbiter gaming");
            ExitCode::SUCCESS
        }
        "balanced" | "off" => {
            if let Err(code) = ensure_root("sched-arbiter", &[action.to_string()]) {
                return code;
            }
            let config = scheduler_arbiter::ArbiterConfig::normalized("balanced", false, false);
            let state = scheduler_arbiter::desired_state(&config, false, false);
            if let Err(error) = scheduler_arbiter::save_config(config_path, &config)
                .and_then(|_| scheduler_arbiter::write_flag(&flag_path, &state))
            {
                eprintln!("kyth-sched-arbiter: {error}");
                return ExitCode::from(1);
            }
            println!("sched-arbiter balanced");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("sched-arbiter", &[action.to_string()]) {
                return code;
            }
            let config = scheduler_arbiter::ArbiterConfig::load(config_path);
            let state = scheduler_arbiter::desired_state(
                &config,
                scheduler_arbiter::detect_scx_active(),
                scheduler_bore_available(),
            );
            if let Err(error) = scheduler_arbiter::write_flag(&flag_path, &state) {
                eprintln!("kyth-sched-arbiter: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-sched-arbiter [gaming|balanced|off|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_oom_gaming(action: &str) -> ExitCode {
    let config_path = preference_presets::oom_gaming_path(None::<&Path>);
    let destination = generated_path(
        "systemd/gaming.slice.d",
        "99-kyth-oom.conf",
        "/etc/systemd/system/gaming.slice.d/99-kyth-oom.conf",
    );
    match action {
        "status" => {
            let config = preference_presets::load_oom_gaming(&config_path);
            println!(
                "profile={} limit={} active={} kind=other",
                config.profile,
                config.limit,
                preference_presets::oom_gaming_status(&destination)
            );
            ExitCode::SUCCESS
        }
        "gaming" => {
            if let Err(code) = ensure_root("oom-gaming", &[action.to_string()]) {
                return code;
            }
            let config = preference_presets::OomGamingConfig {
                profile: "gaming".into(),
                limit: "75%".into(),
            };
            if let Err(error) =
                preference_presets::save_oom_gaming(&config_path, &config).and_then(|_| {
                    preference_presets::generate_oom_gaming(&config, &destination).map(|_| ())
                })
            {
                eprintln!("kyth-oom-gaming: {error}");
                return ExitCode::from(1);
            }
            println!("oom-gaming gaming");
            ExitCode::SUCCESS
        }
        "balanced" | "off" => {
            if let Err(code) = ensure_root("oom-gaming", &[action.to_string()]) {
                return code;
            }
            let config = preference_presets::OomGamingConfig::default();
            if let Err(error) =
                preference_presets::save_oom_gaming(&config_path, &config).and_then(|_| {
                    preference_presets::generate_oom_gaming(&config, &destination).map(|_| ())
                })
            {
                eprintln!("kyth-oom-gaming: {error}");
                return ExitCode::from(1);
            }
            println!("oom-gaming balanced");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("oom-gaming", &[action.to_string()]) {
                return code;
            }
            let config = preference_presets::load_oom_gaming(&config_path);
            if let Err(error) = preference_presets::generate_oom_gaming(&config, &destination) {
                eprintln!("kyth-oom-gaming: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-oom-gaming [gaming|balanced|off|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn sysfs_path(subdirectory: &str) -> PathBuf {
    env::var_os("KYTH_SYSFS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/sys"))
        .join(subdirectory)
}

fn dispatch_gaming_master(action: &str) -> ExitCode {
    let config_path = gaming_master::master_config_path(None::<&Path>);
    match action {
        "status" => {
            let profile = gaming_master::load_master(&config_path);
            println!("profile={} active=unknown kind=other", profile.as_str());
            ExitCode::SUCCESS
        }
        "gaming" => {
            if let Err(code) = ensure_root("gaming-master", &[action.to_string()]) {
                return code;
            }
            let thermal = gaming_master::thermal_high(sysfs_path("class/thermal"), 85);
            let battery = gaming_master::battery_low(sysfs_path("class/power_supply"), 30);
            let (profile, reason) =
                gaming_master::effective_gaming(Profile::Gaming, thermal, battery);
            if let Err(error) = gaming_master::save_master(&config_path, profile) {
                eprintln!("kyth-gaming-master: {error}");
                return ExitCode::from(1);
            }
            if profile == Profile::Balanced {
                eprintln!("kyth-gaming-master: staying balanced ({reason})");
            }
            println!("gaming-master {}", profile.as_str());
            ExitCode::SUCCESS
        }
        "balanced" | "off" => {
            if let Err(code) = ensure_root("gaming-master", &[action.to_string()]) {
                return code;
            }
            if let Err(error) = gaming_master::save_master(&config_path, Profile::Balanced) {
                eprintln!("kyth-gaming-master: {error}");
                return ExitCode::from(1);
            }
            println!("gaming-master balanced");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("gaming-master", &[action.to_string()]) {
                return code;
            }
            let requested = gaming_master::load_master(&config_path);
            let thermal = gaming_master::thermal_high(sysfs_path("class/thermal"), 85);
            let battery = gaming_master::battery_low(sysfs_path("class/power_supply"), 30);
            let (profile, reason) = gaming_master::effective_gaming(requested, thermal, battery);
            if profile != requested {
                if let Err(error) = gaming_master::save_master(&config_path, profile) {
                    eprintln!("kyth-gaming-master: {error}");
                    return ExitCode::from(1);
                }
                eprintln!("kyth-gaming-master: staying balanced ({reason})");
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-gaming-master [gaming|balanced|off|apply|status]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_windows_verify(args: &[String]) -> ExitCode {
    let json = args == ["--json"];
    if args.is_empty() || json {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/root"));
        let report = windows_verify::verify(&home, Path::new("/var/home").exists());
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&report).expect("verification report serializes")
            );
        } else {
            for (name, value) in [
                ("bookmarks", &report.bookmarks),
                ("drives", &report.drives),
                ("files", &report.files),
                ("onedrive", &report.onedrive),
                ("pwa", &report.pwa),
                ("parity", &report.parity),
            ] {
                println!("{name}: {value}");
            }
        }
        return if report.parity == "ok" {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        };
    }
    if args == ["status"] {
        println!("profile=balanced active=unknown kind=other");
        return ExitCode::SUCCESS;
    }
    eprintln!("Usage: kyth-windows-verify [--json] (or kyth-tunable-rs windows-verify status)");
    ExitCode::from(2)
}

fn dispatch_mimalloc(action: &str) -> ExitCode {
    let config_path = module_config_path("mimalloc.toml", "/etc/kyth/mimalloc.toml");
    let environment = generated_path(
        "environment.d",
        "99-kyth-mimalloc.conf",
        "/etc/environment.d/99-kyth-mimalloc.conf",
    );
    match action {
        "status" => {
            let config = extended_preferences::load_mimalloc(&config_path);
            println!(
                "enabled={} global={} per_game={} active={} kind=other",
                config.enabled,
                config.global,
                config.per_game,
                extended_preferences::mimalloc_status(&config, environment.is_file())
            );
            ExitCode::SUCCESS
        }
        "on" | "gaming" => {
            if let Err(code) = ensure_root("mimalloc", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::MimallocConfig {
                enabled: true,
                global: false,
                per_game: true,
            };
            if let Err(error) =
                extended_preferences::save_mimalloc(&config_path, &config).and_then(|_| {
                    extended_preferences::generate_mimalloc_env(&config, &environment).map(|_| ())
                })
            {
                eprintln!("kyth-mimalloc: {error}");
                return ExitCode::from(1);
            }
            println!("mimalloc per-game enabled");
            ExitCode::SUCCESS
        }
        "off" | "balanced" => {
            if let Err(code) = ensure_root("mimalloc", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::MimallocConfig {
                enabled: false,
                global: false,
                per_game: true,
            };
            if let Err(error) =
                extended_preferences::save_mimalloc(&config_path, &config).and_then(|_| {
                    extended_preferences::generate_mimalloc_env(&config, &environment).map(|_| ())
                })
            {
                eprintln!("kyth-mimalloc: {error}");
                return ExitCode::from(1);
            }
            println!("mimalloc disabled");
            ExitCode::SUCCESS
        }
        "global" => {
            if let Err(code) = ensure_root("mimalloc", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::MimallocConfig {
                enabled: true,
                global: true,
                per_game: true,
            };
            if let Err(error) =
                extended_preferences::save_mimalloc(&config_path, &config).and_then(|_| {
                    extended_preferences::generate_mimalloc_env(&config, &environment).map(|_| ())
                })
            {
                eprintln!("kyth-mimalloc: {error}");
                return ExitCode::from(1);
            }
            println!("mimalloc global enabled");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("mimalloc", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::load_mimalloc(&config_path);
            if let Err(error) = extended_preferences::generate_mimalloc_env(&config, &environment) {
                eprintln!("kyth-mimalloc: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-mimalloc [status|on|off|global|apply]");
            ExitCode::from(1)
        }
    }
}

fn normalized_mimalloc_run_args(action: &str, args: &[String]) -> Vec<String> {
    if action == "status" {
        vec!["--status".to_string()]
    } else {
        args.to_vec()
    }
}

fn dispatch_mimalloc_run(args: &[String]) -> ExitCode {
    if matches!(args.first().map(String::as_str), Some("--help" | "-h")) {
        println!("Usage: kyth-mimalloc-run [--status] -- <command> [args...]");
        return ExitCode::SUCCESS;
    }
    if args.first().map(String::as_str) == Some("--status") {
        let config_path = module_config_path("mimalloc.toml", "/etc/kyth/mimalloc.toml");
        let environment = generated_path(
            "environment.d",
            "99-kyth-mimalloc.conf",
            "/etc/environment.d/99-kyth-mimalloc.conf",
        );
        let config = extended_preferences::load_mimalloc(&config_path);
        println!(
            "enabled={} global={} per_game={} status={}",
            config.enabled,
            config.global,
            config.per_game,
            extended_preferences::mimalloc_status(&config, environment.is_file())
        );
        return ExitCode::SUCCESS;
    }
    let command_args = if args.first().map(String::as_str) == Some("--") {
        &args[1..]
    } else {
        args
    };
    let Some(program) = command_args.first() else {
        eprintln!("Usage: kyth-mimalloc-run [--status] -- <command> [args...]");
        return ExitCode::from(1);
    };
    let mut command = std::process::Command::new(program);
    command.args(&command_args[1..]);
    let library = extended_preferences::find_mimalloc_library();
    if Path::new(&library).is_file() {
        let preload = match env::var("LD_PRELOAD") {
            Ok(existing) if !existing.is_empty() => format!("{library}:{existing}"),
            _ => library,
        };
        command
            .env("LD_PRELOAD", preload)
            .env("MIMALLOC_LARGE_OS_PAGES", "1");
    }
    match command.status() {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1).try_into().unwrap_or(1)),
        Err(error) => {
            eprintln!("kyth-mimalloc-run: {error}");
            ExitCode::from(1)
        }
    }
}

fn dispatch_sccache(action: &str) -> ExitCode {
    let config_path = module_config_path("sccache.toml", "/etc/kyth/sccache.toml");
    let environment = generated_path(
        "environment.d",
        "99-kyth-sccache.conf",
        "/etc/environment.d/99-kyth-sccache.conf",
    );
    let service = generated_path(
        "systemd",
        "sccache.service",
        "/etc/systemd/system/sccache.service",
    );
    match action {
        "status" => {
            let config = extended_preferences::load_sccache(&config_path);
            println!(
                "enabled={} size={} active={} kind=other",
                config.enabled,
                config.size,
                extended_preferences::sccache_status(&environment)
            );
            ExitCode::SUCCESS
        }
        "on" | "gaming" => {
            if let Err(code) = ensure_root("sccache", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::SccacheConfig {
                enabled: true,
                size: extended_preferences::load_sccache(&config_path).size,
            };
            if let Err(error) =
                extended_preferences::save_sccache(&config_path, &config).and_then(|_| {
                    extended_preferences::generate_sccache(&config, &environment, &service)
                        .map(|_| ())
                })
            {
                eprintln!("kyth-sccache: {error}");
                return ExitCode::from(1);
            }
            println!("sccache on");
            ExitCode::SUCCESS
        }
        "off" | "balanced" => {
            if let Err(code) = ensure_root("sccache", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::SccacheConfig {
                enabled: false,
                size: extended_preferences::load_sccache(&config_path).size,
            };
            if let Err(error) =
                extended_preferences::save_sccache(&config_path, &config).and_then(|_| {
                    extended_preferences::generate_sccache(&config, &environment, &service)
                        .map(|_| ())
                })
            {
                eprintln!("kyth-sccache: {error}");
                return ExitCode::from(1);
            }
            println!("sccache off");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("sccache", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::load_sccache(&config_path);
            if let Err(error) =
                extended_preferences::generate_sccache(&config, &environment, &service)
            {
                eprintln!("kyth-sccache: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-sccache [status|on|off|apply]");
            ExitCode::from(1)
        }
    }
}

fn detect_vram_gb() -> u64 {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return 8;
    };
    for entry in entries.flatten() {
        let path = entry.path().join("device/mem_info_vram_total");
        if let Ok(value) = std::fs::read_to_string(path) {
            if let Ok(bytes) = value.trim().parse::<u64>() {
                return bytes / 1024 / 1024 / 1024;
            }
        }
    }
    8
}

fn dispatch_shader_cache_size(action: &str) -> ExitCode {
    let config_path =
        module_config_path("shader-cache-size.toml", "/etc/kyth/shader-cache-size.toml");
    let environment = generated_path(
        "environment.d",
        "99-kyth-shader-size.conf",
        "/etc/environment.d/99-kyth-shader-size.conf",
    );
    match action {
        "status" => {
            let config = extended_preferences::load_shader_size(&config_path);
            let resolved = extended_preferences::resolve_shader_size(&config, detect_vram_gb());
            println!(
                "mode={} size={} resolved={} active={} kind=other",
                config.mode,
                config.size,
                resolved,
                extended_preferences::shader_size_status(&environment)
            );
            ExitCode::SUCCESS
        }
        "auto" | "gaming" | "balanced" => {
            if let Err(code) = ensure_root("shader-cache-size", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::ShaderSizeConfig {
                mode: "auto".into(),
                size: extended_preferences::load_shader_size(&config_path).size,
            };
            if let Err(error) = extended_preferences::save_shader_size(&config_path, &config)
                .and_then(|_| {
                    extended_preferences::generate_shader_size(
                        &config,
                        detect_vram_gb(),
                        &environment,
                    )
                    .map(|_| ())
                })
            {
                eprintln!("kyth-shader-cache-size: {error}");
                return ExitCode::from(1);
            }
            println!("shader cache auto");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("shader-cache-size", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::load_shader_size(&config_path);
            if let Err(error) =
                extended_preferences::generate_shader_size(&config, detect_vram_gb(), &environment)
            {
                eprintln!("kyth-shader-cache-size: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-shader-cache-size [status|auto|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_wine_sync(action: &str) -> ExitCode {
    let config_path = module_config_path("wine-sync.toml", "/etc/kyth/wine-sync.toml");
    let environment = generated_path(
        "environment.d",
        "99-kyth-wine-sync.conf",
        "/etc/environment.d/99-kyth-wine-sync.conf",
    );
    match action {
        "status" => {
            let config = extended_preferences::load_wine_sync(&config_path);
            let (ntsync, futex2) = extended_preferences::probe_wine_sync();
            let active = std::fs::read_to_string(&environment).ok();
            println!(
                "mode={} probe_ntsync={} probe_futex2={} env={} kind=other",
                config.mode,
                ntsync,
                futex2,
                extended_preferences::wine_sync_status(active.as_deref())
            );
            ExitCode::SUCCESS
        }
        "auto" | "ntsync" | "fsync" | "esync" | "off" => {
            if let Err(code) = ensure_root("wine-sync", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::WineSyncConfig {
                mode: action.into(),
            };
            if let Err(error) = extended_preferences::save_wine_sync(&config_path, &config)
                .and_then(|_| {
                    extended_preferences::generate_wine_env(&config, &environment).map(|_| ())
                })
            {
                eprintln!("kyth-wine-sync: {error}");
                return ExitCode::from(1);
            }
            println!("wine sync {action}");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("wine-sync", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::load_wine_sync(&config_path);
            if let Err(error) = extended_preferences::generate_wine_env(&config, &environment) {
                eprintln!("kyth-wine-sync: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-wine-sync [status|auto|ntsync|fsync|esync|off|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_kwin_latency(action: &str) -> ExitCode {
    let config_path = module_config_path("kwin-latency.toml", "/etc/kyth/kwin-latency.toml");
    let dropin = generated_path(
        "xdg/kwinrc.d",
        "99-kyth-latency.conf",
        "/etc/xdg/kwinrc.d/99-kyth-latency.conf",
    );
    let environment = generated_path(
        "environment.d",
        "99-kyth-kwin.conf",
        "/etc/environment.d/99-kyth-kwin.conf",
    );
    let write_config =
        |config: &kyth_shared::system::kwin_latency::KwinLatencyConfig| -> std::io::Result<()> {
            kyth_shared::atomic_io::atomic_write_text(
                &config_path,
                &config.to_toml(),
                Some(0o600),
            )?;
            let rendered_dropin = config.render_dropin();
            write_optional(&dropin, rendered_dropin.as_deref())?;
            write_optional(&environment, config.render_environment())
        };
    match action {
        "status" => {
            let config = kyth_shared::system::kwin_latency::KwinLatencyConfig::load(&config_path);
            println!(
                "profile={} tearing={} active={} kind=other",
                config.profile,
                config.tearing,
                kyth_shared::system::kwin_latency::status(dropin.is_file())
            );
            ExitCode::SUCCESS
        }
        "gaming" | "balanced" => {
            if let Err(code) = ensure_root("kwin-latency", &[action.to_string()]) {
                return code;
            }
            let config = kyth_shared::system::kwin_latency::KwinLatencyConfig::normalized(
                action,
                action == "gaming",
            );
            if let Err(error) = write_config(&config) {
                eprintln!("kyth-kwin-latency: {error}");
                return ExitCode::from(1);
            }
            println!("kwin-latency {action}");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("kwin-latency", &[action.to_string()]) {
                return code;
            }
            let config = kyth_shared::system::kwin_latency::KwinLatencyConfig::load(&config_path);
            if let Err(error) = write_config(&config) {
                eprintln!("kyth-kwin-latency: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-kwin-latency [status|gaming|balanced|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_epp_ac(action: &str) -> ExitCode {
    let config_path = module_config_path("epp-ac.toml", "/etc/kyth/epp-ac.toml");
    let rule = generated_path(
        "udev/rules.d",
        "61-kyth-epp-ac.rules",
        "/etc/udev/rules.d/61-kyth-epp-ac.rules",
    );
    match action {
        "status" => {
            let config = extended_preferences::load_epp_ac(&config_path);
            println!(
                "enabled={} active={} kind=other",
                config.enabled,
                if rule.is_file() { "enabled" } else { "off" }
            );
            ExitCode::SUCCESS
        }
        "gaming" => {
            if let Err(code) = ensure_root("epp-ac", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::EppAcConfig { enabled: true };
            if let Err(error) = extended_preferences::save_epp_ac(&config_path, &config)
                .and_then(|_| write_optional(&rule, extended_preferences::epp_ac_rule(&config)))
            {
                eprintln!("kyth-epp-ac: {error}");
                return ExitCode::from(1);
            }
            println!("epp-ac gaming");
            ExitCode::SUCCESS
        }
        "balanced" => {
            if let Err(code) = ensure_root("epp-ac", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::EppAcConfig { enabled: false };
            if let Err(error) = extended_preferences::save_epp_ac(&config_path, &config)
                .and_then(|_| write_optional(&rule, None))
            {
                eprintln!("kyth-epp-ac: {error}");
                return ExitCode::from(1);
            }
            println!("epp-ac balanced");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("epp-ac", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::load_epp_ac(&config_path);
            if let Err(error) = write_optional(&rule, extended_preferences::epp_ac_rule(&config)) {
                eprintln!("kyth-epp-ac: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-epp-ac [status|gaming|balanced|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_gaming_cfs(action: &str) -> ExitCode {
    let config_path = module_config_path("gaming-cfs.toml", "/etc/kyth/gaming-cfs.toml");
    let dropin = generated_path(
        "systemd/gaming.slice.d",
        "99-kyth-cfs.conf",
        "/etc/systemd/system/gaming.slice.d/99-kyth-cfs.conf",
    );
    match action {
        "status" => {
            let config = extended_preferences::load_gaming_cfs(&config_path);
            println!(
                "profile={} active={} kind=other",
                config.profile,
                if dropin.is_file() {
                    "gaming"
                } else {
                    "balanced"
                }
            );
            ExitCode::SUCCESS
        }
        "gaming" | "balanced" => {
            if let Err(code) = ensure_root("gaming-cfs", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::GamingCfsConfig {
                profile: action.into(),
            };
            if let Err(error) = extended_preferences::save_gaming_cfs(&config_path, &config)
                .and_then(|_| {
                    write_optional(&dropin, extended_preferences::gaming_cfs_dropin(&config))
                })
            {
                eprintln!("kyth-gaming-cfs: {error}");
                return ExitCode::from(1);
            }
            println!("gaming-cfs {action}");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("gaming-cfs", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::load_gaming_cfs(&config_path);
            if let Err(error) =
                write_optional(&dropin, extended_preferences::gaming_cfs_dropin(&config))
            {
                eprintln!("kyth-gaming-cfs: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-gaming-cfs [status|gaming|balanced|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_pcie(action: &str) -> ExitCode {
    let config_path = module_config_path("pcie.toml", "/etc/kyth/pcie.toml");
    let rule = generated_path(
        "udev/rules.d",
        "61-kyth-pcie.rules",
        "/etc/udev/rules.d/61-kyth-pcie.rules",
    );
    match action {
        "status" => {
            let config = extended_preferences::load_pcie(&config_path);
            println!(
                "profile={} active={} kind=other",
                config.profile,
                if rule.is_file() { "gaming" } else { "balanced" }
            );
            ExitCode::SUCCESS
        }
        "gaming" | "balanced" => {
            if let Err(code) = ensure_root("pcie", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::PcieConfig {
                profile: action.into(),
            };
            if let Err(error) = extended_preferences::save_pcie(&config_path, &config)
                .and_then(|_| extended_preferences::generate_pcie(&config, &rule).map(|_| ()))
            {
                eprintln!("kyth-pcie: {error}");
                return ExitCode::from(1);
            }
            println!("pcie {action}");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("pcie", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::load_pcie(&config_path);
            if let Err(error) = extended_preferences::generate_pcie(&config, &rule) {
                eprintln!("kyth-pcie: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-pcie [status|gaming|balanced|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_pipewire_gaming(action: &str) -> ExitCode {
    let config_path = module_config_path("pipewire-gaming.toml", "/etc/kyth/pipewire-gaming.toml");
    let destination = generated_path(
        "wireplumber/main.lua.d",
        "99-kyth-gaming.lua",
        "/etc/wireplumber/main.lua.d/99-kyth-gaming.lua",
    );
    match action {
        "status" => {
            let config = extended_preferences::load_pipewire_gaming(&config_path);
            println!(
                "profile={} active={} kind=other",
                config.profile,
                if destination.is_file() {
                    "gaming"
                } else {
                    "balanced"
                }
            );
            ExitCode::SUCCESS
        }
        "gaming" | "balanced" => {
            if let Err(code) = ensure_root("pipewire-gaming", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::PipewireGamingConfig {
                profile: action.into(),
                quantum: 128,
            };
            if let Err(error) = extended_preferences::save_pipewire_gaming(&config_path, &config)
                .and_then(|_| {
                    extended_preferences::generate_pipewire_gaming(&config, &destination)
                        .map(|_| ())
                })
            {
                eprintln!("kyth-pipewire-gaming: {error}");
                return ExitCode::from(1);
            }
            println!("pipewire-gaming {action}");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("pipewire-gaming", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::load_pipewire_gaming(&config_path);
            if let Err(error) =
                extended_preferences::generate_pipewire_gaming(&config, &destination)
            {
                eprintln!("kyth-pipewire-gaming: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-pipewire-gaming [status|gaming|balanced|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_psi_gaming(action: &str) -> ExitCode {
    let config_path = module_config_path("psi.toml", "/etc/kyth/psi.toml");
    let dropin = generated_path(
        "systemd/gaming.slice.d",
        "99-kyth-psi.conf",
        "/etc/systemd/system/gaming.slice.d/99-kyth-psi.conf",
    );
    match action {
        "status" => {
            let config = extended_preferences::load_psi(&config_path);
            println!(
                "profile={} active={} kind=other",
                config.profile,
                if dropin.is_file() {
                    "gaming"
                } else {
                    "balanced"
                }
            );
            ExitCode::SUCCESS
        }
        "gaming" | "balanced" => {
            if let Err(code) = ensure_root("psi-gaming", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::PsiConfig {
                profile: action.into(),
            };
            if let Err(error) = extended_preferences::save_psi(&config_path, &config)
                .and_then(|_| extended_preferences::generate_psi(&config, &dropin).map(|_| ()))
            {
                eprintln!("kyth-psi-gaming: {error}");
                return ExitCode::from(1);
            }
            println!("psi-gaming {action}");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("psi-gaming", &[action.to_string()]) {
                return code;
            }
            let config = extended_preferences::load_psi(&config_path);
            if let Err(error) = extended_preferences::generate_psi(&config, &dropin) {
                eprintln!("kyth-psi-gaming: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-psi-gaming [status|gaming|balanced|apply]");
            ExitCode::from(1)
        }
    }
}

fn dispatch_thp_tune(action: &str) -> ExitCode {
    let config_path = if test_mode() {
        env::var_os("XDG_CONFIG_HOME")
            .map(|path| PathBuf::from(path).join("kyth/thp.toml"))
            .unwrap_or_else(|| PathBuf::from("/etc/kyth/thp.toml"))
    } else {
        PathBuf::from("/etc/kyth/thp.toml")
    };
    let drop_in = generated_path(
        "sysctl.d",
        "99-kyth-thp.conf",
        "/etc/sysctl.d/99-kyth-thp.conf",
    );
    match action {
        "status" => {
            let config = extended_preferences::load_thp(&config_path);
            let active = if drop_in.is_file() {
                "kyth"
            } else {
                "balanced"
            };
            println!("profile={} active={} kind=sysctl", config.profile, active);
            ExitCode::SUCCESS
        }
        "gaming" | "balanced" => {
            if let Err(code) = ensure_root("thp-tune", &[action.to_string()]) {
                return code;
            }
            let mut config = extended_preferences::load_thp(&config_path);
            config.profile = if action == "gaming" {
                "kyth"
            } else {
                "balanced"
            }
            .into();
            if let Err(error) =
                extended_preferences::save_thp(&config_path, &config).and_then(|_| {
                    let content = extended_preferences::thp_dropin(&config);
                    match content {
                        Some(content) => kyth_shared::atomic_io::atomic_write_text(
                            &drop_in,
                            &content,
                            Some(0o644),
                        )
                        .map(|_| ()),
                        None => match std::fs::remove_file(&drop_in) {
                            Ok(()) => Ok(()),
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                            Err(error) => Err(error),
                        },
                    }
                })
            {
                eprintln!("kyth-thp-tune: {error}");
                return ExitCode::from(1);
            }
            run_sysctl_system();
            println!("thp-tune {action}");
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root("thp-tune", &[action.to_string()]) {
                return code;
            }
            let config: ThpConfig = extended_preferences::load_thp(&config_path);
            let result = match extended_preferences::thp_dropin(&config) {
                Some(content) => {
                    kyth_shared::atomic_io::atomic_write_text(&drop_in, &content, Some(0o644))
                        .map(|_| ())
                }
                None => match std::fs::remove_file(&drop_in) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(error),
                },
            };
            if let Err(error) = result {
                eprintln!("kyth-thp-tune: {error}");
                return ExitCode::from(1);
            }
            run_sysctl_system();
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-thp-tune [status|gaming|balanced|apply]");
            ExitCode::from(1)
        }
    }
}

fn list() -> ExitCode {
    for spec in tunable_registry::list_tunables(None::<&Path>) {
        println!("{}", spec.name);
    }
    ExitCode::SUCCESS
}

fn list_native() -> ExitCode {
    for name in native_tunable_names() {
        println!("{name}");
    }
    ExitCode::SUCCESS
}

fn dispatch(name: &str, args: &[String]) -> ExitCode {
    let Some(spec) = tunable_registry::get_spec(name, None::<&Path>) else {
        eprintln!("Unknown tunable: {name} (try kyth-tunable --list)");
        return ExitCode::from(1);
    };
    let action = args.first().map(String::as_str).unwrap_or("status");
    if native_other(&spec.name) {
        return match spec.name.as_str() {
            "ananicy" => dispatch_ananicy(action),
            "btrfs-autotune" => dispatch_btrfs_autotune(action),
            "btrfs-tune" => dispatch_btrfs_tune(action),
            "epp-ac" => dispatch_epp_ac(action),
            "gaming-cfs" => dispatch_gaming_cfs(action),
            "pcie" => dispatch_pcie(action),
            "pipewire-gaming" => dispatch_pipewire_gaming(action),
            "psi-gaming" => dispatch_psi_gaming(action),
            "mimalloc" => dispatch_mimalloc(action),
            "mimalloc-run" => {
                // The compatibility aliases use the common `status` action,
                // while the runner's historical CLI spells it `--status`.
                // Normalize both forms before handing the remaining command
                // arguments to the runner.
                let run_args = normalized_mimalloc_run_args(action, args);
                dispatch_mimalloc_run(&run_args)
            }
            "sccache" => dispatch_sccache(action),
            "shader-cache-size" => dispatch_shader_cache_size(action),
            "wine-sync" => dispatch_wine_sync(action),
            "kwin-latency" => dispatch_kwin_latency(action),
            "distrobox-cache" => dispatch_distrobox_cache(action),
            "flatpak-prefetch" => dispatch_flatpak_prefetch(action),
            "flatpak-trim" => dispatch_flatpak_trim(action),
            "readahead" => dispatch_readahead(action),
            "trim-tune" => dispatch_trim_tune(action),
            "uksmd" => dispatch_uksmd(action),
            "irq-tune" => dispatch_irq_tune(action),
            "fscache" => dispatch_fscache(action),
            "journal-tune" => dispatch_journal_tune(action),
            "io-tune" => dispatch_io_tune(action),
            "podman-overlay" => dispatch_podman_overlay(action),
            "podman-btrfs" => dispatch_podman_btrfs(action),
            "gpu-power" => dispatch_gpu_power(action),
            "numa" => dispatch_numa(action),
            "selinux-gaming" => dispatch_selinux_gaming(action),
            "shader-tmpfs" => dispatch_shader_tmpfs(action),
            "steam-deadzone" => dispatch_steam_deadzone(action),
            "hdr-store" => dispatch_hdr_store(action),
            "hdr-per-game" => dispatch_hdr_per_game(action),
            "work-cache" => dispatch_work_cache(action),
            "telemetry-opt" => dispatch_telemetry_opt(action),
            "perf-gate" => dispatch_perf_gate(action),
            "gaming-audit" => dispatch_gaming_audit(action),
            "system-audit" => dispatch_system_audit(action),
            "fcitx-latency" => dispatch_fcitx_latency(action),
            "boot-timeout" => dispatch_boot_timeout(action),
            "kargs-apply" => dispatch_kargs_apply(action),
            "sched-arbiter" => dispatch_sched_arbiter(action),
            "oom-gaming" => dispatch_oom_gaming(action),
            "gaming-master" => dispatch_gaming_master(action),
            "windows-verify" => dispatch_windows_verify(args),
            _ => ExitCode::from(2),
        };
    }
    if spec.kind != "sysctl" || !native_implemented(&spec.name) {
        eprintln!(
            "kyth-{}: native Rust implementation is not ready; use the compatibility dispatcher",
            spec.name
        );
        return ExitCode::from(2);
    }
    if spec.name == "bore" {
        return dispatch_bore(action);
    }
    if spec.name == "net-tune" {
        return dispatch_net_tune(action);
    }
    if spec.name == "zswap" {
        return dispatch_zswap(action);
    }
    if spec.name == "thp-tune" {
        return dispatch_thp_tune(action);
    }
    let config = format!("{}.toml", spec.name);
    match action {
        "status" => {
            let profile = sysctl_profiles::load(&config, None);
            let active = sysctl_profiles::status(&config, None).as_str();
            println!(
                "profile={} active={} kind={}",
                profile.as_str(),
                active,
                spec.kind
            );
            ExitCode::SUCCESS
        }
        "gaming" | "balanced" => {
            if let Err(code) = ensure_root(&spec.name, args) {
                return code;
            }
            let profile = if action == "gaming" {
                Profile::Gaming
            } else {
                Profile::Balanced
            };
            if let Err(error) = sysctl_profiles::save(&config, None, profile).and_then(|_| {
                sysctl_profiles::generate(&config, None, None, Some(profile)).map(|_| ())
            }) {
                eprintln!("kyth-{}: {error}", spec.name);
                return ExitCode::from(1);
            }
            run_sysctl_system();
            println!("{} {action}", spec.name);
            ExitCode::SUCCESS
        }
        "apply" => {
            if let Err(code) = ensure_root(&spec.name, args) {
                return code;
            }
            if let Err(error) = sysctl_profiles::generate(&config, None, None, None) {
                eprintln!("kyth-{}: {error}", spec.name);
                return ExitCode::from(1);
            }
            run_sysctl_system();
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: kyth-{} [status|gaming|balanced|apply]", spec.name);
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) == Some("--list") {
        return list();
    }
    if args.first().map(String::as_str) == Some("--list-native") {
        return list_native();
    }
    let argv0 = invoked_name();
    let Ok((name, action)) = resolve_name(&argv0, &args) else {
        eprintln!("Usage: kyth-tunable-rs <tunable> [status|gaming|balanced|apply]");
        return ExitCode::from(1);
    };
    dispatch(&name, &action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_expected_complete_split() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../build_files/config/tunables.toml");
        let specs = kyth_shared::system::tunable_registry::list_tunables(Some(&path));
        assert_eq!(specs.len(), 94);
        assert_eq!(
            specs.iter().filter(|spec| spec.kind == "sysctl").count(),
            49
        );
        assert_eq!(specs.iter().filter(|spec| spec.kind == "other").count(), 45);
    }

    #[test]
    fn resolves_direct_and_compat_invocations() {
        assert_eq!(
            resolve_name("kyth-swappiness", &[]).unwrap().0,
            "swappiness"
        );
        assert_eq!(
            resolve_name("kyth-tunable-rs", &["swappiness".into(), "status".into()]).unwrap(),
            ("swappiness".into(), vec!["status".into()])
        );
        assert_eq!(
            resolve_name("kyth-tunable", &["swappiness".into(), "status".into()]).unwrap(),
            ("swappiness".into(), vec!["status".into()])
        );
    }

    #[test]
    fn normalizes_mimalloc_runner_status_action() {
        let args = vec!["status".to_string()];
        assert_eq!(
            normalized_mimalloc_run_args("status", &args),
            vec!["--status"]
        );
        let args = vec!["--status".to_string()];
        assert_eq!(normalized_mimalloc_run_args("--status", &args), args);
    }

    #[test]
    fn native_boundary_matches_the_rust_sysctl_registry() {
        assert!(native_sysctl("swappiness"));
        assert!(native_sysctl("tcp-fastopen"));
        assert!(!native_sysctl("gaming-master"));
    }

    #[test]
    fn native_list_is_exactly_the_implemented_sysctl_subset() {
        let names = native_tunable_names();
        assert_eq!(names.len(), 94);
        assert!(names.iter().any(|name| name == "swappiness"));
        assert!(names.iter().any(|name| name == "thp-collapse"));
        assert!(names.iter().any(|name| name == "thp-tune"));
        assert!(names.iter().any(|name| name == "zswap"));
        assert!(names.iter().any(|name| name == "bore"));
        assert!(names.iter().any(|name| name == "net-tune"));
        assert!(names.iter().any(|name| name == "ananicy"));
        assert!(names.iter().any(|name| name == "btrfs-autotune"));
        assert!(names.iter().any(|name| name == "btrfs-tune"));
        assert!(names.iter().any(|name| name == "epp-ac"));
        assert!(names.iter().any(|name| name == "gaming-cfs"));
        assert!(names.iter().any(|name| name == "pcie"));
        assert!(names.iter().any(|name| name == "pipewire-gaming"));
        assert!(names.iter().any(|name| name == "psi-gaming"));
        assert!(names.iter().any(|name| name == "mimalloc"));
        assert!(names.iter().any(|name| name == "mimalloc-run"));
        assert!(names.iter().any(|name| name == "sccache"));
        assert!(names.iter().any(|name| name == "shader-cache-size"));
        assert!(names.iter().any(|name| name == "wine-sync"));
        assert!(names.iter().any(|name| name == "kwin-latency"));
        assert!(names.iter().any(|name| name == "distrobox-cache"));
        assert!(names.iter().any(|name| name == "flatpak-prefetch"));
        assert!(names.iter().any(|name| name == "flatpak-trim"));
        assert!(names.iter().any(|name| name == "readahead"));
        assert!(names.iter().any(|name| name == "trim-tune"));
        assert!(names.iter().any(|name| name == "uksmd"));
        assert!(names.iter().any(|name| name == "irq-tune"));
        assert!(names.iter().any(|name| name == "fscache"));
        assert!(names.iter().any(|name| name == "journal-tune"));
        assert!(names.iter().any(|name| name == "io-tune"));
        assert!(names.iter().any(|name| name == "podman-overlay"));
        assert!(names.iter().any(|name| name == "podman-btrfs"));
        assert!(names.iter().any(|name| name == "gpu-power"));
        assert!(names.iter().any(|name| name == "numa"));
        assert!(names.iter().any(|name| name == "selinux-gaming"));
        assert!(names.iter().any(|name| name == "shader-tmpfs"));
        assert!(names.iter().any(|name| name == "steam-deadzone"));
        assert!(names.iter().any(|name| name == "hdr-store"));
        assert!(names.iter().any(|name| name == "hdr-per-game"));
        assert!(names.iter().any(|name| name == "work-cache"));
        assert!(names.iter().any(|name| name == "telemetry-opt"));
        assert!(names.iter().any(|name| name == "perf-gate"));
        assert!(names.iter().any(|name| name == "gaming-audit"));
        assert!(names.iter().any(|name| name == "system-audit"));
        assert!(names.iter().any(|name| name == "fcitx-latency"));
        assert!(names.iter().any(|name| name == "boot-timeout"));
        assert!(names.iter().any(|name| name == "kargs-apply"));
        assert!(names.iter().any(|name| name == "sched-arbiter"));
        assert!(names.iter().any(|name| name == "oom-gaming"));
        assert!(names.iter().any(|name| name == "gaming-master"));
        assert!(names.iter().any(|name| name == "windows-verify"));
    }
}
