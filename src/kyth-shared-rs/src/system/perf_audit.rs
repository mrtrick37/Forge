//! Pure performance-audit formatting.
//!
//! `kyth_shared.perf_audit` still owns live collection and its short-lived
//! cache. This module owns only the stable text projection consumed by the
//! welcome and Hub surfaces.

use serde_json::Value;

pub const AUDIT_KEYS: &[&str] = &[
    "master",
    "loader",
    "oom_gaming",
    "shader_tmpfs",
    "gaming_cfs",
    "thp",
    "irq",
    "btrfs",
    "trim",
    "ananicy",
    "zswap",
    "sched",
    "wine",
    "kwin",
    "pipewire_gaming",
    "vm_watermark",
    "tcp_notsent",
    "max_map_count",
    "dirty_ratio",
    "vfs_cache",
    "tcp_ecn",
    "tcp_slow_start",
    "autogroup",
    "nr_migrate",
    "page_cluster",
    "tcp_retries2",
    "tcp_keepalive",
    "sched_child",
    "vm_stat",
    "numa_balancing",
    "tcp_fastopen",
    "tcp_mtu_probing",
    "dirty_expire",
    "file_max",
    "perf_cpu",
    "swappiness",
    "tcp_fin_timeout",
    "somaxconn",
    "inotify_watches",
    "min_free_kbytes",
    "rmem_max",
    "wmem_max",
    "aio_max",
    "overcommit_memory",
    "netdev_budget",
    "rmem_default",
    "wmem_default",
    "tcp_window_scaling",
    "tcp_sack",
    "tcp_timestamps",
    "busy_read",
    "busy_poll",
    "tcp_no_metrics_save",
    "tcp_retries1",
    "tcp_orphan_retries",
];

fn python_value(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "None".into(),
        Some(Value::String(value)) => value.clone(),
        Some(Value::Bool(value)) => if *value { "True" } else { "False" }.into(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Array(value)) => format!("{:?}", value),
        Some(Value::Object(value)) => format!("{:?}", value),
    }
}

/// Render the exact line-oriented contract used by Python's `format_audit`.
pub fn format_audit(audit: &Value) -> String {
    let object = audit.as_object();
    let mut lines = vec!["# Kyth perf audit — 46-140".to_string()];
    for key in AUDIT_KEYS {
        lines.push(format!(
            "{key}: {}",
            python_value(object.and_then(|object| object.get(*key)))
        ));
    }
    lines.push(format!(
        "systemd-analyze: {}",
        python_value(object.and_then(|object| object.get("systemd_analyze")))
    ));
    format!("{}\n", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_all_audit_buckets_in_python_order() {
        let audit = serde_json::json!({"master":"balanced", "loader":"fast", "systemd_analyze":"systemd 256"});
        let output = format_audit(&audit);
        assert!(output.starts_with("# Kyth perf audit — 46-140\nmaster: balanced\nloader: fast\n"));
        assert!(output.contains("tcp_orphan_retries: None\n"));
        assert!(output.ends_with("systemd-analyze: systemd 256\n"));
    }

    #[test]
    fn renders_missing_and_scalar_values_like_python() {
        let output = format_audit(&serde_json::json!({"offline":true}));
        assert!(output.contains("master: None\n"));
        assert!(output.contains("systemd-analyze: None\n"));
    }
}
