//! Pure planning and readback evaluation for live display changes.

use std::time::Duration;

pub const DEBOUNCE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveApplyPlan {
    pub mode: String,
    pub inspect_command: Vec<String>,
    pub timeout: Duration,
}

pub fn plan(mode: impl Into<String>) -> LiveApplyPlan {
    LiveApplyPlan {
        mode: mode.into(),
        inspect_command: vec!["kscreen-doctor".into(), "-o".into()],
        timeout: Duration::from_secs(5),
    }
}

pub fn readback_matches(exit_code: Option<i32>, output: &str, mode: &str) -> bool {
    exit_code == Some(0) && output.contains(mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_uses_bounded_kscreen_readback() {
        let value = plan("1920x1080@60");
        assert_eq!(value.inspect_command, ["kscreen-doctor", "-o"]);
        assert_eq!(value.timeout, Duration::from_secs(5));
        assert_eq!(DEBOUNCE, Duration::from_secs(2));
    }

    #[test]
    fn readback_requires_success_and_requested_mode() {
        assert!(readback_matches(
            Some(0),
            "Output: HDMI mode 1920x1080@60",
            "1920x1080@60"
        ));
        assert!(!readback_matches(Some(1), "1920x1080@60", "1920x1080@60"));
        assert!(!readback_matches(
            Some(0),
            "Output: HDMI mode 1280x720@60",
            "1920x1080@60"
        ));
    }
}
