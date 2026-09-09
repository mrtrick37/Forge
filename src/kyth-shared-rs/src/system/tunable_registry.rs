//! Declarative tunable registry lookup.
//!
//! The Python dispatcher dynamically imports and executes tunable modules.
//! Rust callers only need the data contract: canonical names, wrapper names,
//! implementation module, and whether a tunable is sysctl-backed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunableSpec {
    pub name: String,
    pub module: String,
    pub kind: String,
    pub wrapper: String,
}

impl TunableSpec {
    fn new(name: &str, module: &str, kind: &str) -> Self {
        Self {
            name: name.into(),
            module: module.into(),
            kind: kind.into(),
            wrapper: format!("kyth-{name}"),
        }
    }
}

fn fallback_registry() -> BTreeMap<String, TunableSpec> {
    let mut registry = BTreeMap::new();
    for (name, module) in [
        ("aio-max", "aio_max"),
        ("busy-poll", "busy_poll"),
        ("busy-read", "busy_read"),
        ("compaction", "compaction_tune"),
        ("dirty-expire", "dirty_expire"),
        ("dirty-ratio", "dirty_ratio"),
        ("file-max", "file_max"),
        ("inotify-watches", "inotify_watches"),
        ("max-map-count", "max_map_count"),
        ("min-free-kbytes", "min_free_kbytes"),
        ("net-backlog", "net_backlog"),
        ("netdev-budget", "netdev_budget"),
        ("numa-balancing", "numa_balancing"),
        ("overcommit-memory", "overcommit_memory"),
        ("page-cluster", "page_cluster"),
        ("perf-cpu", "perf_cpu"),
        ("psi-poll", "psi_poll"),
        ("rmem-default", "rmem_default"),
        ("rmem-max", "rmem_max"),
        ("sched-autogroup", "sched_autogroup"),
        ("sched-child", "sched_child"),
        ("sched-latency", "sched_latency"),
        ("sched-nr-migrate", "sched_nr_migrate"),
        ("somaxconn", "somaxconn"),
        ("swappiness", "swappiness"),
        ("tcp-ecn", "tcp_ecn"),
        ("tcp-fastopen", "tcp_fastopen"),
        ("tcp-fin-timeout", "tcp_fin_timeout"),
        ("tcp-keepalive", "tcp_keepalive"),
        ("tcp-mtu-probing", "tcp_mtu_probing"),
        ("tcp-no-metrics-save", "tcp_no_metrics_save"),
        ("tcp-notsent", "tcp_notsent"),
        ("tcp-orphan-retries", "tcp_orphan_retries"),
        ("tcp-retries1", "tcp_retries1"),
        ("tcp-retries2", "tcp_retries2"),
        ("tcp-sack", "tcp_sack"),
        ("tcp-slow-start", "tcp_slow_start"),
        ("tcp-timestamps", "tcp_timestamps"),
        ("tcp-window-scaling", "tcp_window_scaling"),
        ("thp-collapse", "thp_collapse"),
        ("thp-tune", "thp_tune"),
        ("bore", "bore_tune"),
        ("net-tune", "net_latency"),
        ("vm-stat", "vm_stat"),
        ("vm-watermark", "vm_watermark"),
        ("vfs-cache", "vfs_cache_pressure"),
        ("wmem-default", "wmem_default"),
        ("wmem-max", "wmem_max"),
        ("zswap", "zswap_preset"),
    ] {
        registry.insert(name.into(), TunableSpec::new(name, module, "sysctl"));
    }
    for (name, module) in [
        ("ananicy", "ananicy_preset"),
        ("boot-timeout", "boot_loader"),
        ("btrfs-autotune", "btrfs_autotune"),
        ("btrfs-tune", "btrfs_perf"),
        ("distrobox-cache", "distrobox_cache"),
        ("epp-ac", "epp_ac"),
        ("fcitx-latency", "fcitx_latency"),
        ("flatpak-prefetch", "flatpak_prefetch"),
        ("flatpak-trim", "flatpak_trim"),
        ("fscache", "fscache_tune"),
        ("gaming-audit", "perf_audit"),
        ("gaming-cfs", "gaming_cfs"),
        ("gaming-master", "gaming_master"),
        ("gpu-power", "gpu_power"),
        ("hdr-per-game", "hdr_per_game"),
        ("hdr-store", "hdr_store"),
        ("io-tune", "io_tune"),
        ("irq-tune", "irq_tune"),
        ("journal-tune", "journal_tune"),
        ("kargs-apply", "kargs_preset"),
        ("kwin-latency", "kwin_latency"),
        ("mimalloc", "mimalloc_preset"),
        ("mimalloc-run", "mimalloc_preset"),
        ("numa", "numa_tune"),
        ("oom-gaming", "oom_gaming"),
        ("pcie", "pcie_aspm"),
        ("perf-gate", "perf_gate"),
        ("pipewire-gaming", "pipewire_gaming"),
        ("podman-btrfs", "podman_btrfs"),
        ("podman-overlay", "overlay_tune"),
        ("psi-gaming", "psi_gaming"),
        ("readahead", "readahead_preset"),
        ("sccache", "sccache_preset"),
        ("sched-arbiter", "sched_arbiter"),
        ("selinux-gaming", "selinux_gaming"),
        ("shader-cache-size", "shader_cache_size"),
        ("shader-tmpfs", "shader_tmpfs"),
        ("steam-deadzone", "steam_deadzone"),
        ("system-audit", "system_audit"),
        ("telemetry-opt", "telemetry_opt"),
        ("thp-tune", "thp_tune"),
        ("trim-tune", "trim_preset"),
        ("uksmd", "uksmd_preset"),
        ("windows-verify", "windows_verify"),
        ("wine-sync", "wine_sync"),
        ("work-cache", "work_cache"),
    ] {
        registry
            .entry(name.into())
            .or_insert_with(|| TunableSpec::new(name, module, "other"));
    }
    registry
}

fn parse_registry(raw: &str) -> Option<BTreeMap<String, TunableSpec>> {
    let value = raw.parse::<toml::Value>().ok()?;
    let tunables = value.get("tunables")?.as_table()?;
    let parsed: BTreeMap<_, _> = tunables
        .iter()
        .filter_map(|(name, value)| {
            let item = value.as_table()?;
            let module = item
                .get("module")
                .and_then(toml::Value::as_str)
                .unwrap_or_default();
            if module.is_empty() {
                return None;
            }
            let kind = item
                .get("kind")
                .and_then(toml::Value::as_str)
                .unwrap_or("other");
            let wrapper = item
                .get("wrapper")
                .and_then(toml::Value::as_str)
                .unwrap_or("");
            let wrapper = if wrapper.is_empty() {
                format!("kyth-{name}")
            } else {
                wrapper.to_string()
            };
            Some((
                name.clone(),
                TunableSpec {
                    name: name.clone(),
                    module: module.into(),
                    kind: kind.into(),
                    wrapper,
                },
            ))
        })
        .collect();
    (!parsed.is_empty()).then_some(parsed)
}

pub fn load_registry(path: Option<impl AsRef<Path>>) -> BTreeMap<String, TunableSpec> {
    let candidates: Vec<PathBuf> = if let Some(path) = path {
        vec![path.as_ref().to_path_buf()]
    } else {
        vec![
            PathBuf::from("build_files/config/tunables.toml"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../build_files/config/tunables.toml"),
            PathBuf::from("/ctx/config/tunables.toml"),
            PathBuf::from("/usr/share/kyth/config/tunables.toml"),
        ]
    };
    candidates
        .into_iter()
        .find_map(|candidate| {
            std::fs::read_to_string(candidate)
                .ok()
                .and_then(|raw| parse_registry(&raw))
        })
        .unwrap_or_else(fallback_registry)
}

pub fn get_spec(name: &str, path: Option<impl AsRef<Path>>) -> Option<TunableSpec> {
    let registry = load_registry(path);
    let key = name
        .strip_prefix("kyth-")
        .or_else(|| name.strip_prefix("kyth_"))
        .unwrap_or(name);
    registry.get(key).cloned().or_else(|| {
        registry
            .values()
            .find(|spec| spec.name.replace('-', "_") == key.replace('-', "_"))
            .cloned()
    })
}

pub fn list_tunables(path: Option<impl AsRef<Path>>) -> Vec<TunableSpec> {
    load_registry(path).into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_declarative_registry_and_normalizes_names() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("tunables.toml");
        std::fs::write(&path, "[tunables.\"perf-cpu\"]\nmodule = \"perf_cpu\"\nkind = \"sysctl\"\nwrapper = \"kyth-perf-cpu\"\n[tunables.search]\nmodule = \"search_config\"\nkind = \"other\"\n").unwrap();
        assert_eq!(
            get_spec("kyth_perf_cpu", Some(&path)).unwrap().module,
            "perf_cpu"
        );
        assert_eq!(get_spec("search", Some(&path)).unwrap().kind, "other");
        assert_eq!(list_tunables(Some(&path)).len(), 2);
    }

    #[test]
    fn malformed_registry_falls_back_to_builtins() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("bad.toml");
        std::fs::write(&path, "not = [valid").unwrap();
        assert_eq!(
            get_spec("perf-cpu", Some(&path)).unwrap().module,
            "perf_cpu"
        );
        assert_eq!(get_spec("bore", Some(&path)).unwrap().kind, "sysctl");
        assert_eq!(get_spec("net-tune", Some(&path)).unwrap().kind, "sysctl");
        assert_eq!(get_spec("kyth-ananicy", Some(&path)).unwrap().kind, "other");
        assert!(get_spec("unknown", Some(&path)).is_none());
    }
}
