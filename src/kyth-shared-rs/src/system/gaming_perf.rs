//! Port of `page_gaming_tools_perf.py`'s `_PerfTuningMixin` — overlay/tool
//! install probes, sched-ext status and control, and the per-game
//! launch-option string builder.
//!
//! Not ported: `_build_advanced_kernel_card`'s Fedora/CachyOS kernel
//! switch. It's the same `bootc_action("switch", ...)` capability the Hub's
//! This PC > Kernel section already exposes (`commands::updates::
//! bootc_switch_branch`) — Python just also shows a second copy of it here.
//! Faithful parity means matching what's genuinely a gap, not duplicating a
//! Python-side redundancy that has no counterpart need in the Tauri Hub.

use std::path::Path;
use std::time::Duration;

fn executable_exists(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// `services/gaming/tools.py::_mangohud_installed` checks `shutil.which`
/// (a PATH search); this checks the fixed install path instead, matching
/// how the rest of this crate probes for optional binaries (see
/// `security_container::detect_terminal`'s doc comment for the same
/// deliberate narrowing).
pub fn mangohud_installed() -> bool {
    executable_exists("/usr/bin/mangohud")
}

pub fn gamescope_installed() -> bool {
    executable_exists("/usr/bin/gamescope")
}

/// `services/gaming/tools.py::_vkbasalt_installed` — checks the same four
/// candidate library paths, in the same order.
pub fn vkbasalt_installed() -> bool {
    const CANDIDATES: [&str; 4] = [
        "/usr/lib64/vkbasalt/libvkbasalt.so",
        "/usr/lib/vkbasalt/libvkbasalt.so",
        "/usr/lib64/libvkbasalt.so",
        "/usr/lib/libvkbasalt.so",
    ];
    CANDIDATES.iter().any(|path| Path::new(path).exists())
}

/// `services/gaming/tools.py::scx_scheduler_command`.
pub fn scx_scheduler_command(scheduler: &str) -> Vec<String> {
    if scheduler == "stop" {
        vec!["kyth-scx".to_string(), "stop".to_string()]
    } else {
        vec![
            "kyth-scx".to_string(),
            "set".to_string(),
            scheduler.to_string(),
        ]
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScxStatus {
    pub active: bool,
    pub configured: String,
}

/// `page_gaming.py::_apply_scx_status`'s parse of `kyth-scx status` output.
pub fn parse_scx_status(output: &str) -> ScxStatus {
    let active = output.contains("Service: active");
    let configured = output
        .lines()
        .find_map(|line| line.strip_prefix("Configured scheduler:"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_string();
    ScxStatus { active, configured }
}

/// Bounded read of live sched-ext status — mirrors `_refresh_scx_status`'s
/// `command_stdout(["kyth-scx", "status"], timeout=5)`. `None` on any
/// failure to start/complete the probe, same as the Python worker's
/// `.failed` path (rendered as "sched-ext status unavailable" by the caller).
pub fn scx_status() -> Option<ScxStatus> {
    let output = crate::system::process::run_bounded(
        &["kyth-scx".to_string(), "status".to_string()],
        Duration::from_secs(5),
    )
    .ok()?;
    Some(parse_scx_status(&String::from_utf8_lossy(&output.stdout)))
}

/// The five goals `_build_profile_builder_card`'s combo box offers. Fixed
/// set — the launch-option text is generated server-side from one of
/// these, never built from free-form input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileGoal {
    Quality,
    Hdr,
    Sharp,
    Latency,
    Troubleshoot,
}

impl ProfileGoal {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "quality" => Some(Self::Quality),
            "hdr" => Some(Self::Hdr),
            "sharp" => Some(Self::Sharp),
            "latency" => Some(Self::Latency),
            "troubleshoot" => Some(Self::Troubleshoot),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quality => "quality",
            Self::Hdr => "hdr",
            Self::Sharp => "sharp",
            Self::Latency => "latency",
            Self::Troubleshoot => "troubleshoot",
        }
    }
}

/// `_update_profile_builder`'s `launch_options` dict, ported byte-for-byte
/// (including that "hdr" always forces `KYTH_HDR=1` regardless of the HDR
/// toggle, and "troubleshoot" ignores both `fps` and `hdr` entirely).
pub fn build_profile_launch_option(goal: ProfileGoal, fps: Option<&str>, hdr: bool) -> String {
    let hdr_prefix = if hdr { "KYTH_HDR=1 " } else { "" };
    let fps_arg = fps
        .filter(|value| !value.is_empty())
        .map(|value| format!(" --fps {value}"))
        .unwrap_or_default();
    match goal {
        ProfileGoal::Quality => format!("{hdr_prefix}kyth-gamescope quality{fps_arg} -- %command%"),
        ProfileGoal::Hdr => format!("KYTH_HDR=1 kyth-gamescope hdr{fps_arg} -- %command%"),
        ProfileGoal::Sharp => format!("{hdr_prefix}kyth-gamescope sharp --fsr{fps_arg} -- %command%"),
        ProfileGoal::Latency => format!("{hdr_prefix}game-performance --profile gaming -- kyth-gamescope latency{fps_arg} -- %command%"),
        ProfileGoal::Troubleshoot => "PROTON_LOG=1 PROTON_NO_NTSYNC=1 %command%".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scx_status_reads_active_and_configured_scheduler() {
        let status = parse_scx_status("Service: active\nConfigured scheduler: scx_rusty\n");
        assert_eq!(
            status,
            ScxStatus {
                active: true,
                configured: "scx_rusty".to_string()
            }
        );
    }

    #[test]
    fn scx_status_defaults_when_unparseable() {
        assert_eq!(
            parse_scx_status(""),
            ScxStatus {
                active: false,
                configured: "unknown".to_string()
            }
        );
        assert_eq!(
            parse_scx_status("Service: inactive\n"),
            ScxStatus {
                active: false,
                configured: "unknown".to_string()
            }
        );
    }

    #[test]
    fn stop_uses_the_stop_subcommand_others_use_set() {
        assert_eq!(scx_scheduler_command("stop"), vec!["kyth-scx", "stop"]);
        assert_eq!(
            scx_scheduler_command("rusty"),
            vec!["kyth-scx", "set", "rusty"]
        );
    }

    #[test]
    fn goal_round_trips_through_parse_and_as_str() {
        for goal in [
            ProfileGoal::Quality,
            ProfileGoal::Hdr,
            ProfileGoal::Sharp,
            ProfileGoal::Latency,
            ProfileGoal::Troubleshoot,
        ] {
            assert_eq!(ProfileGoal::parse(goal.as_str()), Some(goal));
        }
        assert_eq!(ProfileGoal::parse("not-a-goal"), None);
    }

    #[test]
    fn quality_uses_hdr_prefix_only_when_toggled() {
        assert_eq!(
            build_profile_launch_option(ProfileGoal::Quality, None, false),
            "kyth-gamescope quality -- %command%"
        );
        assert_eq!(
            build_profile_launch_option(ProfileGoal::Quality, None, true),
            "KYTH_HDR=1 kyth-gamescope quality -- %command%"
        );
    }

    #[test]
    fn hdr_goal_always_forces_the_env_var_regardless_of_the_toggle() {
        assert_eq!(
            build_profile_launch_option(ProfileGoal::Hdr, None, false),
            "KYTH_HDR=1 kyth-gamescope hdr -- %command%"
        );
        assert_eq!(
            build_profile_launch_option(ProfileGoal::Hdr, None, true),
            "KYTH_HDR=1 kyth-gamescope hdr -- %command%"
        );
    }

    #[test]
    fn fps_cap_is_appended_only_when_set() {
        assert_eq!(
            build_profile_launch_option(ProfileGoal::Sharp, Some("144"), false),
            "kyth-gamescope sharp --fsr --fps 144 -- %command%"
        );
        assert_eq!(
            build_profile_launch_option(ProfileGoal::Sharp, Some(""), false),
            "kyth-gamescope sharp --fsr -- %command%"
        );
        assert_eq!(
            build_profile_launch_option(ProfileGoal::Sharp, None, false),
            "kyth-gamescope sharp --fsr -- %command%"
        );
    }

    #[test]
    fn latency_wraps_gamescope_in_game_performance() {
        assert_eq!(
            build_profile_launch_option(ProfileGoal::Latency, Some("120"), true),
            "KYTH_HDR=1 game-performance --profile gaming -- kyth-gamescope latency --fps 120 -- %command%",
        );
    }

    #[test]
    fn troubleshoot_ignores_fps_and_hdr() {
        assert_eq!(
            build_profile_launch_option(ProfileGoal::Troubleshoot, Some("60"), true),
            "PROTON_LOG=1 PROTON_NO_NTSYNC=1 %command%"
        );
    }
}
