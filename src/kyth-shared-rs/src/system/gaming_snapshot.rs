//! Pure planning and result evaluation for the pre-gaming snapshot helper.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotPlan {
    pub description: String,
    pub snapper_command: Vec<String>,
    pub btrfs_command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotResult {
    pub ok: bool,
    pub id: String,
    pub tool: String,
    pub error: Option<String>,
}

pub fn plan(description: impl Into<String>) -> SnapshotPlan {
    let description = description.into();
    SnapshotPlan {
        snapper_command: vec![
            "snapper".into(),
            "create".into(),
            "--description".into(),
            description.clone(),
            "--print-number".into(),
        ],
        btrfs_command: vec![
            "btrfs".into(),
            "subvolume".into(),
            "snapshot".into(),
            "-r".into(),
            "/".into(),
            format!("/.snapshots/pre-gaming-{description}"),
        ],
        description,
    }
}

pub fn evaluate(
    description: &str,
    snapper: Option<(i32, &str)>,
    btrfs_exit: Option<i32>,
) -> SnapshotResult {
    if let Some((exit_code, output)) = snapper {
        if exit_code == 0 {
            return SnapshotResult {
                ok: true,
                id: output.trim().into(),
                tool: "snapper".into(),
                error: None,
            };
        }
    }
    if btrfs_exit == Some(0) {
        return SnapshotResult {
            ok: true,
            id: description.into(),
            tool: "btrfs".into(),
            error: None,
        };
    }
    SnapshotResult {
        ok: false,
        id: String::new(),
        tool: String::new(),
        error: Some("no snapper/btrfs available — snapshot skipped (safe to proceed)".into()),
    }
}

pub fn snapshot_path(description: &str) -> PathBuf {
    PathBuf::from(format!("/.snapshots/pre-gaming-{description}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapper_is_preferred_and_btrfs_is_the_fallback() {
        let value = plan("pre-gaming-master");
        assert_eq!(value.snapper_command[0], "snapper");
        assert_eq!(value.btrfs_command[0], "btrfs");
        assert_eq!(
            snapshot_path("pre-gaming-master"),
            std::path::Path::new("/.snapshots/pre-gaming-pre-gaming-master")
        );
        assert_eq!(evaluate("x", Some((0, "17\n")), Some(1)).id, "17");
        assert_eq!(evaluate("x", Some((1, "")), Some(0)).tool, "btrfs");
    }

    #[test]
    fn unavailable_snapshot_is_non_fatal() {
        let result = evaluate("pre-gaming-master", None, Some(1));
        assert!(!result.ok);
        assert!(result.error.unwrap().contains("safe to proceed"));
    }
}
