//! Deterministic static metrics used by the optimization report.
//!
//! Filesystem traversal and runtime probe execution remain in the build
//! script. These helpers keep metric calculation and report shape reusable.

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct StaticMetrics {
    pub installer_js_max_file_bytes: u64,
    pub probe_collector_count: u64,
    pub system_hub_inline_styles: u64,
    pub system_hub_source_files: u64,
}

pub fn max_file_size(sizes: impl IntoIterator<Item = u64>) -> u64 {
    sizes.into_iter().max().unwrap_or(0)
}

/// Count `ProbeCollector(` entries in the default collector block, matching
/// the build script's deliberately simple source-level metric.
pub fn probe_collector_count(source: &str) -> u64 {
    let Some((_, remainder)) = source.split_once("def default_collectors()") else {
        return 0;
    };
    let body = remainder
        .split_once("def _run_collector")
        .map_or(remainder, |(body, _)| body);
    body.matches("ProbeCollector(").count() as u64
}

pub fn static_metrics(
    installer_js_sizes: impl IntoIterator<Item = u64>,
    probe_source: &str,
    inline_style_count: u64,
    source_file_count: u64,
) -> StaticMetrics {
    StaticMetrics {
        installer_js_max_file_bytes: max_file_size(installer_js_sizes),
        probe_collector_count: probe_collector_count(probe_source),
        system_hub_inline_styles: inline_style_count,
        system_hub_source_files: source_file_count,
    }
}

/// Return the optimization-budget failures in the same stable text shape as
/// `optimization-report.py --check`.
pub fn budget_failures(static_metrics: &StaticMetrics, budgets: &Value) -> Vec<String> {
    let values = serde_json::to_value(static_metrics).unwrap_or_default();
    budgets
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, limit)| {
            let actual = values.get(name).and_then(Value::as_u64)?;
            let limit = limit.as_u64()?;
            (actual > limit).then(|| format!("{name}: {actual} exceeds budget {limit}"))
        })
        .collect()
}

/// Assemble the complete optimization report, optionally including already
/// collected runtime metrics. Measurement, filesystem traversal, and report
/// writes remain outside this pure projection.
pub fn report_with_runtime(
    source_revision: &str,
    static_metrics: &StaticMetrics,
    budgets: &Value,
    artifacts: &Value,
    runtime: Option<&Value>,
) -> Value {
    let mut report = serde_json::Map::from_iter([
        ("schema_version".into(), Value::from(1)),
        (
            "source_revision".into(),
            Value::String(source_revision.into()),
        ),
        (
            "static".into(),
            serde_json::to_value(static_metrics).unwrap_or_default(),
        ),
        ("budgets".into(), budgets.clone()),
        ("artifacts".into(), artifacts.clone()),
    ]);
    if let Some(runtime) = runtime {
        report.insert("runtime".into(), runtime.clone());
    }
    Value::Object(report)
}

pub fn report(
    source_revision: &str,
    static_metrics: &StaticMetrics,
    budgets: &Value,
    artifacts: &Value,
) -> Value {
    report_with_runtime(source_revision, static_metrics, budgets, artifacts, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn counts_only_the_default_collector_block() {
        let source = "def default_collectors():\n  ProbeCollector(a)\n  ProbeCollector(b)\ndef _run_collector(x):\n  ProbeCollector(c)\n";
        assert_eq!(probe_collector_count(source), 2);
        assert_eq!(probe_collector_count("no collector function"), 0);
    }

    #[test]
    fn static_report_has_optimization_contract_shape() {
        let metrics = static_metrics(
            [4, 9, 2],
            "def default_collectors(): ProbeCollector(x)",
            3,
            8,
        );
        assert_eq!(metrics.installer_js_max_file_bytes, 9);
        let value = report("local", &metrics, &json!({"x": 10}), &json!({}));
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["static"]["probe_collector_count"], 1);
        assert_eq!(value["source_revision"], "local");
    }

    #[test]
    fn projects_budget_failures_and_optional_runtime_metrics() {
        let metrics = static_metrics(
            [4, 9, 2],
            "def default_collectors(): ProbeCollector(x)",
            3,
            8,
        );
        let budgets =
            json!({"installer_js_max_file_bytes": 8, "probe_collector_count": 1, "unknown": 0});
        assert_eq!(
            budget_failures(&metrics, &budgets),
            vec!["installer_js_max_file_bytes: 9 exceeds budget 8"]
        );
        let report = report_with_runtime(
            "local",
            &metrics,
            &budgets,
            &json!({}),
            Some(&json!({"probe_duration_ms": 12.5})),
        );
        assert_eq!(report["runtime"]["probe_duration_ms"], 12.5);
        assert_eq!(report["static"]["system_hub_source_files"], 8);
    }
}
