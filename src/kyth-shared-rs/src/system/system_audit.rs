//! Pure system-audit aggregation and compact rendering.
//!
//! Live perf collectors, snapshot probes, and cache writes remain outside
//! this crate. Callers pass their already-collected values here.

use serde_json::{Map, Value};

pub fn summarize(perf: &Value, snapshots: usize, flatpak_trim: Option<bool>) -> Value {
    let mut output = perf.as_object().cloned().unwrap_or_default();
    output.insert("snapshots".into(), Value::from(snapshots));
    if let Some(enabled) = flatpak_trim {
        output.insert("flatpak_trim".into(), Value::from(enabled));
    }
    // Preserve the Python contract: this field is informational and the
    // current audit always succeeds once a report has been assembled.
    output.insert("pass".into(), Value::Bool(true));
    Value::Object(output)
}

pub fn format_audit(audit: &Value) -> String {
    fn text(audit: &Map<String, Value>, key: &str) -> String {
        audit
            .get(key)
            .map(|value| match value {
                Value::String(text) => text.clone(),
                Value::Null => "None".into(),
                other => other.to_string(),
            })
            .unwrap_or_else(|| "None".into())
    }
    let object = audit.as_object().cloned().unwrap_or_default();
    let perf = text(&object, "systemd_analyze");
    let perf: String = perf.chars().take(60).collect();
    format!(
        "master: {} loader: {} snapshots: {}\nflatpak_trim: {} perf: {}\n",
        text(&object, "master"),
        text(&object, "loader"),
        text(&object, "snapshots"),
        text(&object, "flatpak_trim"),
        perf
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_collected_values_without_collecting_or_writing() {
        let result = summarize(
            &serde_json::json!({"master":"balanced", "loader":"fast"}),
            3,
            Some(true),
        );
        assert_eq!(result["snapshots"], 3);
        assert_eq!(result["flatpak_trim"], true);
        assert_eq!(result["pass"], true);
    }

    #[test]
    fn formats_missing_and_long_values_compactly() {
        let result = format_audit(
            &serde_json::json!({"master":"balanced", "systemd_analyze":"x".repeat(100)}),
        );
        assert!(result.starts_with("master: balanced loader: None snapshots: None\n"));
        assert!(result.contains(&format!("perf: {}", "x".repeat(60))));
        assert!(!result.contains(&format!("perf: {}", "x".repeat(61))));
    }
}
