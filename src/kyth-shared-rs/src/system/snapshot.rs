//! Read-only snapshot/deployment timeline.
//!
//! Mirrors `kyth_shared.snapshot_timeline`: Snapper is preferred, Btrfs is a
//! filesystem-level fallback, and bootc deployments are appended from the
//! guarded status reader. No snapshot creation, deletion, or rollback is
//! performed here.

use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SnapshotRow {
    pub id: String,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub row_type: String,
    pub description: String,
    pub healthy: Option<bool>,
}

fn run_text(program: &str, args: &[&str], timeout: Duration) -> Option<(bool, String)> {
    let mut argv = vec![program.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    let output = super::process::run_bounded(&argv, timeout).ok()?;
    Some((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
    ))
}

fn value_string(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_string)
}

fn nested_string(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str().map(str::to_string)
}

/// Parse a captured `snapper list --json` response without invoking Snapper.
pub fn parse_snapper_rows(output: &str) -> Vec<SnapshotRow> {
    let Ok(data) = serde_json::from_str::<Value>(output) else {
        return Vec::new();
    };
    data.get("snapshots")
        .and_then(Value::as_array)
        .map(|snapshots| {
            snapshots
                .iter()
                .map(|snapshot| SnapshotRow {
                    id: snapshot
                        .get("number")
                        .map_or_else(String::new, value_string),
                    timestamp: snapshot.get("date").map_or_else(String::new, value_string),
                    row_type: "snapshot".to_string(),
                    description: snapshot
                        .get("description")
                        .map_or_else(String::new, value_string),
                    healthy: None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a captured `btrfs subvolume list` response without touching Btrfs.
pub fn parse_btrfs_rows(output: &str) -> Vec<SnapshotRow> {
    output
        .lines()
        .take(20)
        .map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            SnapshotRow {
                id: fields.get(1).copied().unwrap_or_default().to_string(),
                timestamp: String::new(),
                row_type: "snapshot".to_string(),
                description: line.chars().take(80).collect(),
                healthy: None,
            }
        })
        .collect()
}

/// Parse bootc deployment entries from an already-decoded status document.
pub fn parse_bootc_rows(data: &Value) -> Vec<SnapshotRow> {
    ["booted", "rollback", "staged"]
        .into_iter()
        .filter_map(|section| {
            let deployment = data.get("status")?.get(section)?;
            if deployment.is_null() {
                return None;
            }
            let digest = nested_string(deployment, &["image", "imageDigest"])
                .or_else(|| nested_string(deployment, &["imageDigest"]))
                .unwrap_or_default();
            let id = if digest.is_empty() {
                section.to_string()
            } else {
                digest.chars().take(12).collect()
            };
            Some(SnapshotRow {
                id,
                timestamp: String::new(),
                row_type: if section == "booted" {
                    "deployment"
                } else {
                    "rollback"
                }
                .to_string(),
                description: format!("{section}: {}", digest.chars().take(40).collect::<String>()),
                healthy: None,
            })
        })
        .collect()
}

fn snapper_rows() -> Vec<SnapshotRow> {
    let Some((true, output)) = run_text("snapper", &["list", "--json"], Duration::from_secs(5))
    else {
        return Vec::new();
    };
    parse_snapper_rows(&output)
}

fn btrfs_rows() -> Vec<SnapshotRow> {
    let Some((true, output)) =
        run_text("btrfs", &["subvolume", "list", "/"], Duration::from_secs(5))
    else {
        return Vec::new();
    };
    parse_btrfs_rows(&output)
}

fn bootc_rows() -> Vec<SnapshotRow> {
    let Some(data) = crate::system::bootc_query::fetch_status_data() else {
        return Vec::new();
    };
    parse_bootc_rows(&data)
}

pub fn snapshot_timeline(limit: usize) -> Vec<SnapshotRow> {
    if limit == 0 {
        return Vec::new();
    }
    let mut rows = snapper_rows();
    if rows.is_empty() {
        rows = btrfs_rows();
    }
    rows.extend(bootc_rows());
    let mut seen = HashSet::new();
    rows.into_iter()
        .filter(|row| seen.insert((row.id.clone(), row.row_type.clone())))
        .take(limit)
        .collect()
}

/// Serialize a captured timeline using the same wire shape as the Python
/// `snapshot_timeline_json` helper. This projection is intentionally separate
/// from collection so callers can render supplied rows without running tools.
pub fn snapshot_rows_json(rows: &[SnapshotRow]) -> String {
    serde_json::to_string_pretty(rows).unwrap_or_else(|_| "[]".to_string())
}

pub fn snapshot_timeline_json(limit: usize) -> String {
    snapshot_rows_json(&snapshot_timeline(limit))
}

pub fn snapshot_count() -> usize {
    // Keep the count independent from the presentation limit and avoid
    // querying bootc for a simple Repair-page badge.
    let rows = snapper_rows();
    if !rows.is_empty() {
        rows.len()
    } else {
        btrfs_rows().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nested_bootc_digest_is_read_without_mutation() {
        let data = json!({"image": {"imageDigest": "sha256:1234567890abcdef"}});
        assert_eq!(
            nested_string(&data, &["image", "imageDigest"]),
            Some("sha256:1234567890abcdef".into())
        );
    }

    #[test]
    fn zero_limit_is_empty() {
        assert!(snapshot_timeline(0).is_empty());
    }

    #[test]
    fn row_serializes_type_as_wire_name() {
        let row = SnapshotRow {
            id: "1".into(),
            timestamp: String::new(),
            row_type: "snapshot".into(),
            description: "test".into(),
            healthy: None,
        };
        assert_eq!(serde_json::to_value(row).unwrap()["type"], "snapshot");
    }

    #[test]
    fn serializes_supplied_rows_with_python_wire_keys() {
        let rows = [SnapshotRow {
            id: "7".into(),
            timestamp: "today".into(),
            row_type: "snapshot".into(),
            description: "before update".into(),
            healthy: None,
        }];
        let encoded = snapshot_rows_json(&rows);
        assert!(encoded.contains("\"type\": \"snapshot\""));
        assert!(!encoded.contains("row_type"));
    }

    #[test]
    fn parses_snapper_rows_without_running_snapper() {
        let rows = parse_snapper_rows(
            r#"{"snapshots":[{"number":7,"date":"2026-08-30","description":"before update"}]}"#,
        );
        assert_eq!(
            rows,
            vec![SnapshotRow {
                id: "7".into(),
                timestamp: "2026-08-30".into(),
                row_type: "snapshot".into(),
                description: "before update".into(),
                healthy: None
            }]
        );
        assert!(parse_snapper_rows("not json").is_empty());
    }

    #[test]
    fn parses_btrfs_rows_with_a_bounded_description() {
        let rows = parse_btrfs_rows("ID 42 gen 9 top level 5 path @root\nshort");
        assert_eq!(rows[0].id, "42");
        assert_eq!(rows[1].id, "");
        assert!(rows[0].description.len() <= 80);
    }

    #[test]
    fn parses_bootc_deployments_in_stable_order() {
        let rows = parse_bootc_rows(&json!({"status": {
            "booted": {"image": {"imageDigest": "sha256:booted"}},
            "rollback": {"imageDigest": "sha256:rollback"},
            "staged": null
        }}));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].row_type, "deployment");
        assert_eq!(rows[1].row_type, "rollback");
        assert_eq!(rows[1].id, "sha256:rollb");
    }
}
