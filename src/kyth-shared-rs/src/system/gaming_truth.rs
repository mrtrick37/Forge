//! Offline gaming compatibility data and library classification.
//!
//! This ports the deterministic portion of the Truth Engine: compatibility
//! payload parsing, Steam manifest discovery, normalized lookup, and bucketed
//! classification. Remote refresh and UI ownership stay outside this crate.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatGame {
    pub name: String,
    pub anticheat: String,
    pub status: String,
    pub note: String,
    pub checked: String,
    pub source: String,
    pub source_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatStats {
    pub works: usize,
    pub blocked: usize,
    pub total: usize,
    pub oldest_check: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryGame {
    pub name: String,
    pub status: String,
    pub anticheat: String,
    pub note: String,
    pub checked: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryClassification {
    pub total: usize,
    pub works: usize,
    pub blocked: usize,
    pub buckets: BTreeMap<String, Vec<LibraryGame>>,
    pub summary: String,
}

pub fn normalize_name(name: &str) -> String {
    let mut normalized = String::with_capacity(name.len());
    let mut pending_space = false;
    for character in name.trim().to_ascii_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            if pending_space && !normalized.is_empty() {
                normalized.push(' ');
            }
            pending_space = false;
            normalized.push(character);
        } else {
            pending_space = true;
        }
    }
    normalized
}

pub fn parse_compat_payload(value: &Value) -> (String, Vec<CompatGame>) {
    let Some(object) = value.as_object() else {
        return (String::new(), Vec::new());
    };
    let updated = object
        .get("updated")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let games = object
        .get("games")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let entry = entry.as_object()?;
            let name = entry
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())?;
            let status = entry
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !matches!(status, "native" | "proton" | "tweaks" | "blocked") {
                return None;
            }
            Some(CompatGame {
                name: name.into(),
                anticheat: entry
                    .get("anticheat")
                    .and_then(Value::as_str)
                    .unwrap_or("None")
                    .into(),
                status: status.into(),
                note: entry
                    .get("note")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into(),
                checked: entry
                    .get("checked")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into(),
                source: entry
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into(),
                source_url: entry
                    .get("source_url")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into(),
            })
        })
        .collect();
    (updated, games)
}

pub fn calculate_compat_stats(games: &[CompatGame]) -> CompatStats {
    CompatStats {
        works: games
            .iter()
            .filter(|game| matches!(game.status.as_str(), "native" | "proton" | "tweaks"))
            .count(),
        blocked: games.iter().filter(|game| game.status == "blocked").count(),
        total: games.len(),
        oldest_check: games
            .iter()
            .map(|game| game.checked.as_str())
            .min()
            .unwrap_or("unknown")
            .into(),
    }
}

pub fn build_compat_index(games: &[CompatGame]) -> BTreeMap<String, CompatGame> {
    let mut index = BTreeMap::new();
    for game in games {
        let key = normalize_name(&game.name);
        if !key.is_empty() {
            index.entry(key).or_insert_with(|| game.clone());
        }
    }
    index
}

pub fn classify_library(
    user_games: &[String],
    compat_games: &[CompatGame],
) -> LibraryClassification {
    let index = build_compat_index(compat_games);
    let mut buckets: BTreeMap<String, Vec<LibraryGame>> =
        ["native", "proton", "tweaks", "blocked", "unknown"]
            .into_iter()
            .map(|status| (status.into(), Vec::new()))
            .collect();
    for name in user_games {
        let game = index.get(&normalize_name(name));
        let row = game.map_or_else(
            || LibraryGame {
                name: name.clone(),
                status: "unknown".into(),
                anticheat: String::new(),
                note: "Not in Kyth list — check ProtonDB.".into(),
                checked: String::new(),
            },
            |game| LibraryGame {
                name: game.name.clone(),
                status: game.status.clone(),
                anticheat: game.anticheat.clone(),
                note: game.note.clone(),
                checked: game.checked.clone(),
            },
        );
        buckets.entry(row.status.clone()).or_default().push(row);
    }
    let total = user_games.len();
    let works =
        buckets.get("native").map_or(0, Vec::len) + buckets.get("proton").map_or(0, Vec::len);
    let blocked = buckets.get("blocked").map_or(0, Vec::len);
    let summary = if total == 0 {
        "No games to check — install Steam or paste your library below.".into()
    } else {
        format!("{works} of {total} should work; {blocked} blocked by vendor anti-cheat.")
    };
    LibraryClassification {
        total,
        works,
        blocked,
        buckets,
        summary,
    }
}

pub fn scan_steam_manifests(library_paths: Option<&[PathBuf]>) -> Vec<String> {
    let candidates: Vec<PathBuf> = library_paths.map_or_else(
        || {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/root"));
            vec![
                home.join(".steam/steam/steamapps"),
                home.join(".steam/steamapps"),
                home.join(".local/share/Steam/steamapps"),
            ]
        },
        |paths| paths.iter().map(|path| path.join("steamapps")).collect(),
    );
    let name_pattern = Regex::new(r#""name"\s+"([^"]+)""#).expect("static Steam manifest pattern");
    let mut names = Vec::new();
    for base in candidates {
        let Ok(entries) = std::fs::read_dir(base) else {
            continue;
        };
        for entry in entries.flatten() {
            let filename = entry.file_name().to_string_lossy().into_owned();
            if !filename.starts_with("appmanifest_") || !filename.ends_with(".acf") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            if let Some(name) = name_pattern
                .captures(&text)
                .and_then(|capture| capture.get(1))
            {
                names.push(name.as_str().trim().to_string());
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    names
        .into_iter()
        .filter(|name| seen.insert(normalize_name(name)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn game(name: &str, status: &str) -> CompatGame {
        CompatGame {
            name: name.into(),
            anticheat: "None".into(),
            status: status.into(),
            note: "tested".into(),
            checked: "2026-01-01".into(),
            source: "Kyth".into(),
            source_url: String::new(),
        }
    }

    #[test]
    fn normalizes_punctuation_and_whitespace() {
        assert_eq!(normalize_name("  Star-Wars:  Jedi!  "), "star wars jedi");
    }

    #[test]
    fn parses_payload_and_calculates_stats() {
        let payload = serde_json::json!({
            "updated": "2026-08-01",
            "games": [
                {"name":"Native", "status":"native"},
                {"name":"Blocked", "status":"blocked"},
                {"name":"Ignore", "status":"unknown"}
            ]
        });
        let (updated, games) = parse_compat_payload(&payload);
        assert_eq!(updated, "2026-08-01");
        assert_eq!(games.len(), 2);
        assert_eq!(calculate_compat_stats(&games).blocked, 1);
    }

    #[test]
    fn classifies_known_and_unknown_library_entries() {
        let games = vec![
            game("Portal 2", "proton"),
            game("Native Game", "native"),
            game("Blocked Game", "blocked"),
        ];
        let user = vec!["Portal 2".into(), "Native-Game".into(), "Mystery".into()];
        let result = classify_library(&user, &games);
        assert_eq!(result.total, 3);
        assert_eq!(result.works, 2);
        assert_eq!(result.blocked, 0);
        assert_eq!(
            result.buckets["unknown"][0].note,
            "Not in Kyth list — check ProtonDB."
        );
    }

    #[test]
    fn scans_acf_manifests_and_deduplicates_normalized_names() {
        let directory = tempdir().unwrap();
        let steamapps = directory.path().join("steamapps");
        std::fs::create_dir_all(&steamapps).unwrap();
        std::fs::write(
            steamapps.join("appmanifest_1.acf"),
            "\"name\" \"Portal 2\"\n",
        )
        .unwrap();
        std::fs::write(
            steamapps.join("appmanifest_2.acf"),
            "\"name\" \"Portal-2\"\n",
        )
        .unwrap();
        std::fs::write(steamapps.join("not-a-game.txt"), "\"name\" \"Ignored\"\n").unwrap();
        let roots = vec![directory.path().to_path_buf()];
        let names = scan_steam_manifests(Some(&roots));
        assert_eq!(names.len(), 1);
        assert_eq!(normalize_name(&names[0]), "portal 2");
    }
}
