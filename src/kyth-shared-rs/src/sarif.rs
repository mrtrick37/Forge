//! Deterministic SARIF finding filtering for changed-file CI checks.
//!
//! The caller still owns reading report files and deciding the process exit
//! code. This module only projects report JSON into findings whose paths are
//! in the supplied changed-file set.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SarifFinding {
    pub path: String,
    pub line: u64,
    pub rule: String,
    pub message: String,
}

pub fn changed_file_findings(
    root: impl AsRef<Path>,
    changed_files: &BTreeSet<String>,
    payload: &Value,
    label: &str,
) -> Vec<SarifFinding> {
    let root = normalize_absolute(root.as_ref());
    let mut findings = Vec::new();
    for run in payload
        .get("runs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for result in run
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let suppressed = result
                .get("suppressions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|item| item.get("kind").and_then(Value::as_str) == Some("inSource"));
            if suppressed {
                continue;
            }
            let Some(location) = result
                .get("locations")
                .and_then(Value::as_array)
                .and_then(|locations| locations.first())
            else {
                continue;
            };
            let uri = location
                .get("physicalLocation")
                .and_then(|location| location.get("artifactLocation"))
                .and_then(|location| location.get("uri"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let Some(path) = sarif_path(&root, uri) else {
                continue;
            };
            if !changed_files.contains(&path) {
                continue;
            }
            let physical = location.get("physicalLocation").unwrap_or(&Value::Null);
            findings.push(SarifFinding {
                path,
                line: physical
                    .get("region")
                    .and_then(|region| region.get("startLine"))
                    .and_then(Value::as_u64)
                    .unwrap_or(1),
                rule: result
                    .get("ruleId")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .into(),
                message: result
                    .get("message")
                    .and_then(|message| message.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| {
                        if label.is_empty() {
                            "SARIF finding"
                        } else {
                            label
                        }
                    })
                    .into(),
            });
        }
    }
    findings
}

fn sarif_path(root: &Path, uri: &str) -> Option<String> {
    let (is_file_uri, raw) = uri
        .strip_prefix("file://")
        .map_or((false, uri), |value| (true, value));
    let decoded = percent_decode(raw)?;
    let candidate = PathBuf::from(decoded);
    if is_file_uri || candidate.is_absolute() {
        let resolved = normalize_absolute(&candidate);
        let relative = resolved.strip_prefix(root).ok()?;
        return path_string(relative);
    }
    path_string(&candidate)
}

fn path_string(path: &Path) -> Option<String> {
    let value = path.to_string_lossy().replace('\\', "/");
    (!value.is_empty() && value != ".").then_some(value)
}

fn normalize_absolute(path: &Path) -> PathBuf {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(value) => normalized.push(value),
        }
    }
    normalized
}

fn percent_decode(value: &str) -> Option<String> {
    let mut output = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes.get(index + 1).and_then(|value| hex(*value))?;
            let low = bytes.get(index + 2).and_then(|value| hex(*value))?;
            output.push(char::from((high << 4) | low));
            index += 3;
        } else {
            output.push(char::from(bytes[index]));
            index += 1;
        }
    }
    Some(output)
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_changed_files_and_source_suppressions() {
        let root = PathBuf::from("/repo");
        let changed = BTreeSet::from(["src/main.rs".to_string()]);
        let payload = serde_json::json!({
            "runs": [{
                "results": [
                    {"ruleId":"R1","message":{"text":"bad"},"locations":[{"physicalLocation":{"artifactLocation":{"uri":"src/main.rs"},"region":{"startLine":7}}}]},
                    {"ruleId":"R2","suppressions":[{"kind":"inSource"}],"locations":[{"physicalLocation":{"artifactLocation":{"uri":"src/main.rs"}}}]},
                    {"ruleId":"R3","locations":[{"physicalLocation":{"artifactLocation":{"uri":"src/other.rs"}}}]}
                ]
            }]
        });
        let findings = changed_file_findings(root, &changed, &payload, "CodeQL");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, 7);
        assert_eq!(findings[0].message, "bad");
    }

    #[test]
    fn decodes_file_uri_and_rejects_paths_outside_root() {
        let changed = BTreeSet::from(["src/main file.rs".to_string()]);
        let inside = serde_json::json!({"runs":[{"results":[{"locations":[{"physicalLocation":{"artifactLocation":{"uri":"file:///repo/src/main%20file.rs"}}}]}]}]});
        assert_eq!(
            changed_file_findings("/repo", &changed, &inside, "").len(),
            1
        );
        let outside = serde_json::json!({"runs":[{"results":[{"locations":[{"physicalLocation":{"artifactLocation":{"uri":"file:///other/src/main%20file.rs"}}}]}]}]});
        assert!(changed_file_findings("/repo", &changed, &outside, "x").is_empty());
    }

    #[test]
    fn defaults_missing_line_rule_and_message() {
        let changed = BTreeSet::from(["main.rs".to_string()]);
        let payload = serde_json::json!({"runs":[{"results":[{"locations":[{"physicalLocation":{"artifactLocation":{"uri":"main.rs"}}}]}]}]});
        let findings = changed_file_findings("/repo", &changed, &payload, "Codacy");
        assert_eq!(findings[0].line, 1);
        assert_eq!(findings[0].rule, "unknown");
        assert_eq!(findings[0].message, "Codacy");
    }
}
