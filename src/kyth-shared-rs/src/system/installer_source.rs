//! Pure image-source reference helpers for the installer.
//!
//! This ports the reference and classification layer of
//! `kyth_installer.imagesrc`. Filesystem verification, DNS/network
//! preflight, image downloads, and bootc installation remain caller-owned.

use std::fs;
use std::path::Path;

use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSource {
    pub source_ref: String,
    pub target_ref: String,
    pub kind: String,
    pub digest: String,
    pub verified: bool,
}

impl ImageSource {
    pub fn requires_network(&self) -> bool {
        image_requires_network(&self.source_ref)
    }
}

/// Add the default Docker transport to a user/configured image reference.
pub fn source_image_ref(image: &str, default_source: &str) -> String {
    let image = image.trim();
    if image.is_empty() {
        return default_source.to_string();
    }
    if ["docker://", "containers-storage:", "oci:", "ostree:"]
        .iter()
        .any(|prefix| image.starts_with(prefix))
    {
        image.to_string()
    } else {
        format!("docker://{image}")
    }
}

pub fn image_requires_network(image_ref: &str) -> bool {
    image_ref.starts_with("docker://")
}

/// Extract the registry host using the same conservative split rules as the
/// Python preflight path. This is not URL parsing and intentionally does not
/// perform DNS or socket access.
pub fn registry_host(image_ref: &str) -> String {
    let image = image_ref.strip_prefix("docker://").unwrap_or(image_ref);
    let host = image.split_once('/').map_or(image, |(host, _)| host);
    let host = host.split_once('@').map_or(host, |(host, _)| host);
    host.rsplit_once(':')
        .map_or(host, |(host, _)| host)
        .to_string()
}

/// Split an OCI transport reference into `(layout_path, tag)`.
pub fn oci_layout_ref(image_ref: &str) -> (String, String) {
    let value = image_ref.strip_prefix("oci:").unwrap_or(image_ref);
    let slash = value.rfind('/');
    let colon = value.rfind(':');
    if let Some(colon) = colon.filter(|colon| slash.is_none_or(|slash| *colon > slash)) {
        let tag = if colon + 1 < value.len() {
            value[colon + 1..].to_string()
        } else {
            "latest".to_string()
        };
        (value[..colon].to_string(), tag)
    } else {
        (value.to_string(), "latest".to_string())
    }
}

/// Derive source and target image references for the selected kernel.
pub fn install_images(kernel: &str, source_image: &str, target_image: &str) -> (String, String) {
    if kernel == "fedora" {
        return (
            source_image_ref(source_image, source_image),
            target_image.to_string(),
        );
    }
    let (registry, tag) = target_image.rsplit_once(':').map_or_else(
        || (target_image, "latest"),
        |(registry, tag)| (registry, tag),
    );
    let base_tag = tag.strip_suffix("-cachy").unwrap_or(tag);
    let image = format!("{registry}:{base_tag}-cachy");
    (format!("docker://{image}"), image)
}

/// Classify a resolved reference without claiming that it has been verified.
/// Callers pass the release-pinned digest after their own verification policy.
pub fn classify_source_refs(
    source_ref: &str,
    target_ref: &str,
    digest: &str,
    verified: bool,
) -> ImageSource {
    let kind = if source_ref.starts_with("oci:") {
        "embedded"
    } else if source_ref.starts_with("containers-storage:") || source_ref.starts_with("ostree:") {
        "local"
    } else {
        "network"
    };
    ImageSource {
        source_ref: source_ref.to_string(),
        target_ref: target_ref.to_string(),
        kind: kind.to_string(),
        digest: digest.to_string(),
        verified,
    }
}

fn read_json_file(path: &Path, label: &str) -> Result<Value, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("could not read {label}: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err(format!("{label} is missing or unsafe: {}", path.display()));
    }
    let contents =
        fs::read_to_string(path).map_err(|error| format!("could not read {label}: {error}"))?;
    serde_json::from_str(&contents).map_err(|error| format!("could not parse {label}: {error}"))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not read manifest blob: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("embedded OCI manifest blob is missing or unsafe".to_string());
    }
    let contents =
        fs::read(path).map_err(|error| format!("could not read manifest blob: {error}"))?;
    let digest = Sha256::digest(contents);
    Ok(format!("sha256:{digest:x}"))
}

fn valid_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex.bytes().all(|byte| {
            byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) || (b'A'..=b'F').contains(&byte)
        })
}

/// Verify an embedded OCI layout against a release-pinned digest.
///
/// This is intentionally a read-only verifier. `expected_digest` and
/// `expected_target` are supplied by the release/build caller so the shared
/// crate does not embed deployment-specific image constants.
pub fn verify_oci_source(
    image_ref: &str,
    expected_digest: Option<&str>,
    metadata_path: &Path,
    expected_target: Option<&str>,
) -> Result<String, String> {
    let (root, tag) = oci_layout_ref(image_ref);
    let root = Path::new(&root);
    let root_metadata = fs::symlink_metadata(root)
        .map_err(|error| format!("could not read embedded OCI image: {error}"))?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(format!(
            "embedded OCI layout is missing or unsafe: {}",
            root.display()
        ));
    }
    let layout = read_json_file(&root.join("oci-layout"), "embedded OCI layout")?;
    let index = read_json_file(&root.join("index.json"), "embedded OCI index")?;
    if layout.get("imageLayoutVersion").and_then(Value::as_str) != Some("1.0.0") {
        return Err("embedded OCI image has an unsupported layout version".to_string());
    }

    let manifests = index
        .get("manifests")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let descriptor = manifests
        .iter()
        .find(|item| {
            item.get("annotations")
                .and_then(|annotations| annotations.get("org.opencontainers.image.ref.name"))
                .and_then(Value::as_str)
                == Some(tag.as_str())
        })
        .or_else(|| (manifests.len() == 1).then(|| &manifests[0]));
    let digest = descriptor
        .and_then(|item| item.get("digest"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !valid_sha256_digest(digest) {
        return Err("embedded OCI image has no valid manifest digest".to_string());
    }
    let blob = root
        .join("blobs")
        .join("sha256")
        .join(&digest["sha256:".len()..]);
    if sha256_file(&blob)? != digest {
        return Err("embedded OCI manifest failed its SHA-256 integrity check".to_string());
    }

    let metadata = read_json_file(metadata_path, "source metadata")?;
    if metadata.get("schema_version").and_then(Value::as_i64) != Some(1) {
        return Err("embedded-image metadata has an unsupported schema".to_string());
    }
    let metadata_digest = metadata
        .get("digest")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let configured_digest = expected_digest
        .filter(|value| !value.is_empty())
        .unwrap_or(metadata_digest);
    if configured_digest.is_empty() || configured_digest != digest || metadata_digest != digest {
        return Err(
            "embedded OCI image does not match the digest pinned by this ISO release".to_string(),
        );
    }
    if let Some(expected_target) = expected_target {
        let metadata_target = metadata
            .get("target_image")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !metadata_target.is_empty() && metadata_target != expected_target {
            return Err(
                "embedded-image metadata does not match the configured update target".to_string(),
            );
        }
    }
    Ok(digest.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn normalizes_transports_and_network_classification() {
        assert_eq!(
            source_image_ref("", "oci:/embedded:latest"),
            "oci:/embedded:latest"
        );
        assert_eq!(
            source_image_ref("registry/kyth:latest", "unused"),
            "docker://registry/kyth:latest"
        );
        assert_eq!(source_image_ref(" ostree:kyth", "unused"), "ostree:kyth");
        assert!(image_requires_network("docker://registry/kyth:latest"));
        assert!(!image_requires_network("oci:/embedded:latest"));
    }

    #[test]
    fn extracts_registry_host_and_oci_tag() {
        assert_eq!(
            registry_host("docker://registry.example:5443/kyth/os@sha256:abc"),
            "registry.example"
        );
        assert_eq!(
            registry_host("docker://registry.example/kyth:latest"),
            "registry.example"
        );
        assert_eq!(
            oci_layout_ref("oci:/run/media/kyth/image:v2"),
            ("/run/media/kyth/image".to_string(), "v2".to_string())
        );
        assert_eq!(
            oci_layout_ref("oci:/run/media/kyth/image"),
            ("/run/media/kyth/image".to_string(), "latest".to_string())
        );
    }

    #[test]
    fn derives_cachyos_images_without_double_suffix() {
        assert_eq!(
            install_images("cachyos", "unused", "registry/kyth:stable"),
            (
                "docker://registry/kyth:stable-cachy".to_string(),
                "registry/kyth:stable-cachy".to_string()
            )
        );
        assert_eq!(
            install_images("cachyos", "unused", "registry/kyth:stable-cachy"),
            (
                "docker://registry/kyth:stable-cachy".to_string(),
                "registry/kyth:stable-cachy".to_string()
            )
        );
    }

    #[test]
    fn classifies_source_and_preserves_verification_inputs() {
        let source = classify_source_refs("oci:/image:latest", "target", "sha256:abc", true);
        assert_eq!(source.kind, "embedded");
        assert_eq!(source.digest, "sha256:abc");
        assert!(source.verified);
        assert!(!source.requires_network());
    }

    #[test]
    fn verifies_embedded_oci_layout_without_mutating_it() {
        let temp = tempdir().expect("temporary OCI directory");
        let root = temp.path().join("image");
        let blob_dir = root.join("blobs/sha256");
        fs::create_dir_all(&blob_dir).expect("OCI blob directory");
        fs::write(root.join("oci-layout"), r#"{"imageLayoutVersion":"1.0.0"}"#)
            .expect("OCI layout");
        let manifest = br#"{"schemaVersion":2,"config":{}}"#;
        let digest = Sha256::digest(manifest);
        let digest = format!("sha256:{digest:x}");
        fs::write(blob_dir.join(&digest["sha256:".len()..]), manifest).expect("manifest blob");
        fs::write(
            root.join("index.json"),
            format!(
                r#"{{"manifests":[{{"digest":"{digest}","annotations":{{"org.opencontainers.image.ref.name":"stable"}}}}]}}"#
            ),
        )
        .expect("OCI index");
        let metadata = temp.path().join("source.json");
        fs::write(
            &metadata,
            format!(r#"{{"schema_version":1,"digest":"{digest}","target_image":"registry/kyth:stable"}}"#),
        )
        .expect("source metadata");

        let image_ref = format!("oci:{}:stable", root.display());
        assert_eq!(
            verify_oci_source(
                &image_ref,
                Some(&digest),
                &metadata,
                Some("registry/kyth:stable"),
            )
            .expect("OCI source should verify"),
            digest
        );
    }

    #[test]
    fn rejects_oci_digest_or_metadata_mismatches() {
        let temp = tempdir().expect("temporary OCI directory");
        let root = temp.path().join("missing");
        let metadata = temp.path().join("source.json");
        fs::write(&metadata, r#"{"schema_version":1,"digest":"sha256:bad"}"#).expect("metadata");
        let error = verify_oci_source(
            &format!("oci:{}:latest", root.display()),
            Some("sha256:bad"),
            &metadata,
            None,
        )
        .expect_err("missing layout must fail closed");
        assert!(error.contains("embedded OCI image"));
    }

    #[test]
    fn rejects_path_shaped_manifest_digests_before_blob_lookup() {
        assert!(!valid_sha256_digest("sha256:../../../../etc/passwd"));
        assert!(!valid_sha256_digest("sha256:ABC"));
        assert!(valid_sha256_digest(&format!("sha256:{}", "a".repeat(64))));
    }
}
