//! Shared TOML configuration loading with Python-compatible defaults.

use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Load the first readable TOML file from explicit candidates, then the
/// standard user/system locations, merging its selected table over defaults.
/// Invalid or unreadable candidates are skipped just like `config.py`.
pub fn load_toml_config(
    filename: &str,
    defaults: &BTreeMap<String, Value>,
    section_name: Option<&str>,
    extra_candidates: &[PathBuf],
) -> BTreeMap<String, Value> {
    let mut candidates = extra_candidates.to_vec();
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()));
    candidates.push(home.join(".config/kyth").join(filename));
    candidates.push(Path::new("/etc/kyth").join(filename));
    for path in candidates {
        let Ok(raw) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(value) = raw.parse::<toml::Value>() else {
            continue;
        };
        let Some(table) = value.as_table() else {
            continue;
        };
        let selected = match section_name {
            Some(section) => match table.get(section) {
                Some(value) => value.as_table(),
                None => Some(table),
            },
            None => Some(table),
        };
        let Some(selected) = selected else {
            return defaults.clone();
        };
        let mut merged = defaults.clone();
        for (key, value) in selected {
            if let Ok(json) = serde_json::to_value(value) {
                merged.insert(key.clone(), json);
            }
        }
        return merged;
    }
    defaults.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn merges_selected_section_over_defaults() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("settings.toml");
        std::fs::write(
            &path,
            "[settings]\nname = \"native\"\ncount = 3\nignored = true\n",
        )
        .unwrap();
        let defaults = BTreeMap::from([
            ("name".into(), json!("default")),
            ("enabled".into(), json!(true)),
        ]);
        let loaded = load_toml_config("settings.toml", &defaults, Some("settings"), &[path]);
        assert_eq!(loaded.get("name"), Some(&json!("native")));
        assert_eq!(loaded.get("enabled"), Some(&json!(true)));
        assert_eq!(loaded.get("ignored"), Some(&json!(true)));
    }

    #[test]
    fn invalid_candidates_fall_back_to_copy_of_defaults() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("bad.toml");
        std::fs::write(&path, "not = [valid").unwrap();
        let defaults = BTreeMap::from([("enabled".into(), json!(false))]);
        let loaded = load_toml_config("missing.toml", &defaults, None, &[path]);
        assert_eq!(loaded, defaults);
    }

    #[test]
    fn non_table_requested_section_does_not_leak_top_level_values() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("scalar-section.toml");
        std::fs::write(&path, "settings = \"disabled\"\ntop_level = true\n").unwrap();
        let defaults = BTreeMap::from([("enabled".into(), json!(false))]);
        let loaded = load_toml_config("settings.toml", &defaults, Some("settings"), &[path]);
        assert_eq!(loaded, defaults);
    }
}
