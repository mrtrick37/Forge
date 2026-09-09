//! Pure release-qualification reports and regression gates.
//!
//! This module consumes already-collected checks and VM acceptance logs. It
//! does not run probes, benchmarks, QEMU, or deployment commands.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualificationCheck {
    pub name: String,
    pub status: String,
    pub evidence: String,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default = "default_required")]
    pub required: bool,
}

fn default_category() -> String {
    "system".into()
}
fn default_required() -> bool {
    true
}

impl QualificationCheck {
    pub fn new(
        name: impl Into<String>,
        status: &str,
        evidence: impl Into<String>,
    ) -> Result<Self, String> {
        if !matches!(status, "pass" | "warning" | "fail") {
            return Err(format!("unsupported check status: {status}"));
        }
        Ok(Self {
            name: name.into(),
            status: status.into(),
            evidence: evidence.into(),
            category: default_category(),
            required: true,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualificationMetric {
    pub name: String,
    pub value: f64,
    pub unit: String,
    pub direction: String,
    #[serde(default)]
    pub workload: String,
    #[serde(default)]
    pub source: String,
}

impl QualificationMetric {
    pub fn new(
        name: impl Into<String>,
        value: f64,
        unit: impl Into<String>,
        direction: &str,
        workload: impl Into<String>,
    ) -> Result<Self, String> {
        if !matches!(direction, "higher" | "lower" | "neutral") {
            return Err(format!("unsupported metric direction: {direction}"));
        }
        if !value.is_finite() {
            return Err("metric value must be finite".into());
        }
        Ok(Self {
            name: name.into(),
            value,
            unit: unit.into(),
            direction: direction.into(),
            workload: workload.into(),
            source: String::new(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegressionBudget {
    pub metric: String,
    pub max_regression_percent: f64,
    #[serde(default = "default_workload")]
    pub workload: String,
    #[serde(default = "default_required")]
    pub required: bool,
}

fn default_workload() -> String {
    "*".into()
}

impl RegressionBudget {
    pub fn applies_to(&self, metric: &QualificationMetric) -> bool {
        self.metric == metric.name && (self.workload == "*" || self.workload == metric.workload)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegressionResult {
    pub metric: String,
    pub workload: String,
    pub baseline: f64,
    pub candidate: f64,
    pub regression_percent: f64,
    pub budget_percent: f64,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualificationReport {
    pub generated_at: String,
    pub source: String,
    pub identity: BTreeMap<String, String>,
    pub checks: Vec<QualificationCheck>,
    #[serde(default)]
    pub metrics: Vec<QualificationMetric>,
    #[serde(default)]
    pub regressions: Vec<RegressionResult>,
}

impl QualificationReport {
    pub fn new(
        generated_at: impl Into<String>,
        source: impl Into<String>,
        identity: BTreeMap<String, String>,
        checks: impl IntoIterator<Item = QualificationCheck>,
        metrics: impl IntoIterator<Item = QualificationMetric>,
    ) -> Self {
        Self {
            generated_at: generated_at.into(),
            source: source.into(),
            identity,
            checks: checks.into_iter().collect(),
            metrics: metrics.into_iter().collect(),
            regressions: Vec::new(),
        }
    }

    pub fn overall(&self) -> &'static str {
        if self
            .checks
            .iter()
            .any(|check| check.required && check.status == "fail")
            || self
                .regressions
                .iter()
                .any(|result| result.status == "fail")
        {
            "fail"
        } else if self
            .checks
            .iter()
            .any(|check| matches!(check.status.as_str(), "warning" | "fail"))
        {
            "warning"
        } else {
            "pass"
        }
    }

    pub fn to_value(&self) -> serde_json::Value {
        serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "generated_at": self.generated_at,
            "source": self.source,
            "overall": self.overall(),
            "identity": self.identity,
            "summary": {
                "pass": self.checks.iter().filter(|check| check.status == "pass").count(),
                "warning": self.checks.iter().filter(|check| check.status == "warning").count(),
                "fail": self.checks.iter().filter(|check| check.status == "fail").count(),
            },
            "checks": self.checks,
            "metrics": self.metrics,
            "regressions": self.regressions,
        })
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.to_value()).unwrap_or_else(|_| "{}".into()) + "\n"
    }

    pub fn to_markdown(&self) -> String {
        let mut lines = vec![
            format!(
                "# KythOS qualification: {}",
                self.overall().to_ascii_uppercase()
            ),
            String::new(),
        ];
        lines.extend(
            self.identity
                .iter()
                .map(|(key, value)| format!("- **{key}:** {value}")),
        );
        lines.extend([
            "".into(),
            "## Checks".into(),
            "".into(),
            "| Status | Category | Check | Evidence |".into(),
            "| --- | --- | --- | --- |".into(),
        ]);
        for check in &self.checks {
            lines.push(format!(
                "| {} | {} | {} | {} |",
                check.status,
                check.category,
                check.name,
                check.evidence.replace('|', "\\|").replace('\n', " ")
            ));
        }
        if !self.metrics.is_empty() {
            lines.extend([
                "".into(),
                "## Metrics".into(),
                "".into(),
                "| Metric | Workload | Value | Source |".into(),
                "| --- | --- | --- | --- |".into(),
            ]);
            for metric in &self.metrics {
                lines.push(format!(
                    "| {} | {} | {} {} | {} |",
                    metric.name,
                    if metric.workload.is_empty() {
                        "system"
                    } else {
                        &metric.workload
                    },
                    metric.value,
                    metric.unit,
                    metric.source
                ));
            }
        }
        if !self.regressions.is_empty() {
            lines.extend([
                "".into(),
                "## Regression gate".into(),
                "".into(),
                "| Status | Metric | Workload | Change | Budget |".into(),
                "| --- | --- | --- | ---: | ---: |".into(),
            ]);
            for result in &self.regressions {
                lines.push(format!(
                    "| {} | {} | {} | {:+.2}% | {}% |",
                    result.status,
                    result.metric,
                    if result.workload.is_empty() {
                        "system"
                    } else {
                        &result.workload
                    },
                    result.regression_percent,
                    result.budget_percent
                ));
            }
        }
        lines.join("\n") + "\n"
    }
}

/// Convert already-collected smoke rows into a qualification report. Host
/// identity, telemetry metrics, and live smoke collection remain caller-owned.
pub fn from_smoke_report(
    generated_at: impl Into<String>,
    identity: BTreeMap<String, String>,
    smoke: &crate::system::smoke_check::Report,
) -> QualificationReport {
    let checks = smoke.results.iter().map(|row| QualificationCheck {
        name: row.name.clone(),
        status: match row.level {
            crate::system::smoke_check::Level::Pass => "pass",
            crate::system::smoke_check::Level::Warn => "warning",
            crate::system::smoke_check::Level::Fail => "fail",
        }
        .into(),
        evidence: row.detail.clone(),
        category: if row.section.is_empty() {
            "system".into()
        } else {
            row.section.clone()
        },
        required: true,
    });
    QualificationReport::new(generated_at, "local-smoke", identity, checks, [])
}

fn regression_percent(candidate: f64, baseline: f64, direction: &str) -> f64 {
    let denominator = baseline.abs();
    if denominator == 0.0 {
        if candidate == baseline {
            return 0.0;
        }
        if direction == "higher" {
            return if candidate > baseline { -100.0 } else { 100.0 };
        }
        return if candidate > baseline { 100.0 } else { -100.0 };
    }
    let result = if direction == "higher" {
        (baseline - candidate) / denominator * 100.0
    } else {
        (candidate - baseline) / denominator * 100.0
    };
    (result * 10_000.0).round() / 10_000.0
}

pub fn evaluate_regressions(
    mut candidate: QualificationReport,
    baseline: &QualificationReport,
    budgets: &[RegressionBudget],
) -> QualificationReport {
    let baseline_metrics: BTreeMap<(_, _, _), _> = baseline
        .metrics
        .iter()
        .map(|metric| {
            (
                (
                    metric.name.clone(),
                    metric.workload.clone(),
                    metric.unit.clone(),
                ),
                metric,
            )
        })
        .collect();
    let mut regressions = Vec::new();
    let mut gate_checks = Vec::new();
    for (key, current) in candidate.identity.clone() {
        if matches!(key.as_str(), "cpu" | "gpu" | "machine")
            && baseline
                .identity
                .get(&key)
                .is_some_and(|prior| prior != &current)
        {
            gate_checks.push(QualificationCheck {
                name: format!("matching {key}"),
                status: "fail".into(),
                evidence: format!(
                    "candidate {current:?} != baseline {:?}",
                    baseline.identity[&key]
                ),
                category: "regression".into(),
                required: true,
            });
        }
    }
    for metric in &candidate.metrics {
        let Some(prior) = baseline_metrics.get(&(
            metric.name.clone(),
            metric.workload.clone(),
            metric.unit.clone(),
        )) else {
            continue;
        };
        let Some(budget) = budgets.iter().find(|budget| budget.applies_to(metric)) else {
            continue;
        };
        if metric.direction == "neutral" {
            continue;
        }
        let change = regression_percent(metric.value, prior.value, &metric.direction);
        regressions.push(RegressionResult {
            metric: metric.name.clone(),
            workload: metric.workload.clone(),
            baseline: prior.value,
            candidate: metric.value,
            regression_percent: change,
            budget_percent: budget.max_regression_percent,
            status: if change > budget.max_regression_percent {
                "fail"
            } else {
                "pass"
            }
            .into(),
        });
    }
    for prior in &baseline.metrics {
        if budgets
            .iter()
            .any(|budget| budget.required && budget.applies_to(prior))
            && !candidate.metrics.iter().any(|metric| {
                metric.name == prior.name
                    && metric.workload == prior.workload
                    && metric.unit == prior.unit
            })
        {
            gate_checks.push(QualificationCheck {
                name: format!("required metric {}", prior.name),
                status: "fail".into(),
                evidence: format!(
                    "candidate is missing workload {} ({})",
                    if prior.workload.is_empty() {
                        "system"
                    } else {
                        &prior.workload
                    },
                    prior.unit
                ),
                category: "regression".into(),
                required: true,
            });
        }
    }
    candidate.checks.extend(gate_checks);
    candidate.regressions = regressions;
    candidate
}

pub fn acceptance_report(
    log: &str,
    update_required: bool,
    generated_at: impl Into<String>,
) -> QualificationReport {
    static SENTINEL: OnceLock<Regex> = OnceLock::new();
    let sentinel =
        SENTINEL.get_or_init(|| Regex::new(r"^KYTH_ACCEPTANCE:([A-Z_]+):(.*)$").unwrap());
    let mut events = BTreeMap::new();
    for line in log.lines() {
        if let Some(captures) = sentinel.captures(line.trim()) {
            events.insert(captures[1].to_string(), captures[2].to_string());
        }
    }
    let base = [
        "LIVE_READY",
        "LIVE_SMOKE_OK",
        "INSTALL_COMPLETE",
        "INSTALLED_READY",
        "INSTALLED_SMOKE_OK",
        "COMPLETE",
    ];
    let hub = [
        "HUB_BINARY_OK",
        "HUB_DEEP_LINKS_OK",
        "HUB_SECOND_LAUNCH_OK",
        "HUB_DASHBOARD_DEGRADED_OK",
        "HUB_UPDATES_OK",
        "HUB_PRIVILEGED_FAILURE_OK",
    ];
    let update = [
        "UPDATE_STAGED",
        "UPDATE_BOOTED",
        "UPDATE_SMOKE_OK",
        "ROLLBACK_STAGED",
        "ROLLBACK_BOOTED",
        "ROLLBACK_SMOKE_OK",
    ];
    let phases = base
        .into_iter()
        .chain(hub)
        .chain(update.into_iter().filter(|_| update_required));
    let checks = phases
        .map(|phase| QualificationCheck {
            name: phase.to_ascii_lowercase().replace('_', " "),
            status: if events.contains_key(phase) {
                "pass"
            } else {
                "fail"
            }
            .into(),
            evidence: events
                .get(phase)
                .cloned()
                .unwrap_or_else(|| "acceptance sentinel missing".into()),
            category: "vm-acceptance".into(),
            required: true,
        })
        .chain(events.get("FAILED").map(|failure| QualificationCheck {
            name: "guest failure".into(),
            status: "fail".into(),
            evidence: failure.clone(),
            category: "vm-acceptance".into(),
            required: true,
        }))
        .collect::<Vec<_>>();
    let mut identity = BTreeMap::from([("environment".into(), "qemu".into())]);
    if let Some(value) = events.get("INSTALL_COMPLETE") {
        identity.insert("installed_image".into(), value.clone());
    }
    if let Some(value) = events.get("UPDATE_BOOTED") {
        identity.insert("updated_digest".into(), value.clone());
    }
    QualificationReport::new(generated_at, "vm-acceptance", identity, checks, [])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_smoke_rows_into_local_qualification_checks() {
        let mut smoke = crate::system::smoke_check::Report::default();
        smoke.warned("Firmware", "metadata stale", "Updates");
        let report = from_smoke_report("2026-01-01T00:00:00Z", BTreeMap::new(), &smoke);
        assert_eq!(report.source, "local-smoke");
        assert_eq!(report.checks[0].status, "warning");
        assert_eq!(report.checks[0].category, "Updates");
    }

    fn check(name: &str, status: &str) -> QualificationCheck {
        QualificationCheck {
            name: name.into(),
            status: status.into(),
            evidence: "ok".into(),
            category: "system".into(),
            required: true,
        }
    }

    #[test]
    fn parses_acceptance_sentinels_and_update_phases() {
        let report = acceptance_report(
            "KYTH_ACCEPTANCE:LIVE_READY:fedora\nKYTH_ACCEPTANCE:INSTALL_COMPLETE:sha256:x\n",
            true,
            "now",
        );
        assert_eq!(report.identity["installed_image"], "sha256:x");
        assert_eq!(
            report
                .checks
                .iter()
                .filter(|check| check.status == "fail")
                .count(),
            16
        );
        assert_eq!(report.overall(), "fail");
    }

    #[test]
    fn gates_higher_and_lower_metrics_and_missing_required_data() {
        let candidate = QualificationReport::new(
            "now",
            "test",
            BTreeMap::new(),
            [check("ready", "pass")],
            [QualificationMetric {
                name: "fps".into(),
                value: 90.0,
                unit: "fps".into(),
                direction: "higher".into(),
                workload: "game".into(),
                source: "test".into(),
            }],
        );
        let baseline = QualificationReport::new(
            "old",
            "test",
            BTreeMap::new(),
            [],
            [QualificationMetric {
                name: "fps".into(),
                value: 100.0,
                unit: "fps".into(),
                direction: "higher".into(),
                workload: "game".into(),
                source: "test".into(),
            }],
        );
        let gated = evaluate_regressions(
            candidate,
            &baseline,
            &[RegressionBudget {
                metric: "fps".into(),
                max_regression_percent: 5.0,
                workload: "*".into(),
                required: true,
            }],
        );
        assert_eq!(gated.regressions[0].regression_percent, 10.0);
        assert_eq!(gated.overall(), "fail");
    }

    #[test]
    fn renders_json_and_markdown() {
        let report = QualificationReport::new(
            "now",
            "test",
            BTreeMap::from([("machine".into(), "vm".into())]),
            [check("ready", "pass")],
            [],
        );
        assert!(report.to_json().contains("\"schema_version\": 1"));
        assert!(report
            .to_markdown()
            .starts_with("# KythOS qualification: PASS"));
    }
}
