//! Offline application suggestions for Windows executable imports.
//!
//! The packaged JSON remains the data source, while the lookup and filename
//! handling are shared with Rust callers.  The file is embedded as a fallback
//! so a damaged or missing system copy behaves like the Python implementation
//! instead of making the desktop handler fail open with no suggestion.

use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_APP_DB_PATH: &str = "/usr/share/kyth/exe-handler-apps.json";

/// Normalize an installer filename before application-database lookup.
/// This mirrors `desktop.exe_handler.normalise_filename` and intentionally
/// strips only well-known wrapper, platform, and release suffixes.
pub fn normalise_filename(filename: &str) -> String {
    let path = PathBuf::from(filename);
    let basename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(filename);
    let stem = Path::new(basename)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(basename)
        .to_ascii_lowercase();
    let separator = RegexBuilder::new(r"[\s.]+")
        .build()
        .expect("valid filename separator regex");
    let mut stem = separator.replace_all(&stem, "-").into_owned();
    let wrapper = RegexBuilder::new(r"(?i)^(setup|install|installer|update|updater|launcher)[-_]+")
        .build()
        .expect("valid wrapper regex");
    let wrapper_suffix =
        RegexBuilder::new(r"(?i)[-_]+(setup|install|installer|update|updater|launcher)$")
            .build()
            .expect("valid wrapper suffix regex");
    let token = RegexBuilder::new(r"(?i)[-_]+(x64|x86|x86_64|amd64|win64|win32|windows|pc|arm64|online|offline|stable|v?\d[\d.]*)$").build().expect("valid installer token regex");
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

const EMBEDDED_APP_DB: &str = include_str!("../../../../build_files/exe-handler-apps.json");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSuggestion {
    pub pattern: String,
    pub app_name: String,
    pub suggestion: String,
    pub flatpak_id: Option<String>,
}

fn parse_db(text: &str) -> Option<Vec<AppSuggestion>> {
    let entries =
        serde_json::from_str::<Vec<(String, String, String, Option<String>)>>(text).ok()?;
    Some(
        entries
            .into_iter()
            .map(
                |(pattern, app_name, suggestion, flatpak_id)| AppSuggestion {
                    pattern,
                    app_name,
                    suggestion,
                    flatpak_id,
                },
            )
            .collect(),
    )
}

fn embedded_app_db() -> Vec<AppSuggestion> {
    parse_db(EMBEDDED_APP_DB).expect("packaged application suggestion database must be valid")
}

pub fn load_app_db(path: impl AsRef<Path>) -> Vec<AppSuggestion> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| parse_db(&text))
        .unwrap_or_else(embedded_app_db)
}

fn matches_pattern(pattern: &str, stem: &str) -> bool {
    if let Ok(regex) = RegexBuilder::new(pattern).case_insensitive(true).build() {
        return regex.is_match(stem);
    }

    // The shipped database has one Python-only negative look-ahead:
    // `visual.?studio(?!.*code)`. Preserve that rule explicitly because the
    // Rust regex engine intentionally rejects look-around syntax.
    if pattern == r"visual.?studio(?!.*code)" {
        let Ok(prefix) = RegexBuilder::new(r"visual.?studio")
            .case_insensitive(true)
            .build()
        else {
            return false;
        };
        return prefix
            .find_iter(stem)
            .any(|matched| !stem[matched.end()..].to_ascii_lowercase().contains("code"));
    }
    false
}

pub fn suggest_app(stem: &str, path: impl AsRef<Path>) -> Option<AppSuggestion> {
    load_app_db(path)
        .into_iter()
        .find(|entry| matches_pattern(&entry.pattern, stem))
}

pub fn suggest_default(stem: &str) -> Option<AppSuggestion> {
    suggest_app(stem, DEFAULT_APP_DB_PATH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn loads_packaged_fallback_and_matches_case_insensitively() {
        let result = suggest_app("WINWORD", "/no/such/apps.json").unwrap();
        assert_eq!(result.app_name, "Microsoft Word");
        assert_eq!(
            result.flatpak_id.as_deref(),
            Some("org.libreoffice.LibreOffice")
        );
    }

    #[test]
    fn loads_custom_json_and_ignores_invalid_regex_entries() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("apps.json");
        fs::write(
            &path,
            r#"[["[", "broken", "ignore", null], ["foobar", "Foo Bar", "Use Foo", "org.foo.Bar"]]"#,
        )
        .unwrap();

        let entries = load_app_db(&path);
        assert_eq!(entries.len(), 2);
        let result = suggest_app("foobar-installer", &path).unwrap();
        assert_eq!(result.app_name, "Foo Bar");
        assert_eq!(result.suggestion, "Use Foo");
    }

    #[test]
    fn preserves_database_order() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("apps.json");
        fs::write(
            &path,
            r#"[["game", "First", "first", null], ["game", "Second", "second", null]]"#,
        )
        .unwrap();
        assert_eq!(suggest_app("game", &path).unwrap().app_name, "First");
    }

    #[test]
    fn preserves_python_negative_lookahead_for_visual_studio() {
        let pattern = r"visual.?studio(?!.*code)";
        assert!(!matches_pattern(pattern, "Visual Studio Code"));
        assert!(matches_pattern(pattern, "Visual Studio"));
    }

    #[test]
    fn normalizes_installer_names_and_detects_rpm() {
        assert_eq!(normalise_filename("Setup_Discord-1.2.3-x64.exe"), "discord");
        assert_eq!(normalise_filename("steam_installer_win32.exe"), "steam");
        assert_eq!(normalise_filename("Package.rpm"), "package");
        assert!(is_rpm_installer("FOO.RPM"));
        assert!(!is_rpm_installer("foo.exe"));
    }
}
