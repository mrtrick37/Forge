//! Offline profile-backed sysctl tuning helpers.
//!
//! These small Python modules all have the same contract: persist a
//! `balanced`/`gaming` profile and, for gaming, render one explicit sysctl
//! drop-in.  The privileged service still owns applying the drop-in.  Keeping
//! the render step here makes the native callers independent of Python while
//! retaining the existing file names and payloads.

use super::tuning_profile::{config_path, load_profile, profile_from_str, Profile};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
struct Spec {
    config: &'static str,
    drop_in: &'static str,
    comment: &'static str,
    payload: &'static str,
}

const SPECS: &[Spec] = &[
    Spec { config: "aio-max.toml", drop_in: "99-kyth-aio-max.conf", comment: "Kyth aio max", payload: "fs.aio-max-nr=1048576\n" },
    Spec { config: "inotify-watches.toml", drop_in: "99-kyth-inotify-watches.conf", comment: "Kyth inotify watches", payload: "fs.inotify.max_user_watches=1048576\n" },
    Spec { config: "rmem-default.toml", drop_in: "99-kyth-rmem-default.conf", comment: "Kyth rmem default", payload: "net.core.rmem_default=262144\n" },
    Spec { config: "rmem-max.toml", drop_in: "99-kyth-rmem-max.conf", comment: "Kyth rmem max", payload: "net.core.rmem_max=16777216\n" },
    Spec { config: "vfs-cache.toml", drop_in: "99-kyth-vfs-cache.conf", comment: "Kyth vfs cache", payload: "vm.vfs_cache_pressure=50\n" },
    Spec { config: "overcommit-memory.toml", drop_in: "99-kyth-overcommit-memory.conf", comment: "Kyth overcommit memory", payload: "vm.overcommit_memory=1\n" },
    Spec { config: "page-cluster.toml", drop_in: "99-kyth-page-cluster.conf", comment: "Kyth page cluster", payload: "vm.page-cluster=0\n" },
    Spec { config: "dirty-ratio.toml", drop_in: "99-kyth-dirty-ratio.conf", comment: "Kyth dirty ratio", payload: "vm.dirty_ratio=5\nvm.dirty_background_ratio=5\nvm.dirty_writeback_centisecs=500\n" },
    Spec { config: "dirty-expire.toml", drop_in: "99-kyth-dirty-expire.conf", comment: "Kyth dirty expire", payload: "vm.dirty_expire_centisecs=100\n" },
    Spec { config: "netdev-budget.toml", drop_in: "99-kyth-netdev-budget.conf", comment: "Kyth netdev budget", payload: "net.core.netdev_budget=600\n" },
    Spec { config: "net-backlog.toml", drop_in: "99-kyth-net-backlog.conf", comment: "Kyth net backlog", payload: "net.core.netdev_max_backlog=5000\n" },
    Spec { config: "swappiness.toml", drop_in: "99-kyth-swappiness.conf", comment: "Kyth swappiness", payload: "vm.swappiness=10\n" },
    Spec { config: "busy-poll.toml", drop_in: "99-kyth-busy-poll.conf", comment: "Kyth busy poll", payload: "net.core.busy_poll=50\n" },
    Spec { config: "busy-read.toml", drop_in: "99-kyth-busy-read.conf", comment: "Kyth busy read", payload: "net.core.busy_read=50\n" },
    Spec { config: "compaction.toml", drop_in: "99-kyth-compaction.conf", comment: "Kyth compaction", payload: "vm.compaction_proactiveness=0\n" },
    Spec { config: "thp-collapse.toml", drop_in: "99-kyth-thp-collapse.conf", comment: "Kyth THP collapse", payload: "kernel.khugepaged_defrag=0\n" },
    Spec { config: "numa-balancing.toml", drop_in: "99-kyth-numa-balancing.conf", comment: "Kyth numa balancing", payload: "kernel.numa_balancing=0\n" },
    Spec { config: "psi-poll.toml", drop_in: "99-kyth-psi-poll.conf", comment: "Kyth PSI poll", payload: "vm.pressure_poll=500\n" },
    Spec { config: "tcp-ecn.toml", drop_in: "99-kyth-tcp-ecn.conf", comment: "Kyth tcp ecn", payload: "net.ipv4.tcp_ecn=1\n" },
    Spec { config: "tcp-fastopen.toml", drop_in: "99-kyth-tcp-fastopen.conf", comment: "Kyth tcp fastopen", payload: "net.ipv4.tcp_fastopen=3\n" },
    Spec { config: "tcp-fin-timeout.toml", drop_in: "99-kyth-tcp-fin-timeout.conf", comment: "Kyth tcp fin timeout", payload: "net.ipv4.tcp_fin_timeout=30\n" },
    Spec { config: "tcp-keepalive.toml", drop_in: "99-kyth-tcp-keepalive.conf", comment: "Kyth tcp keepalive", payload: "net.ipv4.tcp_keepalive_time=120\n" },
    Spec { config: "tcp-no-metrics-save.toml", drop_in: "99-kyth-tcp-no-metrics-save.conf", comment: "Kyth tcp no metrics save", payload: "net.ipv4.tcp_no_metrics_save=1\n" },
    Spec { config: "tcp-notsent.toml", drop_in: "99-kyth-tcp-notsent.conf", comment: "Kyth tcp notsent", payload: "net.ipv4.tcp_notsent_lowat=16384\n" },
    Spec { config: "tcp-orphan-retries.toml", drop_in: "99-kyth-tcp-orphan-retries.conf", comment: "Kyth tcp orphan retries", payload: "net.ipv4.tcp_orphan_retries=0\n" },
    Spec { config: "tcp-retries1.toml", drop_in: "99-kyth-tcp-retries1.conf", comment: "Kyth tcp retries1", payload: "net.ipv4.tcp_retries1=3\n" },
    Spec { config: "tcp-retries2.toml", drop_in: "99-kyth-tcp-retries2.conf", comment: "Kyth tcp retries2", payload: "net.ipv4.tcp_retries2=8\n" },
    Spec { config: "tcp-sack.toml", drop_in: "99-kyth-tcp-sack.conf", comment: "Kyth tcp sack", payload: "net.ipv4.tcp_sack=1\n" },
    Spec { config: "tcp-slow-start.toml", drop_in: "99-kyth-tcp-slow-start.conf", comment: "Kyth tcp slow start", payload: "net.ipv4.tcp_slow_start_after_idle=0\n" },
    Spec { config: "tcp-timestamps.toml", drop_in: "99-kyth-tcp-timestamps.conf", comment: "Kyth tcp timestamps", payload: "net.ipv4.tcp_timestamps=1\n" },
    Spec { config: "tcp-window-scaling.toml", drop_in: "99-kyth-tcp-window-scaling.conf", comment: "Kyth tcp window scaling", payload: "net.ipv4.tcp_window_scaling=1\n" },
    Spec { config: "vm-stat.toml", drop_in: "99-kyth-vm-stat.conf", comment: "Kyth vm stat", payload: "vm.stat_interval=10\n" },
    Spec { config: "wmem-default.toml", drop_in: "99-kyth-wmem-default.conf", comment: "Kyth wmem default", payload: "net.core.wmem_default=262144\n" },
    Spec { config: "wmem-max.toml", drop_in: "99-kyth-wmem-max.conf", comment: "Kyth wmem max", payload: "net.core.wmem_max=16777216\n" },
    Spec { config: "max-map-count.toml", drop_in: "99-kyth-max-map-count.conf", comment: "Kyth max map count", payload: "vm.max_map_count=2147483642\n" },
    Spec { config: "min-free-kbytes.toml", drop_in: "99-kyth-min-free-kbytes.conf", comment: "Kyth min free kbytes", payload: "vm.min_free_kbytes=131072\n" },
    Spec { config: "somaxconn.toml", drop_in: "99-kyth-somaxconn.conf", comment: "Kyth somaxconn", payload: "net.core.somaxconn=8192\n" },
    Spec { config: "sched-autogroup.toml", drop_in: "99-kyth-sched-autogroup.conf", comment: "Kyth autogroup", payload: "kernel.sched_autogroup_enabled=0\n" },
    Spec { config: "sched-child.toml", drop_in: "99-kyth-sched-child.conf", comment: "Kyth sched child", payload: "kernel.sched_child_runs_first=0\n" },
    Spec { config: "sched-nr-migrate.toml", drop_in: "99-kyth-sched-nr-migrate.conf", comment: "Kyth nr migrate", payload: "kernel.sched_nr_migrate=64\n" },
    Spec { config: "sched-latency.toml", drop_in: "99-kyth-sched-latency.conf", comment: "Kyth sched latency", payload: "kernel.sched_latency_ns = 6000000\nkernel.sched_min_granularity_ns = 1000000\nkernel.sched_wakeup_granularity_ns = 1000000\nkernel.sched_migration_cost_ns = 500000\nkernel.sched_nr_migrate = 32\n" },
    Spec { config: "file-max.toml", drop_in: "99-kyth-file-max.conf", comment: "Kyth file max", payload: "fs.file-max=2097152\n" },
    Spec { config: "tcp-mtu-probing.toml", drop_in: "99-kyth-tcp-mtu-probing.conf", comment: "Kyth tcp mtu probing", payload: "net.ipv4.tcp_mtu_probing=1\n" },
    Spec { config: "vm-watermark.toml", drop_in: "99-kyth-vm-watermark.conf", comment: "Kyth vm watermark", payload: "vm.watermark_scale_factor=500\n" },
    Spec { config: "perf-cpu.toml", drop_in: "99-kyth-perf-cpu.conf", comment: "Kyth perf cpu", payload: "kernel.perf_cpu_time_max_percent=5\n" },
];

fn spec(config: &str) -> &'static Spec {
    SPECS
        .iter()
        .find(|candidate| candidate.config == config)
        .expect("sysctl profile spec exists")
}

fn paths(spec: &Spec, path: Option<&Path>) -> (PathBuf, PathBuf) {
    let config = config_path(
        path,
        PathBuf::from("/etc/kyth").join(spec.config),
        spec.config,
    );
    (config, default_drop_in(spec.drop_in))
}

fn default_drop_in(drop_in: &str) -> PathBuf {
    if std::env::var("KYTH_TEST_MODE").ok().as_deref() == Some("1") {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config).join("kyth/sysctl.d").join(drop_in);
        }
    }
    PathBuf::from("/etc/sysctl.d").join(drop_in)
}

/// Read a profile, treating malformed or unknown values as balanced.
pub fn load(config: &str, path: Option<&Path>) -> Profile {
    let (path, _) = paths(spec(config), path);
    load_profile(path)
}

/// Persist a normalized profile using the same compact TOML contract as the
/// Python modules.
pub fn save(config: &str, path: Option<&Path>, profile: Profile) -> std::io::Result<PathBuf> {
    let item = spec(config);
    let (path, _) = paths(item, path);
    super::tuning_profile::save_profile(&path, item.comment, profile)?;
    Ok(path)
}

/// Render the gaming drop-in, or remove this module's drop-in for balanced.
/// The destination is explicit and never interpreted as a command.
pub fn generate(
    config: &str,
    path: Option<&Path>,
    destination: Option<&Path>,
    profile: Option<Profile>,
) -> std::io::Result<Option<PathBuf>> {
    let item = spec(config);
    let (_, default_drop_in) = paths(item, path);
    let default_drop_in = destination
        .map(Path::to_path_buf)
        .unwrap_or(default_drop_in);
    let profile = profile.unwrap_or_else(|| load(config, path));
    if profile != Profile::Gaming {
        match std::fs::remove_file(&default_drop_in) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        return Ok(None);
    }
    let content = format!("# {} gaming — generated\n{}", item.comment, item.payload);
    crate::atomic_io::atomic_write_text(&default_drop_in, &content, Some(0o644))?;
    Ok(Some(default_drop_in))
}

pub fn status(config: &str, drop_in: Option<&Path>) -> Profile {
    let item = spec(config);
    let default_drop_in = default_drop_in(item.drop_in);
    super::tuning_profile::status_from_conf(drop_in.unwrap_or(&default_drop_in))
}

/// Expose the canonical config/drop-in names to UI and service adapters.
pub fn known_profiles() -> impl Iterator<Item = (&'static str, &'static str)> {
    SPECS.iter().map(|item| (item.config, item.drop_in))
}

/// Resolve a tunable registry name to a profile-backed sysctl model.
pub fn profile_config_for_tunable(name: &str) -> Option<&'static str> {
    let config = format!("{name}.toml");
    SPECS
        .iter()
        .find(|item| item.config == config)
        .map(|item| item.config)
}

pub fn load_tunable(name: &str, path: Option<&Path>) -> Option<Profile> {
    profile_config_for_tunable(name).map(|config| load(config, path))
}

pub fn status_tunable(name: &str, drop_in: Option<&Path>) -> Option<Profile> {
    profile_config_for_tunable(name).map(|config| status(config, drop_in))
}

pub fn generate_tunable(
    name: &str,
    path: Option<&Path>,
    destination: Option<&Path>,
    profile: Option<Profile>,
) -> Option<std::io::Result<Option<PathBuf>>> {
    profile_config_for_tunable(name).map(|config| generate(config, path, destination, profile))
}

macro_rules! profile_module {
    ($load:ident, $save:ident, $generate:ident, $status:ident, $config:literal) => {
        pub fn $load(path: Option<&Path>) -> Profile {
            load($config, path)
        }
        pub fn $save(path: Option<&Path>, profile: Profile) -> std::io::Result<PathBuf> {
            save($config, path, profile)
        }
        pub fn $generate(
            path: Option<&Path>,
            destination: Option<&Path>,
            profile: Option<Profile>,
        ) -> std::io::Result<Option<PathBuf>> {
            generate($config, path, destination, profile)
        }
        pub fn $status(drop_in: Option<&Path>) -> Profile {
            status($config, drop_in)
        }
    };
}

profile_module!(
    load_aio_max,
    save_aio_max,
    generate_aio_max,
    aio_max_status,
    "aio-max.toml"
);
profile_module!(
    load_inotify_watches,
    save_inotify_watches,
    generate_inotify_watches,
    inotify_watches_status,
    "inotify-watches.toml"
);
profile_module!(
    load_rmem_default,
    save_rmem_default,
    generate_rmem_default,
    rmem_default_status,
    "rmem-default.toml"
);
profile_module!(
    load_rmem_max,
    save_rmem_max,
    generate_rmem_max,
    rmem_max_status,
    "rmem-max.toml"
);
profile_module!(
    load_vfs_cache,
    save_vfs_cache,
    generate_vfs_cache,
    vfs_cache_status,
    "vfs-cache.toml"
);
profile_module!(
    load_overcommit_memory,
    save_overcommit_memory,
    generate_overcommit_memory,
    overcommit_memory_status,
    "overcommit-memory.toml"
);
profile_module!(
    load_page_cluster,
    save_page_cluster,
    generate_page_cluster,
    page_cluster_status,
    "page-cluster.toml"
);
profile_module!(
    load_dirty_ratio,
    save_dirty_ratio,
    generate_dirty_ratio,
    dirty_ratio_status,
    "dirty-ratio.toml"
);
profile_module!(
    load_dirty_expire,
    save_dirty_expire,
    generate_dirty_expire,
    dirty_expire_status,
    "dirty-expire.toml"
);
profile_module!(
    load_netdev_budget,
    save_netdev_budget,
    generate_netdev_budget,
    netdev_budget_status,
    "netdev-budget.toml"
);
profile_module!(
    load_backlog,
    save_backlog,
    generate_backlog,
    backlog_status,
    "net-backlog.toml"
);
profile_module!(
    load_swappiness,
    save_swappiness,
    generate_swappiness,
    swappiness_status,
    "swappiness.toml"
);
profile_module!(
    load_busy_poll,
    save_busy_poll,
    generate_busy_poll,
    busy_poll_status,
    "busy-poll.toml"
);
profile_module!(
    load_busy_read,
    save_busy_read,
    generate_busy_read,
    busy_read_status,
    "busy-read.toml"
);
profile_module!(
    load_compaction,
    save_compaction,
    generate_compaction,
    compaction_status,
    "compaction.toml"
);
profile_module!(
    load_thp_collapse,
    save_thp_collapse,
    generate_thp_collapse,
    thp_collapse_status,
    "thp-collapse.toml"
);
profile_module!(
    load_numa_balancing,
    save_numa_balancing,
    generate_numa_balancing,
    numa_balancing_status,
    "numa-balancing.toml"
);
profile_module!(
    load_psi_poll,
    save_psi_poll,
    generate_psi_poll,
    psi_poll_status,
    "psi-poll.toml"
);
profile_module!(
    load_tcp_ecn,
    save_tcp_ecn,
    generate_tcp_ecn,
    tcp_ecn_status,
    "tcp-ecn.toml"
);
profile_module!(
    load_tcp_fastopen,
    save_tcp_fastopen,
    generate_tcp_fastopen,
    tcp_fastopen_status,
    "tcp-fastopen.toml"
);
profile_module!(
    load_tcp_fin_timeout,
    save_tcp_fin_timeout,
    generate_tcp_fin_timeout,
    tcp_fin_timeout_status,
    "tcp-fin-timeout.toml"
);
profile_module!(
    load_tcp_keepalive,
    save_tcp_keepalive,
    generate_tcp_keepalive,
    tcp_keepalive_status,
    "tcp-keepalive.toml"
);
profile_module!(
    load_tcp_no_metrics_save,
    save_tcp_no_metrics_save,
    generate_tcp_no_metrics_save,
    tcp_no_metrics_save_status,
    "tcp-no-metrics-save.toml"
);
profile_module!(
    load_tcp_notsent,
    save_tcp_notsent,
    generate_tcp_notsent,
    tcp_notsent_status,
    "tcp-notsent.toml"
);
profile_module!(
    load_tcp_orphan_retries,
    save_tcp_orphan_retries,
    generate_tcp_orphan_retries,
    tcp_orphan_retries_status,
    "tcp-orphan-retries.toml"
);
profile_module!(
    load_tcp_retries1,
    save_tcp_retries1,
    generate_tcp_retries1,
    tcp_retries1_status,
    "tcp-retries1.toml"
);
profile_module!(
    load_tcp_retries2,
    save_tcp_retries2,
    generate_tcp_retries2,
    tcp_retries2_status,
    "tcp-retries2.toml"
);
profile_module!(
    load_tcp_sack,
    save_tcp_sack,
    generate_tcp_sack,
    tcp_sack_status,
    "tcp-sack.toml"
);
profile_module!(
    load_tcp_slow_start,
    save_tcp_slow_start,
    generate_tcp_slow_start,
    tcp_slow_start_status,
    "tcp-slow-start.toml"
);
profile_module!(
    load_tcp_timestamps,
    save_tcp_timestamps,
    generate_tcp_timestamps,
    tcp_timestamps_status,
    "tcp-timestamps.toml"
);
profile_module!(
    load_tcp_window_scaling,
    save_tcp_window_scaling,
    generate_tcp_window_scaling,
    tcp_window_scaling_status,
    "tcp-window-scaling.toml"
);
profile_module!(
    load_vm_stat,
    save_vm_stat,
    generate_vm_stat,
    vm_stat_status,
    "vm-stat.toml"
);
profile_module!(
    load_wmem_default,
    save_wmem_default,
    generate_wmem_default,
    wmem_default_status,
    "wmem-default.toml"
);
profile_module!(
    load_wmem_max,
    save_wmem_max,
    generate_wmem_max,
    wmem_max_status,
    "wmem-max.toml"
);
profile_module!(
    load_max_map_count,
    save_max_map_count,
    generate_max_map_count,
    max_map_count_status,
    "max-map-count.toml"
);
profile_module!(
    load_min_free_kbytes,
    save_min_free_kbytes,
    generate_min_free_kbytes,
    min_free_kbytes_status,
    "min-free-kbytes.toml"
);
profile_module!(
    load_somaxconn,
    save_somaxconn,
    generate_somaxconn,
    somaxconn_status,
    "somaxconn.toml"
);
profile_module!(
    load_autogroup,
    save_autogroup,
    generate_autogroup,
    autogroup_status,
    "sched-autogroup.toml"
);
profile_module!(
    load_sched_child,
    save_sched_child,
    generate_sched_child,
    sched_child_status,
    "sched-child.toml"
);
profile_module!(
    load_nr_migrate,
    save_nr_migrate,
    generate_nr_migrate,
    nr_migrate_status,
    "sched-nr-migrate.toml"
);
profile_module!(
    load_sched_latency,
    save_sched_latency,
    generate_sched_latency,
    sched_latency_status,
    "sched-latency.toml"
);
profile_module!(
    load_file_max,
    save_file_max,
    generate_file_max,
    file_max_status,
    "file-max.toml"
);
profile_module!(
    load_tcp_mtu_probing,
    save_tcp_mtu_probing,
    generate_tcp_mtu_probing,
    tcp_mtu_probing_status,
    "tcp-mtu-probing.toml"
);
profile_module!(
    load_watermark,
    save_watermark,
    generate_watermark,
    watermark_status,
    "vm-watermark.toml"
);
profile_module!(
    load_perf_cpu,
    save_perf_cpu,
    generate_perf_cpu,
    perf_cpu_status,
    "perf-cpu.toml"
);

/// Keep profile parsing available to callers that already have a TOML value.
pub fn normalize_profile(value: Option<&str>) -> Profile {
    profile_from_str(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn round_trips_all_small_profiles() {
        let directory = tempdir().unwrap();
        for (config, _) in known_profiles() {
            let path = directory.path().join(config);
            save(config, Some(&path), Profile::Gaming).unwrap();
            assert_eq!(load(config, Some(&path)), Profile::Gaming, "{config}");
        }
    }

    #[test]
    fn renders_exact_drop_in_and_removes_it_for_balanced() {
        let directory = tempdir().unwrap();
        let config = directory.path().join("swappiness.toml");
        let drop_in = directory.path().join("99-kyth-swappiness.conf");
        save("swappiness.toml", Some(&config), Profile::Gaming).unwrap();
        let generated = generate_swappiness(Some(&config), Some(&drop_in), None).unwrap();
        assert_eq!(generated.as_deref(), Some(drop_in.as_path()));
        assert_eq!(
            fs::read_to_string(&drop_in).unwrap(),
            "# Kyth swappiness gaming — generated\nvm.swappiness=10\n"
        );
        generate_swappiness(Some(&config), Some(&drop_in), Some(Profile::Balanced)).unwrap();
        assert!(!drop_in.exists());
        assert_eq!(load_swappiness(Some(&config)), Profile::Gaming);

        let thp_config = directory.path().join("thp-collapse.toml");
        let thp_drop_in = directory.path().join("99-kyth-thp-collapse.conf");
        save("thp-collapse.toml", Some(&thp_config), Profile::Gaming).unwrap();
        generate_thp_collapse(Some(&thp_config), Some(&thp_drop_in), None).unwrap();
        assert_eq!(
            fs::read_to_string(&thp_drop_in).unwrap(),
            "# Kyth THP collapse gaming — generated\nkernel.khugepaged_defrag=0\n"
        );
    }

    #[test]
    fn malformed_profile_is_safe_and_unknown_names_panic_only_for_programmer_error() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("aio-max.toml");
        fs::write(&path, "profile = \"unknown\"\n").unwrap();
        assert_eq!(load_aio_max(Some(&path)), Profile::Balanced);
    }
}
