//! Port of `kyth_shared.system.updates_unified` — bootc + flatpak + firmware.

pub fn pending_updates_summary() -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let firmware = crate::system::firmware::check_firmware_updates(20);
    out.insert("firmware".to_string(), firmware.to_string());
    // flatpak
    let (flatpak_count, flatpak_detail) =
        crate::system::update_availability::flatpak_updates_count(false);
    out.insert("flatpak".to_string(), flatpak_count.to_string());
    if !flatpak_detail.is_empty() {
        out.insert("flatpak_detail".to_string(), flatpak_detail);
    }
    // bootc
    let bootc = crate::system::bootc_query::fetch_status_data()
        .map(|data| {
            if crate::system::bootc::deployment_present(&data, "staged") {
                "staged"
            } else {
                "current"
            }
            .to_string()
        })
        .unwrap_or_else(|| "unknown".to_string());
    out.insert("bootc".to_string(), bootc);
    out
}

pub fn rollback_command() -> Vec<String> {
    vec!["bootc".to_string(), "rollback".to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn summary_has_keys() {
        let m = pending_updates_summary();
        assert!(m.contains_key("bootc"));
        assert!(m.contains_key("flatpak"));
        assert!(m.contains_key("firmware"));
    }
    #[test]
    fn rollback() {
        assert_eq!(rollback_command(), vec!["bootc", "rollback"]);
    }
}
