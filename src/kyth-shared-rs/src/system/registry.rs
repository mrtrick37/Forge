//! Port of `kyth_shared.system.registry` — skopeo/OCI helpers.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct UpdateCheckResult {
    pub state: String,
    pub detail: String,
    pub manifest_raw: Vec<u8>,
    /// True when the local status was readable but the independent remote
    /// manifest probe failed. Callers that have an authoritative fetch path
    /// (bootc) may continue in this degraded mode.
    pub remote_probe_failed: bool,
}

pub const REGISTRY_INSPECT_TIMEOUT: Duration = Duration::from_secs(30);

pub fn booted_image_digest(status_data: &Value) -> Option<String> {
    crate::system::bootc_query::image_digest_from_status(status_data, "booted")
}

pub fn amd64_manifest_entry(manifest: &Value) -> Option<Value> {
    let manifests = manifest.get("manifests")?.as_array()?;
    for entry in manifests {
        let plat = entry.get("platform")?;
        if plat.get("architecture")?.as_str() == Some("amd64")
            && plat.get("os")?.as_str() == Some("linux")
        {
            return Some(entry.clone());
        }
    }
    None
}

pub fn image_annotations(manifest: &Value) -> HashMap<String, String> {
    let mut ann: HashMap<String, String> = manifest
        .get("annotations")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    if !ann.contains_key("org.opencontainers.image.revision") {
        if let Some(entry) = amd64_manifest_entry(manifest) {
            if let Some(eann) = entry.get("annotations").and_then(|v| v.as_object()) {
                for (k, v) in eann {
                    if let Some(s) = v.as_str() {
                        ann.insert(k.clone(), s.to_string());
                    }
                }
            }
        }
    }
    ann
}

pub fn image_revision(ann: &HashMap<String, String>) -> String {
    ann.get("org.opencontainers.image.revision")
        .map(|s| s.chars().take(12).collect())
        .unwrap_or_default()
}

pub fn remote_digest_and_timestamp(raw: &[u8]) -> (Option<String>, String) {
    let manifest: Value = serde_json::from_slice(raw).unwrap_or(Value::Null);
    let mut ts = String::new();
    if let Some(ann) = manifest.get("annotations").and_then(|v| v.as_object()) {
        if let Some(raw_ts) = ann
            .get("org.opencontainers.image.created")
            .and_then(|v| v.as_str())
        {
            ts = raw_ts.to_string();
        }
    }
    let digest = if manifest
        .get("mediaType")
        .and_then(|v| v.as_str())
        .map(|s| s.ends_with("manifest.v1+json"))
        .unwrap_or(false)
    {
        Some(format!("sha256:{:x}", Sha256::digest(raw)))
    } else if manifest
        .get("manifests")
        .and_then(|v| v.as_array())
        .is_some()
    {
        amd64_manifest_entry(&manifest)
            .and_then(|e| {
                e.get("digest")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .filter(|s| s.starts_with("sha256:"))
    } else if manifest.get("config").is_some() && manifest.get("layers").is_some() {
        Some(format!("sha256:{:x}", Sha256::digest(raw)))
    } else {
        None
    };
    (digest, ts)
}

fn inspect_raw(ref_name: &str, timeout: Duration) -> Result<Vec<u8>, String> {
    let argv = vec![
        "skopeo",
        "inspect",
        "--raw",
        "--no-creds",
        &format!("docker://{ref_name}"),
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();
    match super::process::run_bounded(&argv, timeout) {
        Ok(output) if output.status.success() => Ok(output.stdout),
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(if detail.is_empty() {
                format!("Could not check {ref_name}.")
            } else {
                detail
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
            Err(format!("Timed out checking {ref_name}."))
        }
        Err(error) => Err(format!("Could not check {ref_name}: {error}")),
    }
}

pub fn check_registry_update(
    status_data: &Value,
    branch: &str,
    registry: &str,
) -> UpdateCheckResult {
    check_registry_update_with_timeout(status_data, branch, registry, REGISTRY_INSPECT_TIMEOUT)
}

pub fn check_registry_update_with_timeout(
    status_data: &Value,
    branch: &str,
    registry: &str,
    timeout: Duration,
) -> UpdateCheckResult {
    let Some(local_digest) = booted_image_digest(status_data) else {
        return UpdateCheckResult {
            state: "error".to_string(),
            detail: "Could not read the current booted image digest.".to_string(),
            manifest_raw: Vec::new(),
            remote_probe_failed: false,
        };
    };
    let reference = format!("{registry}:{branch}");
    let raw = match inspect_raw(&reference, timeout) {
        Ok(raw) => raw,
        Err(detail) => {
            return UpdateCheckResult {
                state: "error".to_string(),
                detail,
                manifest_raw: Vec::new(),
                remote_probe_failed: true,
            }
        }
    };
    let Some(remote_digest) = remote_digest_and_timestamp(&raw).0 else {
        return UpdateCheckResult {
            state: "error".to_string(),
            detail: format!("Could not parse manifest for {reference}."),
            manifest_raw: raw,
            remote_probe_failed: true,
        };
    };
    let detail = remote_digest_and_timestamp(&raw).1;
    UpdateCheckResult {
        state: if remote_digest == local_digest {
            "uptodate"
        } else {
            "available"
        }
        .to_string(),
        detail,
        manifest_raw: raw,
        remote_probe_failed: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn amd64() {
        let m = json!({"manifests":[{"platform":{"architecture":"amd64","os":"linux"},"digest":"sha256:abc"}]});
        assert!(amd64_manifest_entry(&m).is_some());
    }
    #[test]
    fn revision() {
        let mut ann = HashMap::new();
        ann.insert(
            "org.opencontainers.image.revision".to_string(),
            "abcdef1234567890".to_string(),
        );
        assert_eq!(image_revision(&ann), "abcdef123456");
    }

    #[test]
    fn hashes_single_arch_manifests_instead_of_fabricating_a_digest() {
        let raw = br#"{"mediaType":"application/vnd.oci.image.manifest.v1+json"}"#;
        let (digest, _) = remote_digest_and_timestamp(raw);
        let digest = digest.unwrap();
        assert!(digest.starts_with("sha256:"));
        assert_ne!(digest, "sha256:dummy");
    }

    #[test]
    fn missing_local_digest_is_not_a_degraded_remote_failure() {
        let result = check_registry_update_with_timeout(
            &serde_json::json!({}),
            "testing",
            "ghcr.io/kyth-os/kyth",
            Duration::ZERO,
        );
        assert_eq!(result.state, "error");
        assert!(!result.remote_probe_failed);
    }
}
