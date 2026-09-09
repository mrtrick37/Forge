//! Offline AppStream cache status.

use std::path::{Path, PathBuf};

pub fn cache_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if let Some(cache) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(cache).join("kyth-appstream.json");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
        .join(".cache/kyth-appstream.json")
}

pub fn warm_status(path: impl AsRef<Path>) -> &'static str {
    let path = path.as_ref();
    if path.is_file() {
        if std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .is_some_and(|value| json_truthy(&value))
        {
            return "cached";
        }
    }
    "live"
}

fn json_truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(false) => false,
        serde_json::Value::Number(number) => {
            number.as_i64().is_some_and(|n| n != 0)
                || number.as_u64().is_some_and(|n| n != 0)
                || number.as_f64().is_some_and(|n| n != 0.0)
        }
        serde_json::Value::String(text) => !text.is_empty(),
        serde_json::Value::Array(items) => !items.is_empty(),
        serde_json::Value::Object(items) => !items.is_empty(),
        serde_json::Value::Bool(true) => true,
    }
}

pub fn appstore_status(path: impl AsRef<Path>) -> &'static str {
    if path.as_ref().is_file() {
        "UNAVAILABLE (cached)"
    } else {
        "empty"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn distinguishes_cached_empty_and_missing() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("appstream.json");
        assert_eq!(warm_status(&path), "live");
        assert_eq!(appstore_status(&path), "empty");
        fs::write(&path, r#"{"apps":["org.example.App"]}"#).unwrap();
        assert_eq!(warm_status(&path), "cached");
        assert_eq!(appstore_status(&path), "UNAVAILABLE (cached)");
        fs::write(&path, "[]").unwrap();
        assert_eq!(warm_status(&path), "live");
    }
}
