//! Typed health reporting shared by diagnostics, CLI, and native UI clients.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HealthResult {
    pub component: String,
    pub severity: String,
    pub evidence: String,
    pub remediation: String,
    pub section: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HealthReport {
    pub schema_version: u32,
    pub generated_at: String,
    pub overall: String,
    pub summary: HealthSummary,
    pub results: Vec<HealthResult>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct HealthSummary {
    pub healthy: usize,
    pub warning: usize,
    pub error: usize,
}

impl HealthReport {
    pub fn create_at(
        generated_at: impl Into<String>,
        results: impl IntoIterator<Item = HealthResult>,
    ) -> Self {
        let results: Vec<_> = results.into_iter().collect();
        let summary = HealthSummary {
            healthy: results
                .iter()
                .filter(|result| result.severity == "healthy")
                .count(),
            warning: results
                .iter()
                .filter(|result| result.severity == "warning")
                .count(),
            error: results
                .iter()
                .filter(|result| result.severity == "error")
                .count(),
        };
        let overall = if summary.error > 0 {
            "error"
        } else if summary.warning > 0 {
            "warning"
        } else {
            "healthy"
        };
        Self {
            schema_version: 1,
            generated_at: generated_at.into(),
            overall: overall.to_string(),
            summary,
            results,
        }
    }

    /// Create a report with an RFC3339 UTC timestamp without spawning `date`.
    pub fn create(results: impl IntoIterator<Item = HealthResult>) -> Self {
        Self::create_at(rfc3339_now(), results)
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self).map(|json| format!("{json}\n"))
    }

    pub fn to_text(&self) -> String {
        let mut lines = vec![format!("KythOS health: {}", self.overall)];
        let mut section: Option<&str> = None;
        for result in &self.results {
            if section != Some(result.section.as_str()) {
                section = Some(result.section.as_str());
                if !result.section.is_empty() {
                    lines.extend([String::new(), format!("== {} ==", result.section)]);
                }
            }
            lines.push(format!(
                "{:<7} {}: {}",
                result.severity.to_uppercase(),
                result.component,
                result.evidence
            ));
            if !result.remediation.is_empty() {
                lines.push(format!("        Fix: {}", result.remediation));
            }
        }
        format!("{}\n", lines.join("\n"))
    }
}

pub fn remediation_for(component: &str) -> &'static str {
    let component = component.to_ascii_lowercase();
    if component.contains("bootc") {
        "Open Hub > This PC > Repair and verify the bootc deployment."
    } else if component.contains("firmware") {
        "Open Hub > This PC > Updates and run the firmware check."
    } else if component.contains("flatpak") {
        "Open Hub > This PC > Repair and retry the default app setup."
    } else if component.contains("secure boot") {
        "Review Secure Boot status in Hub > This PC > Hardware."
    } else if component.contains("vulkan") {
        "Open Hub > This PC > Hardware and review the graphics driver."
    } else if component.contains("pipewire") || component.contains("wireplumber") {
        "Open Hub > This PC > Repair and restart audio."
    } else {
        "Review this check in Hub or include the JSON report with support."
    }
}

pub fn from_check(
    component: impl Into<String>,
    level: &str,
    evidence: impl Into<String>,
    section: impl Into<String>,
) -> HealthResult {
    let severity = match level.to_ascii_uppercase().as_str() {
        "PASS" | "HEALTHY" | "OK" => "healthy",
        "FAIL" | "ERROR" => "error",
        _ => "warning",
    };
    let component = component.into();
    HealthResult {
        remediation: (severity != "healthy")
            .then(|| remediation_for(&component).to_string())
            .unwrap_or_default(),
        component,
        severity: severity.to_string(),
        evidence: evidence.into(),
        section: section.into(),
    }
}

/// Project a read-only smoke report into the support-safe health schema.
pub fn from_smoke_report(
    generated_at: impl Into<String>,
    report: &crate::system::smoke_check::Report,
) -> HealthReport {
    HealthReport::create_at(
        generated_at,
        report.results.iter().map(|row| {
            let level = match row.level {
                crate::system::smoke_check::Level::Pass => "PASS",
                crate::system::smoke_check::Level::Warn => "WARN",
                crate::system::smoke_check::Level::Fail => "FAIL",
            };
            from_check(&row.name, level, &row.detail, &row.section)
        }),
    )
}

fn rfc3339_now() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

// Howard Hinnant's proleptic-Gregorian civil date conversion.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }).div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524
        - day_of_era / 146_096)
        .div_euclid(365);
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2).div_euclid(153);
    let day = day_of_year - (153 * month_part + 2).div_euclid(5) + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_check_levels_and_remediation() {
        let result = from_check("PipeWire", "WARN", "not running", "Audio");
        assert_eq!(result.severity, "warning");
        assert!(result.remediation.contains("restart audio"));
    }

    #[test]
    fn projects_smoke_rows_without_collecting() {
        let mut smoke = crate::system::smoke_check::Report::default();
        smoke.warned("Firmware", "metadata stale", "Updates");
        let report = from_smoke_report("2026-01-01T00:00:00Z", &smoke);
        assert_eq!(report.overall, "warning");
        assert_eq!(report.results[0].section, "Updates");
    }

    #[test]
    fn computes_overall_and_serializes_contract() {
        let report = HealthReport::create_at(
            "2026-08-29T00:00:00Z",
            [
                from_check("bootc", "PASS", "healthy", "Boot"),
                from_check("Vulkan", "WARN", "fallback", "Graphics"),
            ],
        );
        assert_eq!(report.overall, "warning");
        assert_eq!(report.summary.healthy, 1);
        assert!(report.to_json().unwrap().contains("schema_version"));
        assert!(report.to_text().contains("== Graphics =="));
    }

    #[test]
    fn formats_epoch_date_without_external_command() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_000), (2024, 10, 4));
    }
}
