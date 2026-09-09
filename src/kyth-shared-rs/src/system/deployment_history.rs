//! Port of `kyth_shared.system.deployment_history` — pure timeline.

use serde_json::Value;

fn nested_get<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cur = v;
    for k in path {
        cur = cur.get(*k)?;
    }
    Some(cur)
}

fn walk_strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Object(m) => {
            for val in m.values() {
                walk_strings(val, out);
            }
        }
        Value::Array(arr) => {
            for val in arr {
                walk_strings(val, out);
            }
        }
        _ => {}
    }
}

fn is_ghcr_reference(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    if t.to_lowercase().starts_with("ghcr.io") {
        return true;
    }
    if let Some(slash) = t.find('/') {
        return t[..slash].to_lowercase() == "ghcr.io";
    }
    false
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeploymentInfo {
    pub section: String,
    pub label: String,
    pub available: bool,
    pub reference: Option<String>,
    pub branch: Option<String>,
    pub timestamp: Option<String>,
    pub digest: Option<String>,
    pub short_digest: Option<String>,
    pub status_text: String,
}

pub fn deployment_history() -> Vec<DeploymentInfo> {
    // Read from probe cache like bootc snapshot does
    let data = crate::system::probe::read_section("bootc-status-data").unwrap_or(Value::Null);
    let status_text = crate::system::bootc_query::fetch_status_text();
    let mut history = Vec::new();
    for (section, label) in [
        ("booted", "Current (booted)"),
        ("staged", "Staged (next boot)"),
        ("rollback", "Previous (rollback)"),
    ] {
        let dep = nested_get(&data, &["status", section]);
        if dep.is_none() {
            history.push(DeploymentInfo {
                section: section.to_string(),
                label: label.to_string(),
                available: false,
                reference: None,
                branch: None,
                timestamp: None,
                digest: None,
                short_digest: None,
                status_text: status_text.clone(),
            });
            continue;
        }
        let dep = dep.unwrap();
        // find ref via multiple paths
        let mut reference: Option<String> = None;
        for path in [
            vec!["image", "reference"],
            vec!["image", "image"],
            vec!["image", "image", "reference"],
            vec!["image"],
        ] {
            if let Some(v) = nested_get(dep, &path) {
                if let Some(s) = v.as_str() {
                    if !s.trim().is_empty() {
                        reference = Some(s.trim().to_string());
                        break;
                    }
                }
                if let Some(obj) = v.as_object() {
                    if let Some(s) = obj.get("reference").and_then(|x| x.as_str()) {
                        if !s.trim().is_empty() {
                            reference = Some(s.trim().to_string());
                            break;
                        }
                    }
                    if let Some(s) = obj.get("image").and_then(|x| x.as_str()) {
                        if !s.trim().is_empty() {
                            reference = Some(s.trim().to_string());
                            break;
                        }
                    }
                }
            }
        }
        if reference.is_none() {
            let mut strs = Vec::new();
            walk_strings(dep, &mut strs);
            for s in strs {
                if is_ghcr_reference(&s) {
                    reference = Some(s.trim().to_string());
                    break;
                }
            }
        }
        let branch = reference
            .as_deref()
            .and_then(|r| crate::system::bootc_policy::branch_from_ref(Some(r)));
        // Keep the same raw timestamp fallback as the status JSON. The Python
        // compatibility layer formats it for the Qt page; the web bridge can
        // format it without losing the source value.
        let timestamp = nested_get(dep, &["image", "timestamp"])
            .or_else(|| nested_get(dep, &["timestamp"]))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let digest = nested_get(dep, &["image", "imageDigest"])
            .or_else(|| nested_get(dep, &["imageDigest"]))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let short = digest
            .as_deref()
            .and_then(|d| d.strip_prefix("sha256:"))
            .map(|d| d.chars().take(12).collect::<String>());
        history.push(DeploymentInfo {
            section: section.to_string(),
            label: label.to_string(),
            available: true,
            reference,
            branch,
            timestamp,
            digest: digest.clone(),
            short_digest: short,
            status_text: status_text.clone(),
        });
    }
    history
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_len_three() {
        let h = deployment_history();
        assert_eq!(h.len(), 3);
    }
}
