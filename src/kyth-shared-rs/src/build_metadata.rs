//! Typed build and release metadata projections.
//!
//! These builders mirror the JSON shape emitted by the build scripts. They
//! only transform validated strings into metadata; file writes and workflow
//! orchestration remain outside the shared crate.

use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImageMetadataInput {
    pub image: String,
    pub digest: String,
    pub version: String,
    pub source_sha: String,
    pub github_sha: String,
    pub source_tag: String,
    pub workflow_run_id: String,
    pub workflow_run_attempt: String,
    pub upstream_base: String,
    pub build_base: String,
    pub proton_cachyos_version: String,
    pub thirdparty_versions_hash: String,
    pub umu_version: String,
    pub kernel_flavor: String,
}

pub fn image_metadata(input: &ImageMetadataInput) -> Value {
    json!({
        "image": input.image,
        "digest": input.digest,
        "version": input.version,
        "source_sha": input.source_sha,
        "github_sha": input.github_sha,
        "source_tag": input.source_tag,
        "workflow_run_id": input.workflow_run_id,
        "workflow_run_attempt": input.workflow_run_attempt,
        "materials": {
            "upstream_base": input.upstream_base,
            "build_base": input.build_base,
            "proton_cachyos_version": input.proton_cachyos_version,
            "thirdparty_versions_hash": input.thirdparty_versions_hash,
            "umu_version": input.umu_version,
            "kernel_flavor": input.kernel_flavor,
        },
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseMetadataInput {
    pub source_tag: String,
    pub iso_basename: String,
    pub channel_basename: String,
    pub sha256: String,
    pub attestation_url: String,
    pub immutable_tag: String,
    pub source_sha: String,
    pub source_image: String,
    pub source_digest: String,
    pub build_date: String,
    pub github_repository: String,
    pub github_run_id: String,
}

pub const RELEASE_ASSET_BASE_URL: &str = "https://pub-9a3cc72972ea44c4ae7504ee7cda1fa6.r2.dev";

pub fn release_metadata(input: &ReleaseMetadataInput) -> (Value, Value) {
    let immutable = json!({
        "iso": input.iso_basename,
        "sha256": input.sha256,
        "signature": format!("{}.sig", input.iso_basename),
        "bundle": format!("{}.bundle", input.iso_basename),
        "provenance": format!("{}.intoto.jsonl", input.iso_basename),
        "checksum": format!("{}-CHECKSUM", input.iso_basename),
        "attestation_url": input.attestation_url,
        "source_image": input.source_image,
        "source_image_digest": input.source_digest,
        "pinned_source_image": format!("{}@{}", input.source_image, input.source_digest),
        "source_commit": input.source_sha,
        "source_tag": input.source_tag,
        "build_date": input.build_date,
        "release_tag": input.immutable_tag,
        "download_url": format!("{}/{}", RELEASE_ASSET_BASE_URL, input.iso_basename),
        "workflow_run": format!("https://github.com/{}/actions/runs/{}", input.github_repository, input.github_run_id),
    });
    let mut channel = immutable.clone();
    if let Some(object) = channel.as_object_mut() {
        object.insert("iso".into(), input.channel_basename.clone().into());
        object.insert(
            "signature".into(),
            format!("{}.sig", input.channel_basename).into(),
        );
        object.insert(
            "bundle".into(),
            format!("{}.bundle", input.channel_basename).into(),
        );
        object.insert(
            "provenance".into(),
            format!("{}.intoto.jsonl", input.channel_basename).into(),
        );
        object.insert(
            "checksum".into(),
            format!("{}-CHECKSUM", input.channel_basename).into(),
        );
        object.insert(
            "download_url".into(),
            format!("{}/{}", RELEASE_ASSET_BASE_URL, input.channel_basename).into(),
        );
        object.insert(
            "immutable_download_url".into(),
            format!("{}/{}", RELEASE_ASSET_BASE_URL, input.iso_basename).into(),
        );
    }
    (immutable, channel)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupplyChainMetadataInput {
    pub image: String,
    pub tag: String,
    pub source_sha: String,
    pub source_github_sha: String,
    pub current_digest: String,
    pub current_version: String,
    pub previous_digest: String,
    pub base_name: String,
    pub base_digest: String,
    pub upstream_base: String,
    pub proton_cachyos_version: String,
    pub thirdparty_hash: String,
    pub kernel_flavor: String,
    pub rpm_manifest: String,
    pub sbom: String,
    pub notes: String,
}

pub fn supply_chain_metadata(input: &SupplyChainMetadataInput) -> Value {
    json!({
        "image": input.image,
        "tag": input.tag,
        "source_sha": input.source_sha,
        "github_sha": if input.source_github_sha.is_empty() { &input.source_sha } else { &input.source_github_sha },
        "current_digest": input.current_digest,
        "current_version": input.current_version,
        "previous_digest": input.previous_digest,
        "materials": {
            "build_base": format!("{}@{}", input.base_name, input.base_digest),
            "upstream_base": input.upstream_base,
            "proton_cachyos_version": input.proton_cachyos_version,
            "thirdparty_versions_hash": input.thirdparty_hash,
            "kernel_flavor": input.kernel_flavor,
        },
        "rpm_manifest": input.rpm_manifest,
        "sbom": input.sbom,
        "notes": input.notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_input() -> ImageMetadataInput {
        ImageMetadataInput {
            image: "ghcr.io/kyth-os/kyth".into(),
            digest: "sha256:abc".into(),
            version: "1.2".into(),
            source_sha: "source".into(),
            github_sha: "trigger".into(),
            source_tag: "testing".into(),
            workflow_run_id: "42".into(),
            workflow_run_attempt: "2".into(),
            upstream_base: "fedora".into(),
            build_base: "base".into(),
            proton_cachyos_version: "9".into(),
            thirdparty_versions_hash: "hash".into(),
            umu_version: "0.10".into(),
            kernel_flavor: "fedora".into(),
        }
    }

    #[test]
    fn image_metadata_keeps_provenance_materials_nested() {
        let value = image_metadata(&image_input());
        assert_eq!(value["github_sha"], "trigger");
        assert_eq!(value["materials"]["kernel_flavor"], "fedora");
    }

    #[test]
    fn release_metadata_has_immutable_and_channel_urls() {
        let input = ReleaseMetadataInput {
            source_tag: "testing".into(),
            iso_basename: "kyth-live-testing-abc.iso".into(),
            channel_basename: "kyth-live-testing.iso".into(),
            sha256: "deadbeef".into(),
            attestation_url: "https://example/attest".into(),
            immutable_tag: "iso-testing-abc".into(),
            source_sha: "source".into(),
            source_image: "ghcr.io/kyth-os/kyth".into(),
            source_digest: "sha256:abc".into(),
            build_date: "20260829".into(),
            github_repository: "kyth-os/kyth".into(),
            github_run_id: "42".into(),
        };
        let (immutable, channel) = release_metadata(&input);
        assert_eq!(immutable["iso"], "kyth-live-testing-abc.iso");
        assert_eq!(channel["iso"], "kyth-live-testing.iso");
        assert_eq!(channel["immutable_download_url"], immutable["download_url"]);
    }

    #[test]
    fn supply_chain_metadata_falls_back_to_source_sha() {
        let value = supply_chain_metadata(&SupplyChainMetadataInput {
            image: "image".into(),
            tag: "testing".into(),
            source_sha: "source".into(),
            source_github_sha: String::new(),
            current_digest: "new".into(),
            current_version: "1".into(),
            previous_digest: "old".into(),
            base_name: "fedora".into(),
            base_digest: "sha256:base".into(),
            upstream_base: "base".into(),
            proton_cachyos_version: "9".into(),
            thirdparty_hash: "hash".into(),
            kernel_flavor: "fedora".into(),
            rpm_manifest: "rpm.txt".into(),
            sbom: "sbom.json".into(),
            notes: "notes.md".into(),
        });
        assert_eq!(value["github_sha"], "source");
        assert_eq!(value["materials"]["build_base"], "fedora@sha256:base");
    }
}
