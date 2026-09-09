//! Pure gaming-session and process-activity projections.
//!
//! The Python service gathers login sessions, GameMode output, and `/proc`
//! entries. Rust callers can use these helpers to interpret that evidence
//! without duplicating the trigger precedence or allowing a generic command
//! bridge.

use std::path::Path;

pub const GAMING_PROCS: &[&str] = &[
    "gamescope",
    "wine",
    "wine64",
    "wineserver",
    "wine-preloader",
    "pressure-vessel-wrap",
];

pub fn active_uids_from_loginctl(output: &str) -> Vec<u32> {
    let mut uids = output
        .lines()
        .filter_map(|line| line.split_whitespace().nth(2))
        .filter_map(|uid| uid.parse::<u32>().ok())
        .collect::<Vec<_>>();
    uids.sort_unstable();
    uids.dedup();
    uids
}

pub fn gamescope_session_path(uid: u32) -> String {
    format!("/run/user/{uid}/gamescope-session.lock")
}

pub fn gamemode_active(command_succeeded: bool, output: &str) -> bool {
    command_succeeded
        && output
            .split_whitespace()
            .last()
            .and_then(|value| value.parse::<i64>().ok())
            .is_some_and(|value| value > 0)
}

pub fn is_gaming_process(executable: &str) -> bool {
    let Some(name) = Path::new(executable)
        .file_name()
        .and_then(|value| value.to_str())
    else {
        return false;
    };
    GAMING_PROCS
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

/// Apply the Python service's gamescope → GameMode → process precedence to
/// already-collected evidence.
pub fn gaming_reason(
    uid: Option<u32>,
    check_all_uids: bool,
    active_session_uids: &[u32],
    gamescope_uids: &[u32],
    gamemode_uids: &[u32],
    process_active: bool,
    current_uid: u32,
) -> Option<String> {
    let mut uids = if check_all_uids {
        active_session_uids.to_vec()
    } else {
        Vec::new()
    };
    if let Some(uid) = uid.filter(|value| !uids.contains(value)) {
        uids.push(uid);
    }
    if uids.is_empty() {
        uids.push(current_uid);
    }
    if let Some(uid) = uids.iter().find(|uid| gamescope_uids.contains(uid)) {
        return Some(format!("gamescope session active (uid {uid})"));
    }
    if let Some(uid) = uids.iter().find(|uid| gamemode_uids.contains(uid)) {
        return Some(format!("GameMode active (uid {uid})"));
    }
    process_active.then(|| "gaming process detected (/proc scan)".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_deduplicates_session_uids() {
        let output = "1 user 1000 seat0\n2 other 1001 seat1\n3 user 1000 seat0\n";
        assert_eq!(active_uids_from_loginctl(output), vec![1000, 1001]);
        assert_eq!(
            gamescope_session_path(1000),
            "/run/user/1000/gamescope-session.lock"
        );
    }

    #[test]
    fn interprets_gamemode_process_names_and_trigger_precedence() {
        assert!(gamemode_active(true, "i 1"));
        assert!(!gamemode_active(false, "i 1"));
        assert!(is_gaming_process("/usr/bin/Wine64"));
        assert!(!is_gaming_process("/usr/bin/konsole"));
        assert_eq!(
            gaming_reason(Some(1000), false, &[], &[1000], &[1000], true, 1000),
            Some("gamescope session active (uid 1000)".into())
        );
        assert_eq!(
            gaming_reason(None, false, &[], &[], &[1000], true, 1000),
            Some("GameMode active (uid 1000)".into())
        );
        assert_eq!(
            gaming_reason(None, false, &[], &[], &[], true, 1000),
            Some("gaming process detected (/proc scan)".into())
        );
    }
}
