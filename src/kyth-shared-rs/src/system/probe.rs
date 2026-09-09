//! Port of `kyth_shared.system.probe`'s READ path — `kyth-probe.service`
//! The native probe service collects and writes the on-disk cache this reads;
//! the Hub itself only reads it. The collector preserves the compatibility
//! cache schema while keeping every child process bounded.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

struct ProbeCacheLock(PathBuf);
impl Drop for ProbeCacheLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Cache contract shared with the legacy Python compatibility module — how
/// many seconds old a cached section may be before `read_section` refuses to
/// return it. The installed `kyth-probe.service` now uses this Rust module.
pub fn disk_ttl() -> HashMap<&'static str, f64> {
    HashMap::from([
        ("bootc-status-data", 90.0),
        ("bootc-status-text", 90.0),
        ("bootc-branch", 90.0),
        ("kernel-flavor", 600.0),
        ("flatpak-apps", 180.0),
        ("flatpak-updates", 180.0),
        ("nvidia-detect", 300.0),
        ("controllers-detect", 120.0),
        ("display-detect", 30.0),
        ("hardware-probes", 30.0),
        ("ntfs-drives", 30.0),
        ("secureboot-state", 300.0),
        ("hardware-summary", 30.0),
        ("network-summary", 60.0),
        ("audit-cache", 30.0),
        ("firmware-cache", 300.0),
    ])
}

fn user_runtime_cache_path() -> PathBuf {
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime).join("kyth").join("probe-cache.json");
    }
    let uid = rustix::process::getuid().as_raw();
    PathBuf::from(format!("/run/user/{uid}"))
        .join("kyth")
        .join("probe-cache.json")
}

fn user_home_cache_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        return PathBuf::from(xdg).join("kyth").join("probe-cache.json");
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home)
        .join(".cache")
        .join("kyth")
        .join("probe-cache.json")
}

fn system_cache_path() -> PathBuf {
    PathBuf::from("/var/cache/kyth/probe-cache.json")
}

/// Select the cache destination for the native probe service.  The system
/// service passes `system=true`; the user service keeps its cache under the
/// session runtime directory and falls back to the user's cache directory.
pub fn default_write_path(system: bool) -> PathBuf {
    if system || rustix::process::getuid().is_root() {
        return system_cache_path();
    }
    let runtime = user_runtime_cache_path();
    if runtime
        .parent()
        .map(|parent| std::fs::create_dir_all(parent).is_ok())
        .unwrap_or(false)
    {
        runtime
    } else {
        user_home_cache_path()
    }
}

/// Atomically replace a cache document.  Cache readers can therefore never
/// observe a partially serialized probe result, even when a service timeout
/// overlaps a Hub refresh.
pub fn write_cache_file(path: &Path, document: &Value) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "cache path has no parent")
    })?;
    if !document.is_object()
        || document
            .get("sections")
            .and_then(Value::as_object)
            .is_none()
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "probe cache document must contain an object sections field",
        ));
    }
    std::fs::create_dir_all(parent)?;
    let lock = path.with_extension(format!(
        "{}.lock",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("cache")
    ));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let _guard = loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(_) => break ProbeCacheLock(lock.clone()),
            Err(error)
                if error.kind() == std::io::ErrorKind::AlreadyExists
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(error) => return Err(error),
        }
    };
    let tmp = parent.join(format!(
        ".probe-{}-{}.json",
        std::process::id(),
        now_unix() as u128
    ));
    let payload = serde_json::to_vec(document)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        use std::io::Write;
        file.write_all(&payload)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)?;
        Ok::<(), std::io::Error>(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Merge freshly collected sections into the shared cache document.
pub fn update_sections(
    sections: &serde_json::Map<String, Value>,
    path: Option<&Path>,
    system: bool,
) -> std::io::Result<PathBuf> {
    let target = path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_write_path(system));
    let mut document = load_cache_file(&target)
        .unwrap_or_else(|| serde_json::json!({"version": 2, "generated_at": 0.0, "sections": {}}));
    let object = document.as_object_mut().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "probe cache is not an object",
        )
    })?;
    let cache_sections = object
        .entry("sections")
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "probe sections are not an object",
            )
        })?;
    let now = now_unix();
    for (key, data) in sections {
        cache_sections.insert(key.clone(), serde_json::json!({"ts": now, "data": data}));
    }
    object.insert("version".into(), Value::from(2));
    object.insert("generated_at".into(), Value::from(now));
    write_cache_file(&target, &Value::Object(object.clone()))?;
    Ok(target)
}

fn command_output(program: &str, args: &[&str], timeout_secs: u64) -> Option<String> {
    let mut argv = vec![program.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    let output =
        crate::system::process::run_bounded(&argv, std::time::Duration::from_secs(timeout_secs))
            .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

fn collect_flatpak_apps() -> Value {
    let Some(output) = command_output("flatpak", &["list", "--app", "--columns=application"], 15)
    else {
        return Value::Null;
    };
    let mut apps = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    apps.sort();
    apps.dedup();
    Value::Array(apps.into_iter().map(Value::String).collect())
}

fn collect_flatpak_updates() -> Value {
    let mut total = 0i64;
    let mut succeeded = false;
    for scope in ["--system", "--user"] {
        if let Some(output) = command_output(
            "flatpak",
            &["remote-ls", "--updates", scope, "--columns=application"],
            30,
        ) {
            succeeded = true;
            total += output
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count() as i64;
        }
    }
    succeeded
        .then_some(Value::from(total))
        .unwrap_or(Value::Null)
}

fn collect_hardware() -> Option<(Value, Value)> {
    let evaluation = crate::system::hardware_policy::evaluate_system().ok()?;
    let has_nvidia = evaluation
        .inventory
        .pci
        .iter()
        .any(|device| device.vendor == "10de" && device.class_code.starts_with("03"));
    let is_hybrid = evaluation
        .capabilities
        .iter()
        .any(|capability| capability == "gpu.hybrid" || capability == "gpu.offload");
    let capabilities = evaluation
        .capabilities
        .iter()
        .take(8)
        .cloned()
        .collect::<Vec<_>>();
    let profiles = evaluation
        .profiles
        .iter()
        .filter_map(|profile| profile.get("id").and_then(Value::as_str))
        .take(3)
        .map(str::to_string)
        .collect::<Vec<_>>();
    let summary = serde_json::json!({
        "has_nvidia": has_nvidia,
        "is_hybrid": is_hybrid,
        "capabilities": capabilities,
        "profiles": profiles,
    });
    Some((Value::Bool(has_nvidia), summary))
}

/// Collect the sections formerly emitted by `kyth_shared.system.probe`.
/// Every operation is bounded and failure is represented as JSON `null`, so a
/// missing optional utility cannot prevent the rest of the cache from being
/// refreshed.
pub fn collect_snapshot() -> serde_json::Map<String, Value> {
    let mut sections = serde_json::Map::new();
    let status_data = crate::system::bootc_query::fetch_status_data();
    let status_text = crate::system::bootc_query::fetch_status_text();
    let reference = status_data
        .as_ref()
        .and_then(crate::system::bootc_query::image_reference_from_status)
        .or_else(|| {
            crate::system::bootc_query::image_reference_from_status_with_output(
                &status_data.clone().unwrap_or(Value::Null),
                &status_text,
            )
        });
    sections.insert(
        "bootc-status-data".into(),
        status_data.unwrap_or(Value::Null),
    );
    sections.insert("bootc-status-text".into(), Value::String(status_text));
    sections.insert(
        "bootc-branch".into(),
        crate::system::bootc_policy::branch_from_ref(reference.as_deref())
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    sections.insert(
        "kernel-flavor".into(),
        Value::String(crate::system::bootc::current_kernel_flavor()),
    );
    sections.insert("flatpak-apps".into(), collect_flatpak_apps());
    sections.insert("flatpak-updates".into(), collect_flatpak_updates());
    if let Some((nvidia, hardware)) = collect_hardware() {
        sections.insert("nvidia-detect".into(), nvidia);
        sections.insert("hardware-summary".into(), hardware.clone());
        sections.insert("display-detect".into(), hardware);
    } else {
        sections.insert("nvidia-detect".into(), Value::Null);
        sections.insert("hardware-summary".into(), Value::Null);
        sections.insert("display-detect".into(), Value::Null);
    }
    sections.insert(
        "controllers-detect".into(),
        serde_json::to_value(crate::system::controllers::detect_controllers())
            .unwrap_or(Value::Null),
    );
    sections.insert(
        "network-summary".into(),
        serde_json::to_value(crate::system::network_identity::get_network_identity())
            .unwrap_or(Value::Null),
    );
    sections
}

/// Same precedence as the compatibility module's `cache_read_paths` for a
/// logged-in desktop user. The system service writes the shared system path;
/// readers always accept it as the final fallback.
pub fn cache_read_paths() -> Vec<PathBuf> {
    vec![
        user_runtime_cache_path(),
        user_home_cache_path(),
        system_cache_path(),
    ]
}

fn load_cache_file(path: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    let data: Value = serde_json::from_str(&raw).ok()?;
    if !data.is_object() {
        return None;
    }
    data.get("sections")?.as_object()?;
    Some(data)
}

fn now_unix() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Port of `probe.py`'s `read_section(key, max_age=None, paths=None)` —
/// reads whatever `kyth-probe.service` (or a prior Hub session) already
/// cached for `key`, picking the freshest entry within its TTL across the
/// candidate paths. `paths` overrides `cache_read_paths()`, same as the
/// Python original's optional param — tests use this instead of mutating
/// process-global env vars.
pub fn read_section_in(key: &str, paths: &[PathBuf]) -> Option<Value> {
    let ttl = *disk_ttl().get(key)?;
    let now = now_unix();
    let mut best: Option<(f64, Value)> = None;
    for path in paths {
        let Some(doc) = load_cache_file(path) else {
            continue;
        };
        let Some(entry) = doc.get("sections").and_then(|s| s.get(key)) else {
            continue;
        };
        let Some(entry_obj) = entry.as_object() else {
            continue;
        };
        let Some(ts) = entry_obj.get("ts").and_then(Value::as_f64) else {
            continue;
        };
        let Some(data) = entry_obj.get("data") else {
            continue;
        };
        let age = now - ts;
        if age < 0.0 || age > ttl {
            continue;
        }
        if best
            .as_ref()
            .map(|(best_ts, _)| ts > *best_ts)
            .unwrap_or(true)
        {
            best = Some((ts, data.clone()));
        }
    }
    best.map(|(_, data)| data)
}

/// `read_section_in` against the real `cache_read_paths()` — what every
/// non-test caller wants.
pub fn read_section(key: &str) -> Option<Value> {
    read_section_in(key, &cache_read_paths())
}

/// Return a cache document with selected sections removed.
///
/// This is the pure transformation used by hotplug and post-mutation
/// invalidation. The caller owns serialization, locking, and persistence.
/// With `keys == None`, only known disk-backed sections are removed; unrelated
/// metadata is preserved just like the Python probe service.
pub fn invalidate_sections(
    document: &Value,
    keys: Option<&[&str]>,
    updated_at: f64,
) -> Option<Value> {
    let mut document = document.as_object()?.clone();
    let sections = document.get_mut("sections")?.as_object_mut()?;
    let remove_all = keys.is_none();
    let requested = keys.map(|keys| {
        keys.iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
    });
    let before = sections.len();
    sections.retain(|key, _| {
        if remove_all {
            !disk_ttl().contains_key(key.as_str())
        } else {
            !requested
                .as_ref()
                .is_some_and(|keys| keys.contains(key.as_str()))
        }
    });
    if sections.len() == before {
        return None;
    }
    document.insert("generated_at".into(), Value::from(updated_at));
    Some(Value::Object(document))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn missing_cache_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe-cache.json");
        assert_eq!(read_section_in("bootc-branch", &[path]), None);
    }

    #[test]
    fn unknown_key_returns_none() {
        assert_eq!(read_section_in("not-a-real-key", &[]), None);
    }

    #[test]
    fn reads_a_fresh_cache_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe-cache.json");
        let now = now_unix();
        let doc = json!({
            "version": 2, "generated_at": now,
            "sections": { "bootc-branch": { "ts": now, "data": "testing" } },
        });
        fs::write(&path, serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(
            read_section_in("bootc-branch", &[path]),
            Some(json!("testing"))
        );
    }

    #[test]
    fn stale_entry_past_its_ttl_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe-cache.json");
        // bootc-branch's TTL is 90s.
        let old = now_unix() - 200.0;
        let doc = json!({
            "version": 2, "generated_at": old,
            "sections": { "bootc-branch": { "ts": old, "data": "testing" } },
        });
        fs::write(&path, serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(read_section_in("bootc-branch", &[path]), None);
    }

    #[test]
    fn picks_the_freshest_entry_across_candidate_paths() {
        let dir = tempfile::tempdir().unwrap();
        let now = now_unix();
        let older_path = dir.path().join("older.json");
        let newer_path = dir.path().join("newer.json");
        fs::write(
            &older_path,
            serde_json::to_string(&json!({
                "version": 2, "sections": { "bootc-branch": { "ts": now - 50.0, "data": "stale-value" } },
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            &newer_path,
            serde_json::to_string(&json!({
                "version": 2, "sections": { "bootc-branch": { "ts": now, "data": "fresh-value" } },
            }))
            .unwrap(),
        )
        .unwrap();
        // Deliberately listed newest-last, to prove this picks the freshest
        // entry rather than the first match.
        assert_eq!(
            read_section_in("bootc-branch", &[older_path, newer_path]),
            Some(json!("fresh-value"))
        );
    }

    #[test]
    fn invalidates_selected_sections_without_touching_unrelated_cache_data() {
        let document = json!({
            "version": 2,
            "generated_at": 1,
            "sections": {
                "controllers-detect": {"ts": 1, "data": []},
                "custom": {"ts": 1, "data": "keep"}
            }
        });
        let updated = invalidate_sections(&document, Some(&["controllers-detect"]), 42.0).unwrap();
        assert!(updated["sections"].get("controllers-detect").is_none());
        assert_eq!(updated["sections"]["custom"]["data"], "keep");
        assert_eq!(updated["generated_at"], 42.0);
        assert!(invalidate_sections(&updated, Some(&["controllers-detect"]), 43.0).is_none());
    }

    #[test]
    fn invalidates_all_known_sections_but_preserves_unknown_sections() {
        let document = json!({"sections":{"audit-cache":{},"custom":{}},"metadata":"keep"});
        let updated = invalidate_sections(&document, None, 9.0).unwrap();
        assert!(updated["sections"].get("audit-cache").is_none());
        assert!(updated["sections"].get("custom").is_some());
        assert_eq!(updated["metadata"], "keep");
    }

    #[test]
    fn writes_a_cache_document_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe-cache.json");
        let document = json!({"sections":{"bootc-branch":{"ts":1,"data":"testing"}}});
        write_cache_file(&path, &document).unwrap();
        assert_eq!(load_cache_file(&path), Some(document));
    }

    #[test]
    fn updates_sections_in_the_existing_cache_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe-cache.json");
        let mut sections = serde_json::Map::new();
        sections.insert("bootc-branch".into(), json!("testing"));
        update_sections(&sections, Some(&path), false).unwrap();
        let written = load_cache_file(&path).unwrap();
        assert_eq!(written["version"], 2);
        assert_eq!(written["sections"]["bootc-branch"]["data"], "testing");
    }

    #[test]
    fn update_sections_writes_an_atomic_versioned_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe-cache.json");
        let mut sections = serde_json::Map::new();
        sections.insert("network-summary".into(), json!({"vpn_connected": false}));
        update_sections(&sections, Some(&path), false).unwrap();
        let written: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(written["version"], 2);
        assert!(written["generated_at"].as_f64().unwrap() > 0.0);
        assert_eq!(
            written["sections"]["network-summary"]["data"]["vpn_connected"],
            false
        );
    }
}
