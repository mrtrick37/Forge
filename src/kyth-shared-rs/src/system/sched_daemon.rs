//! Automatic sched-ext profile switcher core.
//!
//! Ports the `kyth-sched` launcher: profile config, gaming evidence
//! gathering (over `gaming_activity` projections with the 60s cache),
//! scheduler control via kyth-scx, the Hub status file, and the poll
//! state machine. Signal handling and the sleep loop stay with the
//! launcher binary. `gaming.py` and `daemon.py` stay as fixtures.

use super::gaming_activity::{active_uids_from_loginctl, gaming_reason, is_gaming_process};
use crate::config_loader::load_toml_config;
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub const CONFIG_FILENAME: &str = "sched-profiles.toml";
pub const CONFIG_SECTION: &str = "scheduler";
pub const STATUS_FILENAME: &str = "kyth-sched-status.json";
pub const GAMING_CACHE_TTL: f64 = 60.0;

#[derive(Debug, Clone, PartialEq)]
pub struct SchedConfig {
    pub desktop_scheduler: String,
    pub gaming_scheduler: String,
    pub poll_interval: f64,
    pub integrate_perf_mode: bool,
}

impl Default for SchedConfig {
    fn default() -> Self {
        Self {
            desktop_scheduler: "default".to_string(),
            gaming_scheduler: "scx_rusty".to_string(),
            poll_interval: 5.0,
            integrate_perf_mode: true,
        }
    }
}

fn config_string(config: &BTreeMap<String, Json>, key: &str, fallback: &str) -> String {
    config
        .get(key)
        .and_then(|value| match value {
            Json::String(text) => Some(text.clone()),
            Json::Number(number) => Some(number.to_string()),
            Json::Bool(flag) => Some(flag.to_string()),
            _ => None,
        })
        .unwrap_or_else(|| fallback.to_string())
}

fn config_float(config: &BTreeMap<String, Json>, key: &str, fallback: f64) -> f64 {
    config
        .get(key)
        .and_then(|value| match value {
            Json::Number(number) => number.as_f64(),
            Json::String(text) => text.trim().parse::<f64>().ok(),
            _ => None,
        })
        .unwrap_or(fallback)
}

/// Load and merge the profile config: user file, system fallback,
/// defaults. Mirrors `BaseDaemon.load_config` for this daemon.
pub fn load_sched_config() -> SchedConfig {
    let defaults = BTreeMap::from([
        ("desktop_scheduler".to_string(), json!("default")),
        ("gaming_scheduler".to_string(), json!("scx_rusty")),
        ("poll_interval".to_string(), json!(5)),
        ("integrate_perf_mode".to_string(), json!(true)),
    ]);
    let merged = load_toml_config(CONFIG_FILENAME, &defaults, Some(CONFIG_SECTION), &[]);
    SchedConfig {
        desktop_scheduler: config_string(&merged, "desktop_scheduler", "default"),
        gaming_scheduler: config_string(&merged, "gaming_scheduler", "scx_rusty"),
        poll_interval: config_float(&merged, "poll_interval", 5.0),
        integrate_perf_mode: merged
            .get("integrate_perf_mode")
            .is_some_and(|value| match value {
                Json::Bool(true) => true,
                Json::Bool(false) => false,
                Json::Number(number) => number.as_f64().is_some_and(|float| float != 0.0),
                Json::String(text) => !text.trim().is_empty(),
                _ => false,
            }),
    }
}

/// UIDs with active systemd sessions; command failure yields none.
pub fn session_uids(run: &dyn Fn(&[String], u64) -> Option<(i32, String)>) -> Vec<u32> {
    match run(
        &[
            "loginctl".to_string(),
            "list-sessions".to_string(),
            "--no-legend".to_string(),
            "--no-pager".to_string(),
        ],
        5,
    ) {
        Some((0, stdout)) => active_uids_from_loginctl(&stdout),
        _ => Vec::new(),
    }
}

/// Gamescope session lock for one user.
pub fn gamescope_active(uid: u32) -> bool {
    Path::new(&format!("/run/user/{uid}/gamescope-session.lock")).exists()
}

/// Scan a `/proc`-shaped tree for known gaming executables.
pub fn proc_gaming_active_in(proc_root: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(proc_root) else {
        return false;
    };
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .chars()
            .all(|char| char.is_ascii_digit())
        {
            continue;
        }
        let link = entry.path().join("exe");
        if std::fs::read_link(&link)
            .is_ok_and(|target| is_gaming_process(&target.to_string_lossy()))
        {
            return true;
        }
    }
    false
}

pub fn proc_gaming_active() -> bool {
    proc_gaming_active_in(Path::new("/proc"))
}

/// Cached gaming verdict, mirroring the 60s `_GAMING_CACHE`.
pub struct GamingCache {
    entries: HashMap<(Option<u32>, bool), (f64, Option<String>)>,
}

impl GamingCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn check(
        &mut self,
        now: f64,
        uid: Option<u32>,
        check_all_uids: bool,
        session_uids: &[u32],
        gamescope_uids: &[u32],
        gamemode_uids: &[u32],
        proc_active: bool,
        current_uid: u32,
    ) -> Option<String> {
        let key = (uid, check_all_uids);
        if let Some((stamp, value)) = self.entries.get(&key) {
            if now - stamp < GAMING_CACHE_TTL {
                return value.clone();
            }
        }
        let verdict = gaming_reason(
            uid,
            check_all_uids,
            session_uids,
            gamescope_uids,
            gamemode_uids,
            proc_active,
            current_uid,
        );
        self.entries.insert(key, (now, verdict.clone()));
        verdict
    }
}

impl Default for GamingCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Run one argv, returning success and stripped stdout; failures yield
/// `(false, "")` like the launcher helper.
pub fn run_text(
    run: &dyn Fn(&[String], u64) -> Option<(i32, String)>,
    argv: &[&str],
    timeout_secs: u64,
) -> (bool, String) {
    let argv: Vec<String> = argv.iter().map(|part| (*part).to_string()).collect();
    match run(&argv, timeout_secs) {
        Some((0, stdout)) => (true, stdout.trim().to_string()),
        _ => (false, String::new()),
    }
}

/// Read the active scheduler from sysfs, else kyth-scx status output.
pub fn current_scheduler(
    sysfs: impl Fn() -> Option<String>,
    run: &dyn Fn(&[String], u64) -> Option<(i32, String)>,
) -> String {
    if let Some(state) = sysfs() {
        return state;
    }
    let (ok, out) = run_text(run, &["kyth-scx", "status"], 5);
    if ok {
        for line in out.lines() {
            if line.starts_with("Configured scheduler:") {
                let scheduler = line.split(':').last().unwrap_or("").trim();
                if !scheduler.is_empty() {
                    return scheduler.to_string();
                }
            }
        }
    }
    "unknown".to_string()
}

/// Switch the scheduler via kyth-scx. Returns success.
pub fn set_scheduler(run: &dyn Fn(&[String], u64) -> Option<(i32, String)>, name: &str) -> bool {
    if name == "default" {
        run_text(run, &["sudo", "-n", "/usr/bin/kyth-scx", "stop"], 5).0
    } else {
        run_text(run, &["sudo", "-n", "/usr/bin/kyth-scx", "set", name], 5).0
    }
}

/// Schedulers kyth-scx lists, else `scx_*` binaries minus the loader.
pub fn available_schedulers(
    run: &dyn Fn(&[String], u64) -> Option<(i32, String)>,
    usr_bin: &Path,
) -> Vec<String> {
    let (ok, out) = run_text(run, &["kyth-scx", "list"], 5);
    if ok && !out.is_empty() {
        return out
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();
    }
    let Ok(entries) = std::fs::read_dir(usr_bin) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("scx_") && name != "scx_loader")
        .filter(|name| usr_bin.join(name).is_file())
        .collect();
    names.sort();
    names
}

/// Hub status file location.
pub fn status_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(STATUS_FILENAME)
}

/// Write the Hub status document, swallowing errors like the launcher.
pub fn write_status(
    runtime_dir: &Path,
    profile: &str,
    scheduler: &str,
    gaming: bool,
    manual_override: bool,
    now_secs: u64,
) {
    let path = status_path(runtime_dir);
    if path
        .parent()
        .is_some_and(|parent| std::fs::create_dir_all(parent).is_err())
    {
        return;
    }
    let document = json!({
        "profile": profile,
        "scheduler": scheduler,
        "gaming_active": gaming,
        "manual_override": manual_override,
        "ts": now_secs,
    });
    let _ = std::fs::write(&path, serde_json::to_string(&document).unwrap_or_default());
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedEffect {
    SetScheduler(String),
    EnterGamingPerf,
    RestorePerf,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SchedState {
    pub profile: String,
    pub gaming_prev: bool,
    pub manual_override: Option<String>,
}

impl SchedState {
    pub fn new() -> Self {
        Self {
            profile: "desktop".to_string(),
            gaming_prev: false,
            manual_override: None,
        }
    }
}

/// One poll iteration: returns the effects the launcher must apply.
/// Status reporting stays with the caller, as in `poll()`.
pub fn poll_step(
    state: &mut SchedState,
    config: &SchedConfig,
    gaming_detected: bool,
) -> Vec<SchedEffect> {
    let effective = state.manual_override.clone().unwrap_or_else(|| {
        if gaming_detected {
            "gaming".to_string()
        } else {
            "desktop".to_string()
        }
    });
    let gaming_now = effective == "gaming";
    if gaming_now == state.gaming_prev {
        return Vec::new();
    }
    state.gaming_prev = gaming_now;
    if gaming_now {
        state.profile = "gaming".to_string();
        let mut effects = vec![SchedEffect::SetScheduler(config.gaming_scheduler.clone())];
        if config.integrate_perf_mode {
            effects.push(SchedEffect::EnterGamingPerf);
        }
        effects
    } else {
        state.profile = "desktop".to_string();
        let mut effects = vec![SchedEffect::SetScheduler(config.desktop_scheduler.clone())];
        if config.integrate_perf_mode {
            effects.push(SchedEffect::RestorePerf);
        }
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_without_files() {
        let config = SchedConfig::default();
        assert_eq!(config.desktop_scheduler, "default");
        assert_eq!(config.gaming_scheduler, "scx_rusty");
        assert_eq!(config.poll_interval, 5.0);
        assert!(config.integrate_perf_mode);
    }

    #[test]
    fn scans_fake_proc_tree_for_gaming_executables() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("4242")).unwrap();
        std::os::unix::fs::symlink("/usr/bin/wine64", root.path().join("4242/exe")).unwrap();
        std::fs::create_dir_all(root.path().join("99")).unwrap();
        std::os::unix::fs::symlink("/usr/bin/konsole", root.path().join("99/exe")).unwrap();
        assert!(proc_gaming_active_in(root.path()));
        std::fs::remove_file(root.path().join("4242/exe")).unwrap();
        assert!(!proc_gaming_active_in(root.path()));
        assert!(!proc_gaming_active_in(&root.path().join("missing")));
    }

    #[test]
    fn gaming_cache_honors_ttl_and_precedence() {
        let mut cache = GamingCache::new();
        let first = cache.check(0.0, Some(1000), false, &[], &[1000], &[], false, 1000);
        assert_eq!(
            first.as_deref(),
            Some("gamescope session active (uid 1000)")
        );
        let cached = cache.check(30.0, Some(1000), false, &[], &[], &[], false, 1000);
        assert_eq!(cached, first);
        let refreshed = cache.check(61.0, Some(1000), false, &[], &[], &[], false, 1000);
        assert_eq!(refreshed, None);
        let gaming = cache.check(122.0, Some(1000), false, &[], &[], &[], true, 1000);
        assert_eq!(
            gaming.as_deref(),
            Some("gaming process detected (/proc scan)")
        );
    }

    #[test]
    fn scheduler_reads_sysfs_then_scx_then_unknown() {
        let run = |_: &[String], _: u64| None;
        assert_eq!(
            current_scheduler(|| Some("scx_rusty".to_string()), &run),
            "scx_rusty"
        );
        let run =
            |_: &[String], _: u64| Some((0, "noise\nConfigured scheduler:   bore  \n".to_string()));
        assert_eq!(current_scheduler(|| None, &run), "bore");
        let run = |_: &[String], _: u64| None;
        assert_eq!(current_scheduler(|| None, &run), "unknown");
    }

    #[test]
    fn poll_steps_switch_profiles_with_effects() {
        let config = SchedConfig::default();
        let mut state = SchedState::new();
        assert!(poll_step(&mut state, &config, false).is_empty());
        assert_eq!(state.profile, "desktop");
        let effects = poll_step(&mut state, &config, true);
        assert_eq!(
            effects,
            vec![
                SchedEffect::SetScheduler("scx_rusty".to_string()),
                SchedEffect::EnterGamingPerf,
            ]
        );
        assert_eq!(state.profile, "gaming");
        assert!(poll_step(&mut state, &config, true).is_empty());
        let effects = poll_step(&mut state, &config, false);
        assert_eq!(
            effects,
            vec![
                SchedEffect::SetScheduler("default".to_string()),
                SchedEffect::RestorePerf,
            ]
        );
        let mut state = SchedState {
            profile: "desktop".to_string(),
            gaming_prev: false,
            manual_override: Some("gaming".to_string()),
        };
        let effects = poll_step(&mut state, &config, false);
        assert_eq!(
            effects[0],
            SchedEffect::SetScheduler("scx_rusty".to_string())
        );
    }

    #[test]
    fn writes_hub_status_document() {
        let dir = tempfile::tempdir().unwrap();
        write_status(dir.path(), "gaming", "scx_rusty", true, false, 1700000000);
        let document: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(status_path(dir.path())).unwrap())
                .unwrap();
        assert_eq!(document["profile"], "gaming");
        assert_eq!(document["gaming_active"], true);
        assert_eq!(document["manual_override"], false);
        assert_eq!(document["ts"], 1700000000);
    }

    #[test]
    fn lists_scx_binaries_without_loader() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("scx_rusty"), "").unwrap();
        std::fs::write(dir.path().join("scx_loader"), "").unwrap();
        std::fs::write(dir.path().join("other"), "").unwrap();
        let run = |_: &[String], _: u64| None;
        assert_eq!(
            available_schedulers(&run, dir.path()),
            vec!["scx_rusty".to_string()]
        );
        let run = |_: &[String], _: u64| Some((0, "  bore\n\nbalanced\n".to_string()));
        assert_eq!(
            available_schedulers(&run, dir.path()),
            vec!["bore".to_string(), "balanced".to_string()]
        );
    }
}
