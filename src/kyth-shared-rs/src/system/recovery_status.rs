//! Port of `kyth_shared.system.recovery_status` — staged/rollback/quarantined single view.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryStatus {
    pub has_staged: bool,
    pub has_rollback: bool,
    pub quarantined_digest: String,
    pub quarantine_detail: String,
    pub watcher_staged: bool,
    pub clear_quarantine_cmd: String,
}

pub fn recovery_banner(s: &RecoveryStatus) -> String {
    let key = (
        s.has_staged,
        s.has_rollback,
        !s.quarantined_digest.is_empty(),
    );
    match key {
        (false, false, false) => "up-to-date".to_string(),
        (true, false, false) => "reboot to apply staged".to_string(),
        (true, true, false) => "reboot to apply staged".to_string(),
        (false, true, false) => "rollback available".to_string(),
        (_, _, true) => "quarantined — clear-quarantine retry".to_string(),
    }
}

pub fn get_recovery_status() -> RecoveryStatus {
    // Read via probe/cache + the cross-process watcher snapshot + boot health.
    // The watcher writes this file; this path never mutates it.
    let history = crate::system::deployment_history::deployment_history();
    let watcher_staged = crate::system::update_status::read_update_snapshot(600)
        .map(|snapshot| !snapshot.staged_digest.is_empty())
        .unwrap_or(false);
    let has_staged = history
        .iter()
        .find(|d| d.section == "staged")
        .map(|d| d.available)
        .unwrap_or(false)
        || watcher_staged;
    let has_rollback = history
        .iter()
        .find(|d| d.section == "rollback")
        .map(|d| d.available)
        .unwrap_or(false);
    // Python stores quarantines as a digest-keyed map, not a scalar
    // `quarantined_digest`. Decode that same state through the shared port so
    // the Repair page and boot-health CLI agree on the newest record.
    let state = crate::system::boot_health::read_default_state();
    let (quarantined, detail) = state.newest_quarantine().map_or_else(
        || (String::new(), String::new()),
        |record| {
            (
                record.digest.clone(),
                crate::system::boot_health::quarantine_reason(&state, &record.digest)
                    .unwrap_or_else(|| record.reason.clone()),
            )
        },
    );
    let clear_cmd = if !quarantined.is_empty() {
        format!(
            "sudo kyth-boot-health clear-quarantine --digest {}",
            quarantined
        )
    } else {
        String::new()
    };
    // `watcher_staged` is an evidence source for the aggregate only; expose
    // the same user-facing staged state so it cannot become a third safeguard.
    RecoveryStatus {
        has_staged,
        has_rollback,
        quarantined_digest: quarantined,
        quarantine_detail: detail,
        watcher_staged: has_staged,
        clear_quarantine_cmd: clear_cmd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn banner_staged() {
        let s = RecoveryStatus {
            has_staged: true,
            has_rollback: false,
            quarantined_digest: String::new(),
            quarantine_detail: String::new(),
            watcher_staged: true,
            clear_quarantine_cmd: String::new(),
        };
        assert_eq!(recovery_banner(&s), "reboot to apply staged");
    }
    #[test]
    fn banner_quarantined() {
        let s = RecoveryStatus {
            has_staged: false,
            has_rollback: false,
            quarantined_digest: "abc".to_string(),
            quarantine_detail: String::new(),
            watcher_staged: false,
            clear_quarantine_cmd: String::new(),
        };
        assert_eq!(recovery_banner(&s), "quarantined — clear-quarantine retry");
    }
}
