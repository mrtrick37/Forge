//! Offline attestation bundle metadata.

use serde_json::Value;
use std::path::Path;

pub const DEFAULT_ATTEST_PATH: &str = "/usr/share/kyth/attest.json";

pub fn load(path: impl AsRef<Path>) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| serde_json::json!({"bundle":null,"verified":false}))
}

pub fn cached_verification(value: &Value) -> (bool, String) {
    let Some(bundle) = value
        .get("bundle")
        .filter(|bundle| !bundle.is_null() && bundle.as_str() != Some(""))
    else {
        return (false, "no bundle".into());
    };
    let verified = value
        .get("verified")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reason = value
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or(if verified {
            "offline cached"
        } else {
            "not verified"
        });
    let _ = bundle;
    (verified, reason.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn loads_and_reports_cached_attestation() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("attest.json");
        fs::write(&path, r#"{"bundle":"attest.sigstore","verified":true}"#).unwrap();
        assert_eq!(
            cached_verification(&load(&path)),
            (true, "offline cached".into())
        );
    }
}
