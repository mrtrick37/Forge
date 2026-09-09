//! Port of `kyth_shared.system.bootc` — thin cache wrappers around bootc_query/policy.

pub fn branch_from_ref(r: Option<&str>) -> Option<String> {
    crate::system::bootc_policy::branch_from_ref(r)
}

pub fn current_branch() -> Option<String> {
    // Prefer the probe cache, then mirror Python's complete image-reference
    // fallback chain (structured status, text status, rpm-ostree).
    if let Some(branch) = crate::system::probe::read_section("bootc-branch")
        .and_then(|v| v.as_str().map(str::to_string))
    {
        return Some(branch);
    }
    crate::system::bootc_query::image_reference()
        .as_deref()
        .and_then(|reference| branch_from_ref(Some(reference)))
}

pub fn current_kernel_flavor() -> String {
    if let Ok(s) = std::fs::read_to_string("/usr/share/kyth/kernel-flavor") {
        let f = s.trim().to_lowercase();
        if f == "cachy" || f == "fedora" {
            return f;
        }
    }
    // fallback uname -r check
    if let Some((_, stdout)) = run_with_timeout(
        &["uname".to_string(), "-r".to_string()],
        std::time::Duration::from_secs(2),
    ) {
        if stdout.to_lowercase().contains("cachy") {
            return "cachy".to_string();
        }
    }
    "fedora".to_string()
}

fn run_with_timeout(cmd: &[String], timeout: std::time::Duration) -> Option<(i32, String)> {
    if cmd.is_empty() {
        return None;
    }
    let output = super::process::run_bounded(cmd, timeout).ok()?;
    Some((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
    ))
}

pub fn has_staged_update() -> bool {
    crate::system::probe::read_section("bootc-status-data")
        .or_else(crate::system::bootc_query::fetch_status_data)
        .is_some_and(|v| deployment_present(&v, "staged"))
}

pub fn has_rollback_deployment() -> bool {
    crate::system::probe::read_section("bootc-status-data")
        .or_else(crate::system::bootc_query::fetch_status_data)
        .is_some_and(|v| deployment_present(&v, "rollback"))
}

/// bootc emits the deployment keys with `null` when no deployment exists.
/// Testing only for key presence turns an ordinary up-to-date system into a
/// permanently staged/rollback-ready one.
pub fn deployment_present(data: &serde_json::Value, section: &str) -> bool {
    data.get("status")
        .and_then(|status| status.get(section))
        .is_some_and(|deployment| !deployment.is_null())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn branch() {
        assert_eq!(
            branch_from_ref(Some("ghcr.io/kyth-os/kyth:latest")),
            Some("latest".to_string())
        );
    }
    #[test]
    fn staged_bool() {
        let _ = has_staged_update();
    }

    #[test]
    fn null_deployments_are_not_reported_as_available() {
        let status = serde_json::json!({"status": {"staged": null, "rollback": null}});
        assert!(!deployment_present(&status, "staged"));
        assert!(!deployment_present(&status, "rollback"));
        let deployment = serde_json::json!({"status": {"staged": {"image": {}}}});
        assert!(deployment_present(&deployment, "staged"));
    }
}
