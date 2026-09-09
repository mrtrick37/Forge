//! Typed diagnostic results and status aggregation.
//!
//! The actual probes and desktop notifications remain caller-owned. This
//! module ports the stable result bookkeeping from `kyth_shared.diagnostics`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum DiagnosticLevel {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticResult {
    pub level: DiagnosticLevel,
    pub check_name: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticReport {
    pub title: String,
    pub warnings: usize,
    pub failures: usize,
    pub results: Vec<DiagnosticResult>,
}

impl DiagnosticReport {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            warnings: 0,
            failures: 0,
            results: Vec::new(),
        }
    }

    pub fn record(
        &mut self,
        level: DiagnosticLevel,
        check_name: impl Into<String>,
        message: impl Into<String>,
    ) {
        match level {
            DiagnosticLevel::Pass => {}
            DiagnosticLevel::Warn => self.warnings += 1,
            DiagnosticLevel::Fail => self.failures += 1,
        }
        self.results.push(DiagnosticResult {
            level,
            check_name: check_name.into(),
            message: message.into(),
        });
    }

    pub fn passed(&mut self, check_name: impl Into<String>, message: impl Into<String>) {
        self.record(DiagnosticLevel::Pass, check_name, message);
    }
    pub fn warned(&mut self, check_name: impl Into<String>, message: impl Into<String>) {
        self.record(DiagnosticLevel::Warn, check_name, message);
    }
    pub fn failed(&mut self, check_name: impl Into<String>, message: impl Into<String>) {
        self.record(DiagnosticLevel::Fail, check_name, message);
    }

    pub fn exit_code(&self) -> i32 {
        if self.failures > 0 {
            2
        } else if self.warnings > 0 {
            1
        } else {
            0
        }
    }

    pub fn status_message(&self, target_name: &str, warning_message: Option<&str>) -> String {
        if self.failures > 0 {
            format!("Result: {target_name} has failures.")
        } else if self.warnings > 0 {
            format!(
                "Result: {}",
                warning_message.unwrap_or(&format!("{target_name} has warnings."))
            )
        } else {
            format!("Result: {target_name} looks good.")
        }
    }

    pub fn render(&self) -> String {
        self.results
            .iter()
            .map(|result| {
                format!(
                    "{:<5} {:<28} {}",
                    level_text(result.level),
                    result.check_name,
                    result.message
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn level_text(level: DiagnosticLevel) -> &'static str {
    match level {
        DiagnosticLevel::Pass => "PASS",
        DiagnosticLevel::Warn => "WARN",
        DiagnosticLevel::Fail => "FAIL",
    }
}

/// Build a prefilled GitHub issue URL from already-scrubbed report text.
///
/// This function only projects query data. Callers must scrub diagnostics and
/// allowlist the repository before opening the returned URL in a browser.
pub fn github_issue_url(repo_url: &str, title: &str, body: &str, label: Option<&str>) -> String {
    let body = if body.chars().count() > 5500 {
        let clipped: String = body.chars().take(5500).collect();
        format!("{clipped}\n\n[Report body truncated for the browser URL. A full local draft was saved by kyth-report-issue.]")
    } else {
        body.to_string()
    };
    let mut url = format!(
        "{}/issues/new?title={}&body={}",
        repo_url.trim_end_matches('/'),
        crate::url_encode::percent_encode(title),
        crate::url_encode::percent_encode(&body),
    );
    if let Some(label) = label.filter(|label| !label.is_empty()) {
        url.push_str("&labels=");
        url.push_str(&crate::url_encode::percent_encode(label));
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_warning_and_failure_exit_status() {
        let mut report = DiagnosticReport::new("Subsystem Health");
        report.passed("Audio", "PipeWire active");
        report.warned("Vulkan", "fallback");
        assert_eq!(report.exit_code(), 1);
        assert_eq!(
            report.status_message("KythOS", None),
            "Result: KythOS has warnings."
        );
        report.failed("GPU", "missing");
        assert_eq!(report.exit_code(), 2);
        assert_eq!(
            report.status_message("KythOS", None),
            "Result: KythOS has failures."
        );
    }

    #[test]
    fn renders_stable_human_readable_rows_and_serializes_levels() {
        let mut report = DiagnosticReport::new("Health");
        report.passed("Audio", "ready");
        assert_eq!(report.render(), "PASS  Audio                        ready");
        assert!(serde_json::to_string(&report).unwrap().contains("PASS"));
    }

    #[test]
    fn builds_encoded_issue_url_and_bounds_browser_body() {
        let url = github_issue_url(
            "https://github.com/kyth-os/kyth/",
            "A & bug",
            &"x".repeat(5501),
            Some("bug report"),
        );
        assert!(
            url.starts_with("https://github.com/kyth-os/kyth/issues/new?title=A%20%26%20bug&body=")
        );
        assert!(url.contains("labels=bug%20report"));
        assert!(url.contains("Report%20body%20truncated"));
    }
}
