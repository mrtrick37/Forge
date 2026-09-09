//! Port of `kyth_shared.guardian`'s READ path — Guardian's own on-disk
//! state (`~/.local/state/kyth/guardian.json`, written by
//! `kyth-guardian.service` or a user-initiated repair) is what this
//! reads; this module never writes it, and never runs `guardian.py`'s
//! `collect_symptoms()`/`inspect()` live probe sweep (a dozen-plus
//! subprocess calls across audio/network/bluetooth/portal/plasma/
//! flatpak/storage/...). Same "read the cache, don't trigger fresh work"
//! boundary as `system::probe`. The one exception is `execute_recipe`,
//! which runs a repair the user explicitly asked for — under the same
//! eligibility gate `guardian.py:execute_recipe` applies.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

pub const SCHEMA_VERSION: u32 = 1;
/// Same as `guardian.py`'s `NOTIFY_THROTTLE_S` — the window
/// `pending_recommendations` considers "still relevant" for the mission
/// bar / sidebar badge.
pub const NOTIFY_THROTTLE_S: f64 = 6.0 * 3600.0;

#[derive(Debug, Clone, PartialEq)]
pub struct ModelDecision {
    pub recipe_id: String,
    pub confidence: f64,
    pub explanation: String,
    pub probe_id: Option<String>,
}

/// Strict, read-only parser for a local-model Guardian suggestion. Callers
/// still own model invocation and must apply the normal eligibility gate.
pub fn parse_model_decision(
    output: &str,
    allowed: &[&str],
    probes: &[&str],
) -> Option<ModelDecision> {
    let raw = output
        .rsplit('{')
        .next()?
        .trim_end_matches(|c: char| c.is_whitespace() || c == '}');
    let value: Value = serde_json::from_str(&format!("{{{raw}}}")).ok()?;
    let object = value.as_object()?;
    if object.len() != 4
        || !["recipe_id", "confidence", "explanation", "probe_id"]
            .iter()
            .all(|key| object.contains_key(*key))
    {
        return None;
    }
    let recipe_id = object.get("recipe_id")?.as_str()?.to_string();
    let confidence = object.get("confidence")?.as_f64()?;
    let explanation = object
        .get("explanation")?
        .as_str()?
        .chars()
        .filter(|c| !c.is_control())
        .take(400)
        .collect::<String>();
    let probe_id = object.get("probe_id")?.as_str().map(str::to_string);
    if !allowed.contains(&recipe_id.as_str())
        || !recipes().iter().any(|recipe| recipe.id == recipe_id)
        || !(0.0..=1.0).contains(&confidence)
        || explanation.len() > 400
        || probe_id
            .as_deref()
            .is_some_and(|probe| !probes.contains(&probe))
    {
        return None;
    }
    Some(ModelDecision {
        recipe_id,
        confidence,
        explanation,
        probe_id,
    })
}

#[derive(Debug, Clone, Copy)]
pub struct Recipe {
    pub id: &'static str,
    pub title: &'static str,
    pub component: &'static str,
    /// Fixed argv `guardian.py` runs for this repair, empty for an advisory
    /// recipe that only notifies. Static data, never built from input.
    pub command: &'static [&'static str],
    pub risk: &'static str,
    pub requires_auth: bool,
    pub automatic: bool,
    pub cooldown: u32,
    pub verification: &'static str,
    pub recovery: &'static str,
}

/// Same table as `guardian.py`'s `RECIPES` — static policy data, not
/// logic, so it's ported in full even though today's callers only read
/// `title`/`risk`. This is what the rest of Guardian's migration builds
/// on next (execution/cooldown/verification), so it stays faithful to the
/// Python original rather than trimmed to current usage. Keep in sync by
/// hand; `guardian.py`'s copy is still the source of truth for what
/// actually gets executed.
pub fn recipes() -> &'static [Recipe] {
    &[
        Recipe { id: "audio.restart", title: "Restart audio services", component: "audio", command: &["systemctl", "--user", "restart", "pipewire.service", "pipewire-pulse.service", "wireplumber.service"], risk: "safe", requires_auth: false, automatic: true, cooldown: 900, verification: "audio", recovery: "Open Hub > This PC > Repair and inspect the audio stack." },
        Recipe { id: "network.restart-user", title: "Restart the NetworkManager user integration", component: "network", command: &["systemctl", "--user", "restart", "plasma-nm.service"], risk: "safe", requires_auth: false, automatic: true, cooldown: 900, verification: "network", recovery: "Open KDE Network Settings; saved connections are not changed." },
        Recipe { id: "flatpak.refresh-metadata", title: "Refresh Flatpak metadata", component: "flatpak", command: &["flatpak", "update", "--appstream", "--user", "--noninteractive"], risk: "safe", requires_auth: false, automatic: true, cooldown: 1800, verification: "flatpak", recovery: "Retry from Hub > Apps." },
        Recipe { id: "flatpak.repair-user", title: "Repair user Flatpak data", component: "flatpak", command: &["flatpak", "repair", "--user"], risk: "confirm", requires_auth: false, automatic: false, cooldown: 3600, verification: "flatpak", recovery: "No apps are removed; retry the app from Hub." },
        Recipe { id: "bluetooth.restart", title: "Restart Bluetooth", component: "bluetooth", command: &["sudo", "-A", "systemctl", "restart", "bluetooth.service"], risk: "confirm", requires_auth: true, automatic: false, cooldown: 1800, verification: "bluetooth", recovery: "Re-open Bluetooth Settings and reconnect the device." },
        Recipe { id: "portal.restart-user", title: "Restart desktop portals", component: "portal", command: &["systemctl", "--user", "restart", "xdg-desktop-portal.service"], risk: "safe", requires_auth: false, automatic: true, cooldown: 900, verification: "portal", recovery: "If file pickers or screen sharing were blank, retry them now." },
        Recipe { id: "plasma.restart-user", title: "Restart Plasma shell", component: "plasma", command: &["systemctl", "--user", "restart", "plasma-plasmashell.service"], risk: "safe", requires_auth: false, automatic: true, cooldown: 900, verification: "plasma", recovery: "If the panel or task manager vanished, it should reappear. Open windows are kept." },
        Recipe { id: "disk.review", title: "Review storage usage", component: "storage", command: &[], risk: "advisory", requires_auth: false, automatic: false, cooldown: 3600, verification: "storage", recovery: "Open Hub > This PC > Hardware > Storage; Guardian never deletes files." },
        Recipe { id: "storage.maint", title: "Run storage maintenance", component: "storage", command: &["/usr/bin/kyth-btrfs-maint"], risk: "safe", requires_auth: false, automatic: false, cooldown: 86400, verification: "storage", recovery: "Gated btrfs scrub/balance (AC+idle+!gaming). Not a timer auto-fix — a scrub can outlive the 90s oneshot." },
        Recipe { id: "firmware.refresh", title: "Refresh firmware metadata", component: "firmware", command: &["flock", "-w", "10", "/run/kyth-fwupd.lock", "fwupdmgr", "refresh", "--force"], risk: "safe", requires_auth: false, automatic: true, cooldown: 43200, verification: "firmware", recovery: "Refreshes LVFS metadata only; does not flash devices." },
        Recipe { id: "display.reconfigure", title: "Re-apply display outputs", component: "display", command: &["systemctl", "--user", "restart", "plasma-kscreen.service"], risk: "safe", requires_auth: false, automatic: true, cooldown: 21600, verification: "display", recovery: "Restarts KScreen and enables connected outputs after dock/HDR change; no reboot." },
        Recipe { id: "controller.repair", title: "Restart controller stack", component: "controller", command: &["sudo", "-A", "systemctl", "restart", "joycond.service"], risk: "confirm", requires_auth: true, automatic: false, cooldown: 21600, verification: "controller", recovery: "Restarts system joycond after suspend; may ask for permission. Re-pair if needed." },
        Recipe { id: "network.captive-fix", title: "Re-toggle networking for captive portals", component: "network", command: &["nmcli", "networking", "off"], risk: "safe", requires_auth: false, automatic: false, cooldown: 1800, verification: "network", recovery: "Re-toggles NetworkManager to clear captive portal / local-only state; saved connections kept. Not an unattended auto-fix — a failed re-enable would leave networking off." },
        Recipe { id: "audio.sink-fallback", title: "Restore default audio sink", component: "audio", command: &["pactl", "list", "short", "sinks"], risk: "safe", requires_auth: false, automatic: true, cooldown: 900, verification: "audio", recovery: "Falls back to the first real sink after HDMI/headset swap; no data changed." },
        Recipe { id: "power.profile-fix", title: "Reset power profile to balanced", component: "power", command: &["powerprofilesctl", "set", "balanced"], risk: "safe", requires_auth: false, automatic: true, cooldown: 3600, verification: "power", recovery: "Resets stuck power profile after driver update; no reboot." },
        Recipe { id: "thermal.notify", title: "Thermal throttling detected", component: "thermal", command: &[], risk: "advisory", requires_auth: false, automatic: false, cooldown: 3600, verification: "thermal", recovery: "System is hot — close heavy tasks and check vents; Guardian resumes after cooldown." },
        Recipe { id: "storage.smart-warn", title: "SMART disk health at risk", component: "storage", command: &[], risk: "advisory", requires_auth: false, automatic: false, cooldown: 86400, verification: "storage", recovery: "SMART reports reallocated/pending sectors — back up and check Disks." },
        Recipe { id: "memory.pressure-relief", title: "Memory pressure high", component: "memory", command: &[], risk: "advisory", requires_auth: false, automatic: false, cooldown: 3600, verification: "memory", recovery: "High PSI / low MemAvailable — close heavy apps; Guardian pauses auto-fixes until pressure drops." },
        Recipe { id: "network.vpn-fix", title: "Restart always-on VPN connection", component: "network", command: &["nmcli", "-t", "-f", "NAME,TYPE,AUTOCONNECT", "connection", "show"], risk: "safe", requires_auth: false, automatic: true, cooldown: 1800, verification: "network", recovery: "Re-establishes an autoconnect VPN after a captive-portal hop; idle VPN profiles are left alone." },
        Recipe { id: "network.dns-flush", title: "Flush DNS cache", component: "network", command: &["resolvectl", "flush-caches"], risk: "safe", requires_auth: false, automatic: true, cooldown: 1800, verification: "network", recovery: "Flushes systemd-resolved cache after portal/DNS change." },
        Recipe { id: "update.review-health", title: "Review update health", component: "updates", command: &[], risk: "advisory", requires_auth: false, automatic: false, cooldown: 3600, verification: "updates", recovery: "Run ujust update-health; rollback remains controlled by boot health." },
    ]
}

pub fn recipe_title(recipe_id: &str) -> String {
    recipes()
        .iter()
        .find(|r| r.id == recipe_id)
        .map_or_else(|| recipe_id.to_string(), |r| r.title.to_string())
}

pub fn recipe_risk(recipe_id: &str) -> String {
    recipes()
        .iter()
        .find(|r| r.id == recipe_id)
        .map_or_else(|| "unknown".to_string(), |r| r.risk.to_string())
}

/// Recipe ids whose real work lives in `guardian_actions.py`'s
/// `ACTION_EXECUTORS`, not in their `command` tuple. Running the tuple on
/// its own would be wrong, not merely incomplete: `audio.sink-fallback`'s
/// command is a read (`pactl list short sinks`) and `network.captive-fix`'s
/// is only the "off" half of a toggle, so it would leave networking down.
/// These are dispatched by `run_executor` below rather than by running the
/// recipe's first argv.  Some recipes are deliberately multi-step.
const EXECUTOR_ONLY: &[&str] = &[
    "display.reconfigure",
    "audio.sink-fallback",
    "power.profile-fix",
    "network.dns-flush",
    "network.captive-fix",
    "network.vpn-fix",
    "controller.repair",
    "portal.restart-user",
    "firmware.refresh",
    "storage.maint",
];

/// `guardian.py:execute_recipe`'s wording for anything that must not run.
const NOT_ELIGIBLE: &str = "recipe is not eligible for automatic execution";

fn run_command(argv: &[&str], timeout: Duration) -> Result<String, String> {
    // Same shape as `system::printing::run_with_timeout`, but keeping stderr
    // too — `guardian.py:_run` reports stderr first, and a failed systemctl
    // says nothing on stdout.
    let argv = argv
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    let out = crate::system::process::run_bounded(&argv, timeout).map_err(|error| {
        if error.kind() == std::io::ErrorKind::TimedOut {
            "repair timed out".to_string()
        } else {
            "repair failed to start".to_string()
        }
    })?;
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let mut detail = if stderr.is_empty() { stdout } else { stderr };
    detail.truncate(400);
    if out.status.success() {
        Ok(if detail.is_empty() {
            "done".to_string()
        } else {
            detail
        })
    } else {
        Err(if detail.is_empty() {
            "repair failed".to_string()
        } else {
            detail
        })
    }
}

fn run_ok(argv: &[&str], timeout: Duration) -> Result<(), String> {
    run_command(argv, timeout).map(|_| ())
}

/// Rust equivalent of guardian_actions.py's bounded executors. Every command
/// remains an argv invocation; no user-controlled text reaches a shell.
fn run_executor(id: &str) -> Result<String, String> {
    match id {
        "display.reconfigure" => {
            let _ = run_ok(
                &["systemctl", "--user", "restart", "plasma-kscreen.service"],
                Duration::from_secs(10),
            );
            run_ok(&["kscreen-doctor", "-o"], Duration::from_secs(8))
                .map(|_| "display outputs refreshed".to_string())
        }
        "audio.sink-fallback" => {
            let output = run_command(&["pactl", "list", "short", "sinks"], Duration::from_secs(6))?;
            let sink = output
                .lines()
                .filter_map(|line| line.split_whitespace().nth(1))
                .find(|name| !name.contains("auto_null") && *name != "@DEFAULT_SINK@");
            let Some(sink) = sink else {
                return Err("no usable audio sink".to_string());
            };
            run_ok(&["pactl", "set-default-sink", sink], Duration::from_secs(6))?;
            Ok(format!("default sink set to {sink}"))
        }
        "power.profile-fix" => {
            run_ok(
                &["powerprofilesctl", "set", "balanced"],
                Duration::from_secs(6),
            )?;
            Ok("power profile set to balanced".to_string())
        }
        "network.dns-flush" => {
            if run_ok(&["resolvectl", "flush-caches"], Duration::from_secs(6)).is_ok()
                || run_ok(
                    &["systemd-resolve", "--flush-caches"],
                    Duration::from_secs(6),
                )
                .is_ok()
            {
                Ok("flushed DNS caches".to_string())
            } else {
                Err("DNS cache flush unavailable".to_string())
            }
        }
        "network.captive-fix" => {
            run_ok(&["nmcli", "networking", "off"], Duration::from_secs(8))?;
            std::thread::sleep(Duration::from_secs(2));
            let result = run_ok(&["nmcli", "networking", "on"], Duration::from_secs(8));
            if result.is_ok() {
                Ok("networking re-toggled".to_string())
            } else {
                let _ = run_ok(&["nmcli", "networking", "on"], Duration::from_secs(8));
                Err("failed to re-enable networking".to_string())
            }
        }
        "network.vpn-fix" => {
            let listed = run_command(
                &[
                    "nmcli",
                    "-t",
                    "-f",
                    "NAME,TYPE,AUTOCONNECT",
                    "connection",
                    "show",
                ],
                Duration::from_secs(6),
            )?;
            for line in listed.lines() {
                let mut parts = line.rsplitn(3, ':');
                let auto = parts.next().unwrap_or("");
                let kind = parts.next().unwrap_or("");
                let name = parts.next().unwrap_or("");
                if kind == "vpn" && auto == "yes" && !name.is_empty() {
                    run_ok(
                        &["nmcli", "connection", "up", name],
                        Duration::from_secs(15),
                    )?;
                    return Ok(format!("brought up VPN {name}"));
                }
            }
            Err("no always-on VPN needed reconnecting".to_string())
        }
        "controller.repair" => {
            run_ok(
                &["sudo", "-A", "systemctl", "restart", "joycond.service"],
                Duration::from_secs(20),
            )?;
            Ok("joycond restarted".to_string())
        }
        "portal.restart-user" => {
            run_ok(
                &[
                    "systemctl",
                    "--user",
                    "restart",
                    "xdg-desktop-portal.service",
                ],
                Duration::from_secs(15),
            )?;
            let _ = run_ok(
                &[
                    "systemctl",
                    "--user",
                    "restart",
                    "plasma-xdg-desktop-portal-kde.service",
                ],
                Duration::from_secs(15),
            );
            Ok("desktop portals restarted".to_string())
        }
        "firmware.refresh" => {
            if run_ok(
                &[
                    "flock",
                    "-w",
                    "10",
                    "/run/kyth-fwupd.lock",
                    "fwupdmgr",
                    "refresh",
                    "--force",
                ],
                Duration::from_secs(30),
            )
            .is_err()
            {
                run_ok(&["fwupdmgr", "refresh", "--force"], Duration::from_secs(30))?;
            }
            Ok("firmware metadata refreshed".to_string())
        }
        "storage.maint" => {
            run_ok(&["/usr/bin/kyth-btrfs-maint"], Duration::from_secs(3600))?;
            Ok("storage maintenance started".to_string())
        }
        _ => Err(NOT_ELIGIBLE.to_string()),
    }
}

#[cfg(test)]
fn executor_supported(id: &str) -> bool {
    matches!(
        id,
        "display.reconfigure"
            | "audio.sink-fallback"
            | "power.profile-fix"
            | "network.dns-flush"
            | "network.captive-fix"
            | "network.vpn-fix"
            | "controller.repair"
            | "portal.restart-user"
            | "firmware.refresh"
            | "storage.maint"
    )
}

/// Persist Guardian state for the native service and the Tauri command path.
/// The write remains atomic so a concurrent Hub read sees either the previous
/// state or the complete new state.
pub fn save_state(state: &Value) -> Result<(), String> {
    let path = state_path();
    let parent = path
        .parent()
        .ok_or_else(|| "invalid Guardian state path".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|_| "could not create Guardian state directory".to_string())?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(
        &temp,
        serde_json::to_vec_pretty(state)
            .map_err(|_| "could not encode Guardian state".to_string())?,
    )
    .map_err(|_| "could not write Guardian state".to_string())?;
    std::fs::rename(temp, path).map_err(|_| "could not commit Guardian state".to_string())
}

/// Record a native service check using the same history shape consumed by the
/// Hub's pending-recommendation projection.
pub fn record_service_check(symptoms: &[Value], decisions: &[Value]) -> Result<Value, String> {
    let mut state = load_state();
    let occurrences = state
        .get_mut("occurrences")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "Guardian occurrences are unavailable".to_string())?;
    let active = symptoms
        .iter()
        .filter_map(|symptom| symptom.get("component").and_then(Value::as_str))
        .collect::<std::collections::HashSet<_>>();
    for recipe in recipes() {
        let count = if active.contains(recipe.component) {
            occurrences
                .get(recipe.component)
                .and_then(Value::as_i64)
                .unwrap_or(0)
                .saturating_add(1)
        } else {
            0
        };
        occurrences.insert(recipe.component.to_string(), Value::from(count));
    }
    let history = state
        .get_mut("history")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "Guardian history is unavailable".to_string())?;
    history.extend(decisions.iter().cloned());
    if history.len() > 200 {
        let keep_from = history.len() - 200;
        history.drain(0..keep_from);
    }
    state["last_check"] = Value::from(now_unix());
    save_state(&state)?;
    Ok(state)
}

fn cooldown_active(state: &Value, recipe: &Recipe) -> bool {
    let now = now_unix();
    state
        .get("history")
        .and_then(Value::as_array)
        .is_some_and(|history| {
            history.iter().any(|item| {
                item.get("recipe_id").and_then(Value::as_str) == Some(recipe.id)
                    && item.get("action").and_then(Value::as_str) == Some("executed")
                    && item.get("verified").and_then(Value::as_bool) != Some(false)
                    && now - item.get("timestamp").and_then(Value::as_f64).unwrap_or(0.0)
                        < recipe.cooldown as f64
            })
        })
}

/// Run a repair the user asked for, porting the `user_initiated=True` path
/// of `guardian.py:execute_recipe`.
///
/// The gate is the point of this function. The Hub used to hand `recipe_id`
/// to `just_run`, which meant two wrong things at once: a dotted id is not
/// a just recipe so nothing ever ran, and the spawn still "succeeded", so
/// the Hub reported every repair as launched. Advisory recipes
/// (`thermal.notify`, `storage.smart-warn`) have no command in `guardian.py`
/// either — they are notifications, and must stay one click away from
/// nothing at all.
pub fn execute_recipe(recipe_id: &str) -> Result<String, String> {
    let Some(recipe) = recipes().iter().find(|r| r.id == recipe_id) else {
        return Err(NOT_ELIGIBLE.to_string());
    };
    if recipe.command.is_empty() || !matches!(recipe.risk, "safe" | "confirm") {
        return Err(NOT_ELIGIBLE.to_string());
    }
    let state = load_state();
    if cooldown_active(&state, recipe) {
        return Err("repair cooldown is active".to_string());
    }
    if recipe.requires_auth && std::env::var_os("SUDO_ASKPASS").is_none() {
        // The command is `sudo -A …`; with no askpass helper in the session
        // it can only fail, and it would fail with sudo's wording rather
        // than something a user can act on.
        return Err(format!(
            "{} needs an administrator password, and this session has no askpass helper — run it from a terminal",
            recipe.title,
        ));
    }
    let result = if EXECUTOR_ONLY.contains(&recipe.id) {
        run_executor(recipe.id)
    } else {
        run_command(recipe.command, Duration::from_secs(30))
    };
    let (ok, detail) = match result {
        Ok(detail) => (true, detail),
        Err(detail) => (false, detail),
    };
    let verified = ok && verify_recipe(recipe.id);
    let mut next = state;
    if let Some(history) = next.get_mut("history").and_then(Value::as_array_mut) {
        history.push(serde_json::json!({"timestamp": now_unix(), "recipe_id": recipe.id, "source": "hub", "action": "executed", "verified": verified, "detail": detail}));
        if history.len() > 200 {
            let keep_from = history.len() - 200;
            history.drain(0..keep_from);
        }
    }
    let _ = save_state(&next);
    if ok && verified {
        Ok(detail)
    } else if ok {
        Err(format!("{detail}; post-repair verification failed"))
    } else {
        Err(detail)
    }
}

fn verify_recipe(recipe_id: &str) -> bool {
    match recipe_id {
        "audio.sink-fallback" => {
            run_ok(&["pactl", "get-default-sink"], Duration::from_secs(4)).is_ok()
        }
        "network.captive-fix" | "network.vpn-fix" | "network.dns-flush" => run_ok(
            &["nmcli", "-t", "-f", "STATE", "general"],
            Duration::from_secs(5),
        )
        .is_ok(),
        "controller.repair" => run_ok(
            &["systemctl", "is-active", "--quiet", "joycond.service"],
            Duration::from_secs(5),
        )
        .is_ok(),
        "portal.restart-user" => run_ok(
            &[
                "systemctl",
                "--user",
                "is-active",
                "--quiet",
                "xdg-desktop-portal.service",
            ],
            Duration::from_secs(5),
        )
        .is_ok(),
        "firmware.refresh" => run_ok(&["fwupdmgr", "get-updates"], Duration::from_secs(8)).is_ok(),
        "power.profile-fix" => run_ok(&["powerprofilesctl", "get"], Duration::from_secs(4)).is_ok(),
        "display.reconfigure" => run_ok(&["kscreen-doctor", "-o"], Duration::from_secs(5)).is_ok(),
        "storage.maint" => true,
        _ => run_ok(
            &[
                "systemctl",
                "--user",
                "is-active",
                "--quiet",
                "pipewire.service",
            ],
            Duration::from_secs(5),
        )
        .is_ok(),
    }
}

fn state_dir() -> PathBuf {
    let base = std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{home}/.local/state")
    });
    PathBuf::from(base).join("kyth")
}

fn state_path() -> PathBuf {
    state_dir().join("guardian.json")
}

fn empty_state() -> Value {
    serde_json::json!({ "schema_version": SCHEMA_VERSION, "history": [], "occurrences": {} })
}

/// Port of `guardian.py`'s `load_state()` from an explicit path — `path`
/// overrides `state_path()`, same "explicit path for tests" shape as
/// `system::probe::read_section_in`.
pub fn load_state_from(path: &Path) -> Value {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return empty_state();
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return empty_state();
    };
    if !value.is_object() {
        return empty_state();
    }
    if !value.get("history").is_some_and(Value::is_array) {
        return empty_state();
    }
    value
}

/// `load_state_from` against the real on-disk `state_path()` — what every
/// non-test caller wants.
pub fn load_state() -> Value {
    load_state_from(&state_path())
}

fn now_unix() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PendingItem {
    pub recipe_id: String,
    pub detail: String,
}

/// Port of `guardian.py`'s `pending_recommendations(state, now=None,
/// window=NOTIFY_THROTTLE_S)` — same "latest history entry per recipe_id
/// inside the window, still `action == "recommended"`" logic. This is the
/// exact same list Hub's own mission bar / sidebar badge is built from.
pub fn pending_recommendations(state: &Value) -> Vec<PendingItem> {
    let now = now_unix();
    let mut latest: std::collections::HashMap<String, (f64, Value)> =
        std::collections::HashMap::new();
    let Some(history) = state.get("history").and_then(Value::as_array) else {
        return Vec::new();
    };
    for item in history {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let Some(recipe_id) = obj.get("recipe_id").and_then(Value::as_str) else {
            continue;
        };
        let Some(timestamp) = obj.get("timestamp").and_then(Value::as_f64) else {
            continue;
        };
        // No `age < 0` guard here — matches guardian.py exactly, which
        // only skips entries *older* than the window, not future-dated
        // ones (clock skew), unlike system::probe::read_section_in.
        if now - timestamp > NOTIFY_THROTTLE_S {
            continue;
        }
        let replace = latest
            .get(recipe_id)
            .is_none_or(|(prev_ts, _)| timestamp >= *prev_ts);
        if replace {
            latest.insert(recipe_id.to_string(), (timestamp, item.clone()));
        }
    }
    latest
        .into_values()
        .filter(|(_, item)| item.get("action").and_then(Value::as_str) == Some("recommended"))
        .map(|(_, item)| PendingItem {
            recipe_id: item
                .get("recipe_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            detail: item
                .get("detail")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryItem {
    pub timestamp: f64,
    pub recipe_id: Option<String>,
    pub detail: String,
    pub action: String,
    pub verified: Option<bool>,
}

/// Most-recent-first, capped to `limit`, entries without a timestamp
/// dropped — mirrors what the retired Python `guardian_bridge.py`
/// computed for its "history" field before this port replaced it.
pub fn recent_history(state: &Value, limit: usize) -> Vec<HistoryItem> {
    let Some(history) = state.get("history").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut items: Vec<HistoryItem> = history
        .iter()
        .filter_map(|item| {
            let obj = item.as_object()?;
            let timestamp = obj.get("timestamp").and_then(Value::as_f64)?;
            Some(HistoryItem {
                timestamp,
                recipe_id: obj
                    .get("recipe_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                detail: obj
                    .get("detail")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                action: obj
                    .get("action")
                    .and_then(Value::as_str)
                    .unwrap_or("executed")
                    .to_string(),
                verified: obj.get("verified").and_then(Value::as_bool),
            })
        })
        .collect();
    items.sort_by(|a, b| {
        b.timestamp
            .partial_cmp(&a.timestamp)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    items.truncate(limit);
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_decision_requires_exact_allowed_shape() {
        let allowed = ["audio.restart"];
        let probes = ["audio"];
        let valid = r#"note {"recipe_id":"audio.restart","confidence":0.8,"explanation":"restart audio","probe_id":"audio"}"#;
        assert_eq!(
            parse_model_decision(valid, &allowed, &probes)
                .unwrap()
                .recipe_id,
            "audio.restart"
        );
        assert!(parse_model_decision(
            r#"{"recipe_id":"bad","confidence":1.0,"explanation":"x","probe_id":"audio"}"#,
            &allowed,
            &probes
        )
        .is_none());
        assert!(parse_model_decision("not json", &allowed, &probes).is_none());
    }
    use serde_json::json;

    #[test]
    fn missing_state_file_is_empty_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let state = load_state_from(&dir.path().join("guardian.json"));
        assert_eq!(state, empty_state());
    }

    /// Advisory recipes are notifications: `guardian.py` gives them an empty
    /// command tuple and refuses to execute them. Before this gate, the Hub
    /// handed the id straight to `just_run` and reported "launched".
    #[test]
    fn advisory_recipes_never_execute() {
        for id in [
            "thermal.notify",
            "storage.smart-warn",
            "memory.pressure-relief",
            "disk.review",
            "update.review-health",
        ] {
            assert_eq!(execute_recipe(id), Err(NOT_ELIGIBLE.to_string()), "{id}");
        }
    }

    #[test]
    fn unknown_recipe_ids_never_execute() {
        assert_eq!(
            execute_recipe("not.a.real.recipe"),
            Err(NOT_ELIGIBLE.to_string())
        );
        assert_eq!(execute_recipe(""), Err(NOT_ELIGIBLE.to_string()));
    }

    /// Every executor-backed recipe has an explicit Rust implementation; this
    /// test is static so the suite never runs mutating system commands.
    #[test]
    fn executor_backed_recipes_are_implemented() {
        for id in EXECUTOR_ONLY {
            assert!(executor_supported(id), "{id} has no Rust executor");
        }
    }

    /// Every executable recipe carries the argv `guardian.py` runs; an empty
    /// one would fall through `run_command` and index out of bounds.
    #[test]
    fn executable_recipes_have_a_command() {
        for recipe in recipes() {
            if matches!(recipe.risk, "safe" | "confirm") && !EXECUTOR_ONLY.contains(&recipe.id) {
                assert!(!recipe.command.is_empty(), "{}", recipe.id);
            }
        }
    }

    #[test]
    fn recipe_lookup_knows_a_real_recipe() {
        assert_eq!(recipe_title("audio.restart"), "Restart audio services");
        assert_eq!(recipe_risk("audio.restart"), "safe");
    }

    #[test]
    fn recipe_lookup_falls_back_gracefully_for_an_unknown_id() {
        assert_eq!(recipe_title("not.a.real.recipe"), "not.a.real.recipe");
        assert_eq!(recipe_risk("not.a.real.recipe"), "unknown");
    }

    #[test]
    fn pending_recommendation_is_the_latest_entry_for_its_recipe() {
        let now = now_unix();
        let state = json!({
            "schema_version": 1,
            "history": [
                { "timestamp": now - 100.0, "recipe_id": "audio.restart", "action": "recommended", "detail": "old" },
                { "timestamp": now, "recipe_id": "audio.restart", "action": "recommended", "detail": "new" },
            ],
            "occurrences": {},
        });
        let pending = pending_recommendations(&state);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].detail, "new");
    }

    #[test]
    fn resolved_recipe_is_not_pending() {
        let now = now_unix();
        let state = json!({
            "schema_version": 1,
            "history": [
                { "timestamp": now, "recipe_id": "audio.restart", "action": "executed", "detail": "fixed" },
            ],
            "occurrences": {},
        });
        assert_eq!(pending_recommendations(&state), Vec::new());
    }

    #[test]
    fn old_recommendation_outside_the_notify_window_is_not_pending() {
        let old = now_unix() - NOTIFY_THROTTLE_S - 10.0;
        let state = json!({
            "schema_version": 1,
            "history": [
                { "timestamp": old, "recipe_id": "audio.restart", "action": "recommended", "detail": "stale" },
            ],
            "occurrences": {},
        });
        assert_eq!(pending_recommendations(&state), Vec::new());
    }

    #[test]
    fn recent_history_is_most_recent_first_and_capped() {
        let now = now_unix();
        let history: Vec<Value> = (0..12)
            .map(|i| json!({ "timestamp": now - i as f64, "recipe_id": "audio.restart", "action": "executed", "detail": format!("run {i}") }))
            .collect();
        let state = json!({ "schema_version": 1, "history": history, "occurrences": {} });
        let items = recent_history(&state, 8);
        assert_eq!(items.len(), 8);
        assert_eq!(items[0].detail, "run 0"); // most recent (smallest i => largest timestamp) first
    }

    #[test]
    fn recent_history_drops_entries_without_a_timestamp() {
        let state = json!({
            "schema_version": 1,
            "history": [ { "recipe_id": "audio.restart", "action": "executed", "detail": "no timestamp" } ],
            "occurrences": {},
        });
        assert_eq!(recent_history(&state, 8), Vec::new());
    }
}

/// Is `recipe_id` currently a pending recommendation?
///
/// The authorization gate for acting on a Guardian recommendation: only a
/// recipe Guardian is *currently recommending* may be executed, so a
/// caller can't be talked into running an arbitrary recipe id by whatever
/// hands it the string. Kept here next to the list it derives from; the
/// explicit execution path applies this gate before dispatching a fixed
/// recipe command.
pub fn is_pending_recipe(state: &Value, recipe_id: &str) -> bool {
    pending_recommendations(state)
        .iter()
        .any(|p| p.recipe_id == recipe_id)
}

/// Dismiss a current recommendation without running its repair. Record the
/// dismissal as the newest per-recipe history item so the existing pending
/// projection stops surfacing it across the Hub and notifications.
pub fn dismiss_recommendation(recipe_id: &str) -> Result<String, String> {
    let mut state = load_state();
    if !is_pending_recipe(&state, recipe_id) {
        return Err("recommendation is no longer pending".to_string());
    }
    let history = state
        .get_mut("history")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "Guardian history is unavailable".to_string())?;
    history.push(serde_json::json!({
        "timestamp": now_unix(),
        "recipe_id": recipe_id,
        "source": "hub",
        "action": "dismissed",
        "verified": null,
        "detail": "Dismissed from the Home screen.",
    }));
    save_state(&state)?;
    Ok(format!("Dismissed {}.", recipe_title(recipe_id)))
}

#[cfg(test)]
mod pending_gate_tests {
    use super::{is_pending_recipe, now_unix};
    use serde_json::json;

    fn state_with(action: &str) -> serde_json::Value {
        json!({
            "schema_version": 1,
            "history": [
                { "timestamp": now_unix(), "recipe_id": "audio.restart", "action": action, "detail": "d" }
            ],
            "occurrences": {},
        })
    }

    #[test]
    fn allows_a_currently_recommended_recipe() {
        assert!(is_pending_recipe(
            &state_with("recommended"),
            "audio.restart"
        ));
    }

    #[test]
    fn rejects_a_recipe_that_is_not_recommended() {
        assert!(!is_pending_recipe(&state_with("executed"), "audio.restart"));
    }

    #[test]
    fn rejects_an_unknown_or_empty_recipe_id() {
        let state = state_with("recommended");
        for id in ["", "network.reset", "audio.restart\n", "../../etc/passwd"] {
            assert!(
                !is_pending_recipe(&state, id),
                "{id:?} must not pass the gate"
            );
        }
    }

    #[test]
    fn rejects_everything_when_there_is_no_history() {
        let empty = json!({ "schema_version": 1, "history": [], "occurrences": {} });
        assert!(!is_pending_recipe(&empty, "audio.restart"));
    }
}
