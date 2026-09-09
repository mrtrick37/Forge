//! Pure command planning for snapshot quota and timeline cleanup.
//!
//! The Python helper performs best-effort Btrfs/Snapper writes. Rust callers
//! can use this plan to present or authorize the same bounded operations while
//! retaining execution and failure handling in the privileged owner.

use std::path::{Path, PathBuf};

pub const TIMELINE_CONFIG: &[&str] = &[
    "TIMELINE_LIMIT_HOURLY=5",
    "TIMELINE_LIMIT_DAILY=7",
    "TIMELINE_LIMIT_MONTHLY=2",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupPlan {
    pub home: PathBuf,
    pub limit_percent: i64,
    pub filesystem_check: Vec<String>,
    pub quota_enable: Vec<String>,
    pub quota_limit: Vec<String>,
    pub snapper_config: Vec<String>,
    pub snapper_cleanup: Vec<String>,
}

pub fn plan(home: impl AsRef<Path>, limit_percent: i64) -> CleanupPlan {
    let home = home.as_ref().to_path_buf();
    CleanupPlan {
        filesystem_check: vec![
            "btrfs".into(),
            "filesystem".into(),
            "show".into(),
            home.display().to_string(),
        ],
        quota_enable: vec![
            "btrfs".into(),
            "quota".into(),
            "enable".into(),
            home.display().to_string(),
        ],
        quota_limit: vec![
            "btrfs".into(),
            "qgroup".into(),
            "limit".into(),
            format!("{limit_percent}%"),
            home.display().to_string(),
        ],
        snapper_config: ["snapper", "-c", "root", "set-config"]
            .into_iter()
            .map(String::from)
            .chain(TIMELINE_CONFIG.iter().map(|value| (*value).into()))
            .collect(),
        snapper_cleanup: vec!["snapper".into(), "cleanup".into(), "timeline".into()],
        home,
        limit_percent,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesystemStatus {
    Btrfs,
    NotBtrfs,
    Unavailable,
}

pub fn filesystem_status(exit_code: Option<i32>) -> FilesystemStatus {
    match exit_code {
        Some(0) => FilesystemStatus::Btrfs,
        Some(_) => FilesystemStatus::NotBtrfs,
        None => FilesystemStatus::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_matches_btrfs_and_snapper_command_order() {
        let value = plan("/home", 120);
        assert_eq!(value.limit_percent, 120);
        assert_eq!(value.filesystem_check[..3], ["btrfs", "filesystem", "show"]);
        assert_eq!(value.quota_limit[3], "120%");
        assert_eq!(
            value.snapper_config.last().unwrap(),
            "TIMELINE_LIMIT_MONTHLY=2"
        );
        assert_eq!(value.snapper_cleanup, ["snapper", "cleanup", "timeline"]);
    }

    #[test]
    fn filesystem_status_fails_closed() {
        assert_eq!(filesystem_status(Some(0)), FilesystemStatus::Btrfs);
        assert_eq!(filesystem_status(Some(1)), FilesystemStatus::NotBtrfs);
        assert_eq!(filesystem_status(None), FilesystemStatus::Unavailable);
    }
}
