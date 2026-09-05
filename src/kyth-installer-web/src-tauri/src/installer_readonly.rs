//! Read-only installer inventories and source-image status.
//!
//! These probes intentionally use fixed executable paths and bounded output.
//! They are part of the privileged daemon's API surface, but they never write
//! to the target disk or trust values supplied by the frontend.

use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_PROBE_OUTPUT: usize = 4 * 1024 * 1024;

fn command_output(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    if output.stdout.len() > MAX_PROBE_OUTPUT || output.stderr.len() > MAX_PROBE_OUTPUT {
        return Err(format!("{program} returned too much output"));
    }
    if !output.status.success() {
        return Err(format!(
            "{program} failed with exit code {}: {}",
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("{program} returned non-UTF-8 output"))
}

fn command_lines(program: &str, args: &[&str]) -> Option<Vec<String>> {
    let output = command_output(program, args).ok()?;
    let values = output
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    (!values.is_empty()).then_some(values)
}

fn fallback_lines(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

pub(crate) fn timezones() -> Vec<String> {
    if let Some(values) = command_lines("/usr/bin/timedatectl", &["list-timezones"]) {
        return values;
    }
    let mut zones = BTreeSet::from(["UTC".to_string()]);
    for path in [
        "/usr/share/zoneinfo/zone1970.tab",
        "/usr/share/zoneinfo/zone.tab",
    ] {
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };
        for line in contents.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(zone) = line.split_whitespace().nth(2) {
                zones.insert(zone.to_string());
            }
        }
        if zones.len() > 1 {
            break;
        }
    }
    zones.into_iter().collect()
}

pub(crate) fn locales() -> Vec<String> {
    command_lines("/usr/bin/localectl", &["list-locales", "--no-pager"])
        .unwrap_or_else(|| fallback_lines(&["en_US.UTF-8"]))
}

pub(crate) fn keymaps() -> Vec<String> {
    command_lines("/usr/bin/localectl", &["list-keymaps", "--no-pager"])
        .unwrap_or_else(|| fallback_lines(&["us"]))
}

fn configured(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn source_reference() -> String {
    let image = configured("KYTH_SOURCE_IMAGE", "ghcr.io/kyth-os/kyth:latest");
    if ["docker://", "containers-storage:", "oci:", "ostree:"]
        .iter()
        .any(|prefix| image.starts_with(prefix))
    {
        image
    } else {
        format!("docker://{image}")
    }
}

fn target_reference() -> String {
    configured(
        "KYTH_TARGET_IMAGE",
        &configured("KYTH_SOURCE_IMAGE", "ghcr.io/kyth-os/kyth:latest"),
    )
}

fn oci_layout_parts(reference: &str) -> Option<(PathBuf, String)> {
    let value = reference.strip_prefix("oci:")?;
    let slash = value.rfind('/').unwrap_or(0);
    let colon = value.rfind(':');
    if colon.is_some_and(|colon| colon > slash) {
        let colon = colon?;
        Some((
            PathBuf::from(&value[..colon]),
            value[colon + 1..].to_string(),
        ))
    } else {
        Some((PathBuf::from(value), "latest".to_string()))
    }
}

fn regular_file(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect source metadata: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "source metadata is missing or unsafe: {}",
            path.display()
        ));
    }
    Ok(())
}

fn embedded_digest(reference: &str, target: &str) -> Result<String, String> {
    let (root, tag) = oci_layout_parts(reference)
        .ok_or_else(|| "embedded OCI image reference is invalid".to_string())?;
    let root_metadata = fs::symlink_metadata(&root)
        .map_err(|error| format!("could not inspect embedded OCI image: {error}"))?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(format!(
            "embedded OCI layout is missing or unsafe: {}",
            root.display()
        ));
    }
    let layout_path = root.join("oci-layout");
    regular_file(&layout_path)?;
    let layout: Value = serde_json::from_slice(
        &fs::read(&layout_path).map_err(|error| format!("could not read OCI layout: {error}"))?,
    )
    .map_err(|error| format!("embedded OCI layout is invalid: {error}"))?;
    if layout.get("imageLayoutVersion").and_then(Value::as_str) != Some("1.0.0") {
        return Err("embedded OCI image has an unsupported layout version".to_string());
    }
    let index_path = root.join("index.json");
    regular_file(&index_path)?;
    let index: Value = serde_json::from_slice(
        &fs::read(&index_path).map_err(|error| format!("could not read OCI index: {error}"))?,
    )
    .map_err(|error| format!("embedded OCI index is invalid: {error}"))?;
    let manifests = index
        .get("manifests")
        .and_then(Value::as_array)
        .ok_or_else(|| "embedded OCI index has no manifests".to_string())?;
    let descriptor = manifests
        .iter()
        .find(|item| {
            item.get("annotations")
                .and_then(Value::as_object)
                .and_then(|annotations| annotations.get("org.opencontainers.image.ref.name"))
                .and_then(Value::as_str)
                == Some(tag.as_str())
        })
        .or_else(|| (manifests.len() == 1).then(|| &manifests[0]))
        .ok_or_else(|| "embedded OCI image tag was not found".to_string())?;
    let digest = descriptor
        .get("digest")
        .and_then(Value::as_str)
        .filter(|digest| digest.starts_with("sha256:") && digest.len() == 71)
        .ok_or_else(|| "embedded OCI image has no valid manifest digest".to_string())?;
    let blob = root.join("blobs").join("sha256").join(&digest[7..]);
    regular_file(&blob)?;
    let calculated = command_output(
        "/usr/bin/sha256sum",
        &[blob
            .to_str()
            .ok_or_else(|| "OCI manifest path is not UTF-8".to_string())?],
    )?
    .split_whitespace()
    .next()
    .map(|value| format!("sha256:{value}"))
    .ok_or_else(|| "sha256sum returned no digest".to_string())?;
    if calculated != digest {
        return Err("embedded OCI manifest failed its SHA-256 integrity check".to_string());
    }

    let metadata_path = PathBuf::from(configured(
        "KYTH_SOURCE_METADATA",
        "/usr/share/kyth/image-source.json",
    ));
    regular_file(&metadata_path)?;
    let metadata: Value = serde_json::from_slice(
        &fs::read(&metadata_path)
            .map_err(|error| format!("could not read embedded-image metadata: {error}"))?,
    )
    .map_err(|error| format!("embedded-image metadata is invalid: {error}"))?;
    if metadata.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err("embedded-image metadata has an unsupported schema".to_string());
    }
    let metadata_digest = metadata
        .get("digest")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let configured_digest = configured("KYTH_SOURCE_DIGEST", metadata_digest);
    if configured_digest != digest || metadata_digest != digest {
        return Err(
            "embedded OCI image does not match the digest pinned by this ISO release".to_string(),
        );
    }
    if let Some(metadata_target) = metadata.get("target_image").and_then(Value::as_str) {
        if !metadata_target.is_empty() && metadata_target != target {
            return Err(
                "embedded-image metadata does not match the configured update target".to_string(),
            );
        }
    }
    Ok(digest.to_string())
}

fn source_status() -> Value {
    let source = source_reference();
    let target = target_reference();
    if source.starts_with("oci:") {
        return match embedded_digest(&source, &target) {
            Ok(digest) => serde_json::json!({
                "available": true,
                "kind": "embedded",
                "verified": true,
                "requires_network": false,
                "digest": digest,
                "target_ref": target,
                "message": "Verified image embedded in this ISO"
            }),
            Err(error) => serde_json::json!({
                "available": false,
                "kind": "invalid",
                "verified": false,
                "requires_network": false,
                "digest": "",
                "message": error
            }),
        };
    }
    if ["containers-storage:", "ostree:"]
        .iter()
        .any(|prefix| source.starts_with(prefix))
    {
        let digest = configured("KYTH_SOURCE_DIGEST", "");
        return serde_json::json!({
            "available": true,
            "kind": "local",
            "verified": !digest.is_empty(),
            "requires_network": false,
            "digest": digest,
            "target_ref": target,
            "message": "Local image selected"
        });
    }
    let digest = configured("KYTH_SOURCE_DIGEST", "");
    serde_json::json!({
        "available": true,
        "kind": "network",
        "verified": !digest.is_empty(),
        "requires_network": true,
        "digest": digest,
        "target_ref": target,
        "message": "Network image selected"
    })
}

pub(crate) fn config() -> Value {
    serde_json::json!({
        "source_image": configured("KYTH_SOURCE_IMAGE", "ghcr.io/kyth-os/kyth:latest"),
        "is_live": Path::new("/etc/kyth-installer.env").is_file(),
        "source": source_status()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_inventories_are_nonempty() {
        assert_eq!(fallback_lines(&["UTC"]), vec!["UTC"]);
        assert_eq!(fallback_lines(&["en_US.UTF-8"]), vec!["en_US.UTF-8"]);
        assert_eq!(fallback_lines(&["us"]), vec!["us"]);
    }

    #[test]
    fn source_reference_normalizes_registry_images() {
        assert_eq!(
            source_reference().starts_with("docker://") || source_reference().starts_with("oci:"),
            true
        );
    }

    #[test]
    fn source_status_is_support_safe_without_embedded_metadata() {
        let value = source_status();
        assert!(value.get("kind").and_then(Value::as_str).is_some());
        assert!(value.get("message").and_then(Value::as_str).is_some());
        assert!(value.get("digest").and_then(Value::as_str).is_some());
    }
}
