//! Offline SBOM and CVE summaries for the Hub supply-chain view.

use serde_json::{Map, Value};
use std::path::Path;

pub const DEFAULT_SBOM_PATH: &str = "/usr/share/kyth/sbom.json";
pub const DEFAULT_CVE_PATH: &str = "/var/cache/kyth/cve/osv.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct SbomDiff {
    pub added: Vec<Value>,
    pub removed: Vec<Value>,
    pub changed: Vec<ChangedArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ChangedArtifact {
    pub name: String,
    pub from: Option<Value>,
    pub to: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct CveSummary {
    pub total: usize,
    pub high: usize,
    pub results: usize,
}

pub fn load_json(path: impl AsRef<Path>, fallback_key: &str) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| {
            let mut object = Map::new();
            object.insert(fallback_key.to_string(), Value::Array(Vec::new()));
            Value::Object(object)
        })
}

pub fn load_sbom() -> Value {
    load_json(DEFAULT_SBOM_PATH, "artifacts")
}

pub fn load_cve() -> Value {
    load_json(DEFAULT_CVE_PATH, "results")
}

fn artifacts(value: &Value) -> Vec<(String, Value)> {
    value
        .get("artifacts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artifact| {
            let object = artifact.as_object()?;
            let name = object.get("name")?.as_str()?.to_string();
            Some((name, artifact.clone()))
        })
        .collect()
}

pub fn sbom_diff(current: &Value, previous: &Value) -> SbomDiff {
    let current = artifacts(current);
    let previous = artifacts(previous);
    let current_map: std::collections::HashMap<_, _> = current.iter().cloned().collect();
    let previous_map: std::collections::HashMap<_, _> = previous.iter().cloned().collect();
    let added = current
        .iter()
        .filter(|(name, _)| !previous_map.contains_key(name))
        .map(|(_, value)| value.clone())
        .collect();
    let removed = previous
        .iter()
        .filter(|(name, _)| !current_map.contains_key(name))
        .map(|(_, value)| value.clone())
        .collect();
    let changed = current
        .iter()
        .filter_map(|(name, value)| {
            let old = previous_map.get(name)?;
            let new_version = value.get("version");
            let old_version = old.get("version");
            (new_version != old_version).then(|| ChangedArtifact {
                name: name.clone(),
                from: old_version.cloned(),
                to: new_version.cloned(),
            })
        })
        .collect();
    SbomDiff {
        added,
        removed,
        changed,
    }
}

pub fn cve_summary(value: &Value) -> CveSummary {
    let results = value
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut summary = CveSummary {
        results: results.len(),
        ..Default::default()
    };
    for result in results {
        for vulnerability in result
            .get("vulnerabilities")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            summary.total += 1;
            if matches!(
                vulnerability
                    .get("severity")
                    .and_then(Value::as_str)
                    .map(|severity| severity.to_ascii_uppercase())
                    .as_deref(),
                Some("HIGH" | "CRITICAL")
            ) {
                summary.high += 1;
            }
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_added_removed_and_changed_artifacts() {
        let old: Value = serde_json::json!({"artifacts":[{"name":"a","version":"1"},{"name":"gone","version":"1"}]});
        let new: Value = serde_json::json!({"artifacts":[{"name":"a","version":"2"},{"name":"new","version":"1"}]});
        let diff = sbom_diff(&new, &old);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.changed[0].name, "a");
    }

    #[test]
    fn counts_high_cves() {
        let value = serde_json::json!({"results":[{"vulnerabilities":[{"severity":"HIGH"},{"severity":"low"}]},{"vulnerabilities":[{"severity":"CRITICAL"}]}]});
        assert_eq!(
            cve_summary(&value),
            CveSummary {
                total: 3,
                high: 2,
                results: 2
            }
        );
    }
}
