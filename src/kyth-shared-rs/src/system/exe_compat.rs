//! Offline EXE compatibility lookup by bounded SHA-256 or filename.

use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

pub const DEFAULT_COMPAT_PATH: &str = "/usr/share/kyth/compat.json";
const HASH_BYTES: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CompatResult {
    pub status: String,
    pub runner: String,
    pub reason: String,
}

pub fn normalise_filename(filename: &str) -> String {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let separators = Regex::new(r"[\s.]+").expect("static filename separator pattern");
    let wrapper = Regex::new(r"^(setup|install|installer|update|updater|launcher)[-_]+")
        .expect("static wrapper prefix pattern");
    let wrapper_suffix = Regex::new(r"[-_]+(setup|install|installer|update|updater|launcher)$")
        .expect("static wrapper suffix pattern");
    let token = Regex::new(r"[-_]+(x64|x86|x86_64|amd64|win64|win32|windows|pc|arm64|online|offline|stable|v?\d[\d.]*)$").expect("static release token pattern");
    let mut stem = separators.replace_all(&stem, "-").into_owned();
    for _ in 0..4 {
        let old = stem.clone();
        stem = wrapper.replace(&stem, "").into_owned();
        stem = wrapper_suffix.replace(&stem, "").into_owned();
        stem = token.replace(&stem, "").into_owned();
        if stem == old {
            break;
        }
    }
    stem
}

pub fn is_rpm_installer(filename: &str) -> bool {
    filename.to_ascii_lowercase().ends_with(".rpm")
}

pub fn rewrite_steam_exec(exec_line: &str) -> Option<String> {
    let target = exec_line.split_once('=')?.1.trim();
    let game_pattern = Regex::new(r"steam://rungameid/([0-9]+)").expect("static Steam URI pattern");
    if let Some(capture) = game_pattern
        .captures(target)
        .and_then(|capture| capture.get(1))
    {
        return Some(format!(
            "Exec=flatpak run com.valvesoftware.Steam steam://rungameid/{}",
            capture.as_str()
        ));
    }
    let app_pattern = Regex::new(r"-applaunch\s+([0-9]+)").expect("static Steam applaunch pattern");
    app_pattern
        .captures(target)
        .and_then(|capture| capture.get(1))
        .map(|capture| {
            format!(
                "Exec=flatpak run com.valvesoftware.Steam steam://rungameid/{}",
                capture.as_str()
            )
        })
}

pub fn load_compat(path: impl AsRef<Path>) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| serde_json::json!({"entries": {}}))
}

fn bounded_hash(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes[..bytes.len().min(HASH_BYTES)]);
    Some(format!("{:x}", hasher.finalize())[..12].to_string())
}

fn entry_result(entry: &Value) -> CompatResult {
    CompatResult {
        status: entry
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("Works")
            .to_string(),
        runner: entry
            .get("runner")
            .and_then(Value::as_str)
            .unwrap_or("Wine")
            .to_string(),
        reason: entry
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    }
}

pub fn check_exe(path: impl AsRef<Path>, compat: &Value) -> CompatResult {
    let path = path.as_ref();
    let entries = compat.get("entries").and_then(Value::as_object);
    let hash = bounded_hash(path);
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if let Some(entry) = hash
        .as_deref()
        .and_then(|key| entries.and_then(|entries| entries.get(key)))
        .or_else(|| entries.and_then(|entries| entries.get(&filename)))
    {
        return entry_result(entry);
    }
    for marker in ["easyanticheat", "eac", "vgc", "battleye"] {
        if filename.contains(marker) {
            return CompatResult {
                status: "Blocked".to_string(),
                runner: "Anti-cheat".to_string(),
                reason: format!("Contains {marker} — blocked"),
            };
        }
    }
    CompatResult {
        status: "Works".to_string(),
        runner: "Bottles".to_string(),
        reason: "Offline DB: best-effort Wine".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn uses_filename_then_anticheat_fallback() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("game.exe");
        fs::write(&path, "demo").unwrap();
        let compat = serde_json::json!({"entries":{"game.exe":{"status":"Gold","runner":"Proton","reason":"tested"}}});
        assert_eq!(check_exe(&path, &compat).status, "Gold");
        let blocked = directory.path().join("easyanticheat.exe");
        fs::write(&blocked, "demo").unwrap();
        assert_eq!(
            check_exe(&blocked, &serde_json::json!({"entries":{}})).status,
            "Blocked"
        );
    }

    #[test]
    fn normalizes_installer_names_and_rewrites_steam_launchers() {
        assert_eq!(
            normalise_filename("/tmp/Setup My.Game v1.2 x64.exe"),
            "my-game"
        );
        assert!(is_rpm_installer("driver.RPM"));
        assert_eq!(
            rewrite_steam_exec("Exec=steam -applaunch 123"),
            Some("Exec=flatpak run com.valvesoftware.Steam steam://rungameid/123".into())
        );
        assert_eq!(
            rewrite_steam_exec("Exec=steam://rungameid/456"),
            Some("Exec=flatpak run com.valvesoftware.Steam steam://rungameid/456".into())
        );
        assert_eq!(rewrite_steam_exec("Name=Game"), None);
    }
}
