//! User-session configuration transforms and their best-effort file writes.
//!
//! The pure `update_*` renders mirror `kyth_shared.session`; the
//! `*_file` appliers and `enable_vscode_brave_wallet_prompts` own the
//! `kyth-vscode-wallet` launcher surface. `session.py` stays as the
//! Phase 3 fixture.

use std::path::{Path, PathBuf};

/// Set VS Code's password store without preserving malformed/non-object JSON.
pub fn update_code_argv(raw: Option<&str>) -> String {
    let mut value = raw
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    value.as_object_mut().unwrap().insert(
        "password-store".into(),
        serde_json::Value::String("kwallet5".into()),
    );
    format!(
        "{}\n",
        serde_json::to_string_pretty(&value).expect("JSON object serializes")
    )
}

/// Replace an existing Chromium/Brave password-store flag, or append one.
pub fn update_chromium_flags(raw: Option<&str>) -> String {
    let mut updated = Vec::new();
    let mut wrote = false;
    for line in raw.unwrap_or_default().lines() {
        let stripped = line.trim();
        if stripped.starts_with("--password-store=") || stripped.starts_with("password-store=") {
            if !wrote {
                updated.push("--password-store=kwallet5".to_string());
                wrote = true;
            }
        } else {
            updated.push(line.to_string());
        }
    }
    if !wrote {
        updated.push("--password-store=kwallet5".into());
    }
    format!("{}\n", updated.join("\n").trim_end())
}

/// VS Code's per-user arguments file.
pub fn code_argv_path(home: &Path) -> PathBuf {
    home.join(".config/Code/argv.json")
}

/// Every Brave/Chromium flags file the launcher rewrites, in order.
pub fn chromium_flags_paths(home: &Path) -> Vec<PathBuf> {
    [
        ".config/brave-flags.conf",
        ".config/BraveSoftware/Brave-Browser/brave-flags.conf",
        ".config/BraveSoftware/Brave-Browser/chrome-flags.conf",
        ".var/app/com.brave.Browser/config/brave-flags.conf",
        ".var/app/com.brave.Browser/config/chrome-flags.conf",
        ".var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser/brave-flags.conf",
        ".var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser/chrome-flags.conf",
    ]
    .iter()
    .map(|rel| home.join(rel))
    .collect()
}

/// Best-effort rewrite of VS Code's argv.json: missing parents are
/// created, a missing or unreadable file starts empty, and write
/// failures are swallowed.
pub fn write_code_argv_file(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let existing = std::fs::read_to_string(path).ok();
    let _ = std::fs::write(path, update_code_argv(existing.as_deref()));
}

/// Best-effort rewrite of one flags file.
pub fn write_chromium_flags_file(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let _ = std::fs::write(path, update_chromium_flags(Some(&existing)));
}

/// Enable KWallet integration for VS Code and Brave under home.
pub fn enable_vscode_brave_wallet_prompts(home: &Path) {
    write_code_argv_file(&code_argv_path(home));
    for flags_path in chromium_flags_paths(home) {
        write_chromium_flags_file(&flags_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_code_json_and_recovers_from_malformed_input() {
        assert!(update_code_argv(Some(r#"{"theme":"dark"}"#))
            .contains("\"password-store\": \"kwallet5\""));
        assert_eq!(
            update_code_argv(Some("bad json")),
            "{\n  \"password-store\": \"kwallet5\"\n}\n"
        );
    }

    #[test]
    fn de_duplicates_chromium_password_store_flags() {
        let output =
            update_chromium_flags(Some("--foo\n--password-store=basic\npassword-store=old\n"));
        assert_eq!(output.matches("password-store=").count(), 1);
        assert!(output.contains("--password-store=kwallet5"));
    }

    #[test]
    fn wallet_paths_cover_code_and_all_brave_variants() {
        let home = Path::new("/home/demo");
        assert_eq!(
            code_argv_path(home),
            PathBuf::from("/home/demo/.config/Code/argv.json")
        );
        let paths = chromium_flags_paths(home);
        assert_eq!(paths.len(), 7);
        assert!(paths.iter().all(|path| path.starts_with(home)));
    }

    #[test]
    fn enable_writes_all_files_and_is_idempotent() {
        let home = tempfile::tempdir().unwrap();
        enable_vscode_brave_wallet_prompts(home.path());
        enable_vscode_brave_wallet_prompts(home.path());
        let argv = std::fs::read_to_string(home.path().join(".config/Code/argv.json")).unwrap();
        assert!(argv.contains("kwallet5"));
        for path in chromium_flags_paths(home.path()) {
            let content = std::fs::read_to_string(&path).unwrap();
            assert_eq!(content.matches("password-store=").count(), 1, "{path:?}");
            assert!(content.contains("--password-store=kwallet5"));
        }
    }
}
