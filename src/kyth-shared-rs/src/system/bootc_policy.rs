//! Pure bootc branch, update, cancellation, and presentation policy —
//! faithful port of `kyth_shared.system.bootc_policy` (pure stdlib, no I/O).
//!
//! Keep strings and branches exactly as Python does; this is presentation
//! truth shared by both Hubs, so drifting copy here would desync the UI.
//! Tests mirror `tests/test_kyth_bootc_policy.py` where it exists.

pub const REGISTRY: &str = "ghcr.io/kyth-os/kyth";

pub fn default_phase(mode: &str) -> String {
    match mode {
        "update" => "Pulling OS image from container registry…".to_string(),
        "full-update" => "Running full system update…".to_string(),
        "rollback" => "Staging rollback deployment…".to_string(),
        _ => "Operation in progress…".to_string(),
    }
}

pub fn parse_update_phase(line: &str, mode: &str) -> Option<String> {
    let lo = line.to_lowercase();
    let registry = REGISTRY.to_lowercase();
    if lo.contains("resolved") && (lo.contains("image") || lo.contains(&registry)) {
        return Some("Resolving OS image version…".to_string());
    }
    if lo.contains("fetching") && (lo.contains("manifest") || lo.contains("sha256")) {
        return Some("Fetching image manifest…".to_string());
    }
    if (lo.contains("pulling") || lo.contains("copying") || lo.contains("fetching"))
        && (lo.contains("sha256")
            || lo.contains("blob")
            || lo.contains("layer")
            || lo.contains("ghcr.io")
            || lo.contains("registry"))
    {
        return Some("Downloading image layers…".to_string());
    }
    let rules: &[(&[&str], &str)] = &[
        (
            &["layers already present", "layers needed"],
            "Checking for new image layers…",
        ),
        (&["unpacking", "extracting"], "Unpacking image layers…"),
        (
            &["checking out", "checkout", "importing"],
            "Importing image into system storage…",
        ),
        (
            &["writing manifest", "manifest to image destination"],
            "Storing image manifest…",
        ),
        (&["rpmdb"], "Updating package database in the new image…"),
        (
            &["initramfs", "kernel"],
            "Preparing boot files for the new image…",
        ),
        (&["deploying"], "Deploying new OS image…"),
        (
            &["no update available", "already booted"],
            "Already on the latest image — nothing to download.",
        ),
    ];
    for (needles, msg) in rules {
        if needles.iter().any(|n| lo.contains(n)) {
            return Some(msg.to_string());
        }
    }
    if lo.contains("writing") || lo.contains("composing") || lo.contains("committing") {
        return Some("Writing new OS image to disk…".to_string());
    }
    if lo.contains("staging") || lo.contains("staged") || lo.contains("transaction complete") {
        return Some("Staging new image for next reboot…".to_string());
    }
    if lo.contains("queued") && lo.contains("boot") {
        return Some("Staged — new image ready for next reboot.".to_string());
    }
    if mode == "full-update" && line.starts_with('―') {
        // Python: re.match(r"――\s*[\d:]+\s*-\s*(.+?)\s*――", line)
        // Rust: manual parse of the same shape without regex crate
        let trimmed = line.trim();
        // Find first and last '―'
        if let Some(start) = trimmed.find('―') {
            if let Some(end) = trimmed.rfind('―') {
                if end > start {
                    let middle = &trimmed[start + '―'.len_utf8()..end];
                    // middle like " 12:34 - foo "
                    if let Some(dash) = middle.find('-') {
                        let title = middle[dash + 1..].trim();
                        if !title.is_empty() {
                            return Some(format!("Updating {}…", title));
                        }
                    }
                }
            }
        }
    }
    None
}

pub fn cancel_block_reason(mode: &str, phase: &str) -> String {
    if mode == "rollback" {
        return "Rollback is already staging the previous deployment. Let it finish, then reboot or update again.".to_string();
    }
    let unsafe_set = [
        "Unpacking image layers…",
        "Download complete — processing image layers…",
        "Processing image layers…",
        "Importing image into system storage…",
        "Storing image manifest…",
        "Writing new OS image to disk…",
        "Updating package database in the new image…",
        "Preparing boot files for the new image…",
        "Deploying new OS image…",
        "Staging new image for next reboot…",
        "Staged — new image ready for next reboot.",
    ];
    if unsafe_set.contains(&phase) {
        return "The operation is past the safe cancel point and is writing or staging the new image. Let it finish.".to_string();
    }
    let lower = phase.to_lowercase();
    if lower.contains("writing image to disk") || lower.contains("committing image") {
        return "The operation is writing the new image. Let it finish.".to_string();
    }
    String::new()
}

pub fn branch_from_ref(r: Option<&str>) -> Option<String> {
    let raw = r?.trim();
    if raw.is_empty() {
        return None;
    }
    let base = raw.split('@').next().unwrap_or(raw);
    if base.contains(':') {
        Some(base.rsplit(':').next().unwrap_or("").to_string())
    } else {
        None
    }
}

pub fn branch_display_name(tag: Option<&str>) -> String {
    match tag {
        Some("latest") => "Stable (latest)".to_string(),
        Some("testing") => "Testing".to_string(),
        Some("latest-cachy") => "Stable + CachyOS kernel".to_string(),
        Some("testing-cachy") => "Testing + CachyOS kernel".to_string(),
        Some(other) if !other.is_empty() => other.to_string(),
        _ => "unknown".to_string(),
    }
}

pub fn image_tag_for_channel(channel: &str, flavor: &str) -> String {
    let base = if channel == "testing" {
        "testing"
    } else {
        "latest"
    };
    if flavor == "cachy" {
        format!("{}-cachy", base)
    } else {
        base.to_string()
    }
}

pub fn image_tag_for_kernel(flavor: &str, current_branch: Option<&str>) -> String {
    let channel = if current_branch.unwrap_or("").starts_with("testing") {
        "testing"
    } else {
        "latest"
    };
    image_tag_for_channel(channel, flavor)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchCardView {
    pub object_name: String,
    pub button_text: String,
    pub build_label_text: String,
    pub build_label_visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchesView {
    pub stable: BranchCardView,
    pub testing: BranchCardView,
}

pub fn branches_view(tag: Option<&str>, booted_ts: Option<&str>) -> BranchesView {
    let build_text = booted_ts
        .map(|ts| format!("Running: built {}", ts))
        .unwrap_or_default();
    let has_ts = booted_ts.is_some();
    let mut stable = BranchCardView {
        object_name: "branch-inactive".to_string(),
        button_text: "Switch to Stable".to_string(),
        build_label_text: String::new(),
        build_label_visible: false,
    };
    let mut testing = BranchCardView {
        object_name: "branch-inactive".to_string(),
        button_text: "Switch to Testing".to_string(),
        build_label_text: String::new(),
        build_label_visible: false,
    };
    match tag {
        Some("latest") | Some("latest-cachy") => {
            stable = BranchCardView {
                object_name: "branch-active".to_string(),
                button_text: "On Stable  (current)".to_string(),
                build_label_text: build_text,
                build_label_visible: has_ts,
            };
        }
        Some("testing") | Some("testing-cachy") => {
            testing = BranchCardView {
                object_name: "branch-active".to_string(),
                button_text: "On Testing  (current)".to_string(),
                build_label_text: build_text,
                build_label_visible: has_ts,
            };
        }
        _ => {}
    }
    BranchesView { stable, testing }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateAvailabilityView {
    pub card_style: String,
    pub icon_text: String,
    pub icon_style: String,
    pub title: String,
    pub body: String,
    pub update_btn_visible: bool,
    pub restart_btn_visible: bool,
}

pub fn update_availability_view(
    staged: bool,
    check_state: &str,
    flatpak_count: u32,
    check_ts: &str,
    check_ts_details: &str,
    staged_ts: Option<&str>,
) -> UpdateAvailabilityView {
    let ts_hint = format!("  ·  Checked at {}", check_ts);
    let built = if !check_ts_details.is_empty() {
        format!("  ·  built {}", check_ts_details)
    } else {
        String::new()
    };
    if staged {
        let built_staged = staged_ts
            .map(|t| format!("  ·  built {}", t))
            .unwrap_or_default();
        let apps = if flatpak_count > 0 {
            let noun = if flatpak_count == 1 {
                "update"
            } else {
                "updates"
            };
            format!(
                " Additionally, {} Flatpak {} can be installed.",
                flatpak_count, noun
            )
        } else {
            String::new()
        };
        return UpdateAvailabilityView {
            card_style: "card-accent-ok".to_string(),
            icon_text: "↻".to_string(),
            icon_style: "avail-icon-blue".to_string(),
            title: "Restart required".to_string(),
            body: format!(
                "A new image is staged and waiting{}.{} Restart now or later — your current system stays available as a fallback.{}",
                built_staged, apps, ts_hint
            ),
            update_btn_visible: false,
            restart_btn_visible: true,
        };
    }
    if check_state == "available" {
        let apps = if flatpak_count > 0 {
            let noun = if flatpak_count == 1 {
                "update"
            } else {
                "updates"
            };
            format!(" and {} Flatpak {} are pending", flatpak_count, noun)
        } else {
            String::new()
        };
        return UpdateAvailabilityView {
            card_style: "card-accent-warn".to_string(),
            icon_text: "↓".to_string(),
            icon_style: "avail-icon-warn".to_string(),
            title: "Update available".to_string(),
            body: format!(
                "A new system image is ready{}{}. Run a full update to download and install them.{}",
                built, apps, ts_hint
            ),
            update_btn_visible: true,
            restart_btn_visible: false,
        };
    }
    if flatpak_count > 0 {
        let noun = if flatpak_count == 1 {
            "update is"
        } else {
            "updates are"
        };
        return UpdateAvailabilityView {
            card_style: "card-accent-warn".to_string(),
            icon_text: "↓".to_string(),
            icon_style: "avail-icon-warn".to_string(),
            title: "App updates available".to_string(),
            body: format!(
                "Your system OS is up to date, but {} Flatpak app {} available. Run a full update to install them.{}",
                flatpak_count, noun, ts_hint
            ),
            update_btn_visible: true,
            restart_btn_visible: false,
        };
    }
    if check_state == "uptodate" {
        return UpdateAvailabilityView {
            card_style: "card-accent-ok".to_string(),
            icon_text: "✓".to_string(),
            icon_style: "avail-icon-ok".to_string(),
            title: "Up to date".to_string(),
            body: format!("Running the latest image{}.{}", built, ts_hint),
            update_btn_visible: false,
            restart_btn_visible: false,
        };
    }
    let detail = check_ts_details.trim();
    if detail.to_lowercase().contains("retryable") {
        return UpdateAvailabilityView {
            card_style: "card-accent-warn".to_string(),
            icon_text: "↻".to_string(),
            icon_style: "avail-icon-warn".to_string(),
            title: "Update failed — Retry available".to_string(),
            body: format!(
                "{}{} Click Update Now to retry the download.",
                detail, ts_hint
            ),
            update_btn_visible: true,
            restart_btn_visible: false,
        };
    }
    let body = if !detail.is_empty()
        && detail.to_lowercase()
            != "checking timed out after 45 s. click check now to retry (skopeo/flatpak may be slow offline)."
    {
        format!("{}{}", detail, ts_hint)
    } else {
        format!("Could not reach the update server — check your network connection.{}", ts_hint)
    };
    UpdateAvailabilityView {
        card_style: "card".to_string(),
        icon_text: "⚠".to_string(),
        icon_style: "avail-icon-dim".to_string(),
        title: "Check unavailable".to_string(),
        body,
        update_btn_visible: false,
        restart_btn_visible: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_from_ref_basic() {
        assert_eq!(
            branch_from_ref(Some("ghcr.io/kyth-os/kyth:latest")),
            Some("latest".to_string())
        );
        assert_eq!(
            branch_from_ref(Some("ghcr.io/kyth-os/kyth:testing-cachy@sha256:abc")),
            Some("testing-cachy".to_string())
        );
        assert_eq!(branch_from_ref(Some("  ")), None);
        assert_eq!(branch_from_ref(None), None);
        assert_eq!(branch_from_ref(Some("no-colon")), None);
    }

    #[test]
    fn branch_display() {
        assert_eq!(branch_display_name(Some("latest")), "Stable (latest)");
        assert_eq!(
            branch_display_name(Some("testing-cachy")),
            "Testing + CachyOS kernel"
        );
        assert_eq!(branch_display_name(Some("custom")), "custom");
        assert_eq!(branch_display_name(None), "unknown");
    }

    #[test]
    fn image_tags() {
        assert_eq!(image_tag_for_channel("testing", "cachy"), "testing-cachy");
        assert_eq!(image_tag_for_channel("stable", "fedora"), "latest");
        assert_eq!(
            image_tag_for_kernel("cachy", Some("testing")),
            "testing-cachy"
        );
        assert_eq!(
            image_tag_for_kernel("fedora", Some("latest-cachy")),
            "latest"
        );
    }

    #[test]
    fn availability_staged() {
        let v = update_availability_view(
            true,
            "available",
            2,
            "12:00",
            "2024-01-01",
            Some("2024-01-02"),
        );
        assert_eq!(v.title, "Restart required");
        assert!(v.restart_btn_visible);
    }

    #[test]
    fn availability_uptodate() {
        let v = update_availability_view(false, "uptodate", 0, "12:00", "", None);
        assert_eq!(v.title, "Up to date");
        assert!(!v.update_btn_visible);
    }

    #[test]
    fn parse_phase_download() {
        assert_eq!(
            parse_update_phase("Copying blob sha256:abc", "update"),
            Some("Downloading image layers…".to_string())
        );
        assert_eq!(
            parse_update_phase("Writing manifest", "update"),
            Some("Storing image manifest…".to_string())
        );
        assert_eq!(parse_update_phase("unknown line xyz", "update"), None);
    }
}

/// Normalize a requested update channel to the argument `ujust
/// switch-channel` actually accepts, or `None` if it isn't a channel.
///
/// The recipe's own `case` (system-updates.just) takes `stable|latest|
/// testing` and exits 1 on anything else — `next`/`cachyos`/`cachy` are
/// kernel flavors belonging to `switch-kernel`, not channels, so accepting
/// them here would only stage a call the recipe rejects.
///
/// Pure policy on purpose: the spawn stays in the caller (see MIGRATION.md
/// on why this crate holds no mutating functions). Returning a fixed
/// literal rather than the caller's string is also what makes passing it
/// to a subprocess safe by construction.
pub fn switch_channel_arg(channel: &str) -> Option<&'static str> {
    match channel.trim().to_lowercase().as_str() {
        "stable" | "latest" => Some("stable"),
        "testing" => Some("testing"),
        _ => None,
    }
}

#[cfg(test)]
mod switch_channel_tests {
    use super::switch_channel_arg;

    #[test]
    fn accepts_the_channels_the_recipe_accepts() {
        assert_eq!(switch_channel_arg("stable"), Some("stable"));
        assert_eq!(switch_channel_arg("latest"), Some("stable"));
        assert_eq!(switch_channel_arg("testing"), Some("testing"));
    }

    #[test]
    fn is_forgiving_about_case_and_padding() {
        assert_eq!(switch_channel_arg("  Testing "), Some("testing"));
        assert_eq!(switch_channel_arg("STABLE"), Some("stable"));
    }

    #[test]
    fn rejects_kernel_flavors_which_belong_to_switch_kernel() {
        for flavor in ["next", "cachyos", "cachy", "fedora"] {
            assert_eq!(
                switch_channel_arg(flavor),
                None,
                "{flavor} is not a channel"
            );
        }
    }

    #[test]
    fn rejects_empty_and_injection_shaped_input() {
        for bad in [
            "",
            "   ",
            "testing; rm -rf /",
            "stable && reboot",
            "../../etc",
        ] {
            assert_eq!(switch_channel_arg(bad), None, "{bad:?} must not pass");
        }
    }
}
