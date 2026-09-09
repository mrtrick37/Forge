//! Pure build and CI checks shared by local tooling and future Rust callers.
//!
//! These helpers project JSON and filesystem inputs into deterministic values.
//! They do not invoke Docker/GitHub, write reports, or decide CI exit codes.

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const LABEL_UPSTREAM: &str = "org.kyth.build.upstream-base";
pub const LABEL_FLAVOR: &str = "org.kyth.build.kernel-flavor";
pub const LABEL_KERNEL: &str = "org.kyth.build.cachyos-kernel-version";
pub const LABEL_SRC: &str = "org.kyth.build.base-src-hash";

const REQUIRED_LABELS: [&str; 4] = [LABEL_UPSTREAM, LABEL_FLAVOR, LABEL_KERNEL, LABEL_SRC];

fn text(value: Option<&Value>) -> String {
    value.map_or_else(String::new, |value| {
        value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string())
    })
}

fn number(value: Option<&Value>) -> Option<f64> {
    value.and_then(|value| match value {
        Value::Number(value) => value.as_f64(),
        Value::String(value) => value.parse::<f64>().ok(),
        _ => None,
    })
}

fn coverage_percent(report: &Value, filename: &str) -> Option<f64> {
    let files = report.get("files").and_then(Value::as_object)?;
    let alternate = if let Some(rest) = filename.strip_prefix("build_files/") {
        format!("src/{rest}")
    } else if let Some(rest) = filename.strip_prefix("src/") {
        format!("build_files/{rest}")
    } else {
        String::new()
    };
    let result = [
        Some(filename),
        (!alternate.is_empty()).then_some(alternate.as_str()),
    ]
    .into_iter()
    .flatten()
    .find_map(|name| {
        files
            .get(name)?
            .get("summary")
            .and_then(|summary| number(summary.get("percent_covered")))
    });
    result
}

/// Return sorted RPM NVRAs from a Syft-style SBOM document.
pub fn rpm_manifest(sbom: &Value) -> Vec<String> {
    let mut packages = Vec::new();
    for artifact in sbom
        .get("artifacts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if artifact.get("type").and_then(Value::as_str) != Some("rpm") {
            continue;
        }
        let Some(name) = artifact
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        let metadata = artifact.get("metadata").and_then(Value::as_object);
        let arch = text(metadata.and_then(|metadata| metadata.get("architecture")));
        let metadata_version = text(metadata.and_then(|metadata| metadata.get("version")));
        let metadata_release = text(metadata.and_then(|metadata| metadata.get("release")));
        if !metadata_version.is_empty() && !metadata_release.is_empty() {
            packages.push(format!(
                "{name}-{metadata_version}-{metadata_release}.{arch}"
            ));
        } else {
            let version = text(artifact.get("version"));
            packages.push(if arch.is_empty() {
                format!("{name}-{version}")
            } else {
                format!("{name}-{version}.{arch}")
            });
        }
    }
    packages.sort();
    packages
}

pub fn render_rpm_manifest(sbom: &Value) -> String {
    format!("{}\n", rpm_manifest(sbom).join("\n"))
}

pub fn safe_release_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric())
        && tag.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '+' | '-')
        })
}

/// Validate the exact digest contract enforced by `validate-digest.sh`.
pub fn valid_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn copr_latest_nvr(payload: &Value) -> Option<String> {
    let package = payload
        .get("builds")?
        .get("latest_succeeded")?
        .get("source_package")?;
    let version = text(package.get("version")).trim().to_string();
    let release = text(package.get("release")).trim().to_string();
    let nvr = format!("{version}-{release}").trim_matches('-').to_string();
    (!nvr.is_empty()).then_some(nvr)
}

pub fn artifact_metrics(
    oci_manifest: Option<&Value>,
    rpm_manifest: Option<&str>,
) -> BTreeMap<String, u64> {
    let mut metrics = BTreeMap::new();
    if let Some(document) = oci_manifest {
        if let Some(manifests) = document
            .get("manifests")
            .and_then(Value::as_array)
            .filter(|items| !items.is_empty())
        {
            metrics.insert(
                "image_index_bytes".into(),
                manifests
                    .iter()
                    .map(|item| item.get("size").and_then(Value::as_u64).unwrap_or(0))
                    .sum(),
            );
        } else if let Some(layers) = document.get("layers").and_then(Value::as_array) {
            metrics.insert(
                "image_compressed_bytes".into(),
                layers
                    .iter()
                    .map(|item| item.get("size").and_then(Value::as_u64).unwrap_or(0))
                    .sum(),
            );
            metrics.insert("image_layer_count".into(), layers.len() as u64);
        }
    }
    if let Some(manifest) = rpm_manifest {
        let packages = manifest
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect::<std::collections::BTreeSet<_>>();
        metrics.insert("rpm_package_count".into(), packages.len() as u64);
    }
    metrics
}

/// Raise coverage floors to the whole-number portion of measured coverage.
/// Missing report entries are intentionally left unchanged.
pub fn raised_coverage_floors(
    floors: &BTreeMap<String, f64>,
    report: &Value,
) -> BTreeMap<String, f64> {
    floors
        .iter()
        .map(|(filename, floor)| {
            let actual = coverage_percent(report, filename);
            let raised = actual.map_or(*floor, |actual| (*floor).max(actual.trunc()));
            (filename.clone(), raised)
        })
        .collect()
}

/// Return critical-coverage failures without reading files or deciding the
/// process exit status.
///
/// `changed` mirrors the script's `--changed-only` set. The report lookup
/// accepts the packaged `build_files/` ↔ source-tree `src/` alias used by CI.
/// Missing report entries are failures; callers can present the returned
/// strings and choose their own exit policy.
pub fn coverage_failures(
    floors: &BTreeMap<String, f64>,
    report: &Value,
    changed: Option<&BTreeSet<String>>,
) -> Vec<String> {
    floors
        .iter()
        .filter(|(filename, _)| changed.is_none_or(|changed| changed.contains(*filename)))
        .filter_map(
            |(filename, minimum)| match coverage_percent(report, filename) {
                None => Some(format!("{filename}: absent from coverage report")),
                Some(actual) if actual < *minimum => {
                    Some(format!("{filename}: {actual:.1}% is below {minimum:.1}%"))
                }
                Some(_) => None,
            },
        )
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BaseReuseDecision {
    pub reuse: bool,
    pub digest: Option<String>,
    pub reason: String,
}

pub fn extract_digest(payload: &Value) -> Option<String> {
    [
        ("manifest", "digest"),
        ("Descriptor", "digest"),
        ("descriptor", "digest"),
    ]
    .into_iter()
    .find_map(|(outer, inner)| {
        payload
            .get(outer)?
            .get(inner)?
            .as_str()
            .filter(|value| value.starts_with("sha256:"))
            .map(str::to_owned)
    })
    .or_else(|| {
        payload
            .get("digest")
            .and_then(Value::as_str)
            .filter(|value| value.starts_with("sha256:"))
            .map(str::to_owned)
    })
}

pub fn extract_labels(payload: &Value) -> BTreeMap<String, String> {
    [payload.get("image"), payload.get("Image"), Some(payload)]
        .into_iter()
        .flatten()
        .filter_map(|image| image.as_object())
        .filter_map(|image| image.get("config").or_else(|| image.get("Config")))
        .filter_map(Value::as_object)
        .filter_map(|config| config.get("Labels").or_else(|| config.get("labels")))
        .filter_map(Value::as_object)
        .find(|labels| !labels.is_empty())
        .map(|labels| {
            labels
                .iter()
                .filter_map(|(key, value)| {
                    (!value.is_null()).then(|| (key.clone(), text(Some(value))))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn decide_base_reuse(
    labels: &BTreeMap<String, String>,
    digest: Option<&str>,
    upstream: &str,
    flavor: &str,
    kernel: &str,
    source_hash: &str,
) -> BaseReuseDecision {
    let expected = BTreeMap::from([
        (LABEL_UPSTREAM, upstream),
        (LABEL_FLAVOR, flavor),
        (LABEL_KERNEL, kernel),
        (LABEL_SRC, source_hash),
    ]);
    if REQUIRED_LABELS.iter().any(|key| !labels.contains_key(*key)) {
        return BaseReuseDecision {
            reuse: false,
            digest: digest.map(str::to_owned),
            reason: "missing-labels".into(),
        };
    }
    if expected
        .iter()
        .any(|(key, value)| labels.get(*key).map(String::as_str) != Some(*value))
    {
        return BaseReuseDecision {
            reuse: false,
            digest: digest.map(str::to_owned),
            reason: "label-mismatch".into(),
        };
    }
    if !digest.is_some_and(|digest| digest.starts_with("sha256:")) {
        return BaseReuseDecision {
            reuse: false,
            digest: digest.map(str::to_owned),
            reason: "missing-digest".into(),
        };
    }
    BaseReuseDecision {
        reuse: true,
        digest: digest.map(str::to_owned),
        reason: "match".into(),
    }
}

/// Stable SHA-256 over regular files under a build source root.
pub fn source_hash(root: impl AsRef<Path>) -> std::io::Result<String> {
    let root = root.as_ref().canonicalize()?;
    let mut files = Vec::new();
    collect_files(&root, &mut files)?;
    files.sort();
    let mut digest = Sha256::new();
    for path in files {
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        digest.update(relative.as_bytes());
        digest.update([0]);
        digest.update(std::fs::read(path)?);
        digest.update([0]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn collect_files(root: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn extracts_sorted_rpm_manifest_with_syft_fallbacks() {
        let sbom = serde_json::json!({"artifacts":[
            {"type":"rpm","name":"z","version":"1","metadata":{"architecture":"x86_64","version":"2","release":"3"}},
            {"type":"rpm","name":"a","version":"4","metadata":{"architecture":"noarch"}},
            {"type":"deb","name":"ignored"}, {"type":"rpm","name":""}
        ]});
        assert_eq!(rpm_manifest(&sbom), vec!["a-4.noarch", "z-2-3.x86_64"]);
        assert!(render_rpm_manifest(&sbom).ends_with('\n'));
    }

    #[test]
    fn validates_release_tags_and_parses_copr_nvr() {
        assert!(safe_release_tag("v9.1+build-2"));
        assert!(!safe_release_tag("../unsafe"));
        assert!(valid_sha256_digest(&format!("sha256:{}", "a".repeat(64))));
        assert!(!valid_sha256_digest("sha256:ABC"));
        assert!(!valid_sha256_digest("sha256:abc"));
        let payload = serde_json::json!({"builds":{"latest_succeeded":{"source_package":{"version":"6.12", "release":"4.fc42"}}}});
        assert_eq!(copr_latest_nvr(&payload).as_deref(), Some("6.12-4.fc42"));
    }

    #[test]
    fn measures_image_and_unique_rpm_artifact_sizes() {
        let oci = serde_json::json!({"layers":[{"size":10},{"size":20}]});
        assert_eq!(
            artifact_metrics(Some(&oci), Some("a\na\n# comment\nb\n"))["image_compressed_bytes"],
            30
        );
        assert_eq!(
            artifact_metrics(Some(&oci), Some("a\na\n# comment\nb\n"))["rpm_package_count"],
            2
        );
        let index = serde_json::json!({"manifests":[{"size":4},{"size":6}],"layers":[{"size":99}]});
        assert_eq!(
            artifact_metrics(Some(&index), None)["image_index_bytes"],
            10
        );
        assert!(!artifact_metrics(Some(&index), None).contains_key("image_layer_count"));
    }

    #[test]
    fn raises_only_measured_coverage_floors() {
        let floors = BTreeMap::from([("a.py".into(), 80.0), ("missing.py".into(), 90.0)]);
        let report = serde_json::json!({"files":{"a.py":{"summary":{"percent_covered":91.8}}}});
        let updated = raised_coverage_floors(&floors, &report);
        assert_eq!(updated["a.py"], 91.0);
        assert_eq!(updated["missing.py"], 90.0);
    }

    #[test]
    fn coverage_floor_lookup_accepts_packaged_source_aliases() {
        let floors =
            BTreeMap::from([("build_files/kyth_shared/kyth_shared/health.py".into(), 80.0)]);
        let report = serde_json::json!({"files":{"src/kyth_shared/kyth_shared/health.py":{"summary":{"percent_covered":"91.4"}}}});
        assert_eq!(
            raised_coverage_floors(&floors, &report)
                ["build_files/kyth_shared/kyth_shared/health.py"],
            91.0
        );
    }

    #[test]
    fn reports_missing_and_below_floor_entries_and_honors_changed_only() {
        let floors = BTreeMap::from([
            ("a.py".into(), 80.0),
            ("missing.py".into(), 90.0),
            ("untouched.py".into(), 50.0),
        ]);
        let report = serde_json::json!({"files":{"a.py":{"summary":{"percent_covered":79.94}}}});
        let changed = BTreeSet::from(["a.py".to_string(), "missing.py".to_string()]);
        assert_eq!(
            coverage_failures(&floors, &report, Some(&changed)),
            vec![
                "a.py: 79.9% is below 80.0%",
                "missing.py: absent from coverage report",
            ]
        );
        assert!(coverage_failures(&floors, &report, None)
            .iter()
            .any(|failure| failure.starts_with("untouched.py:")));
    }

    #[test]
    fn extracts_labels_and_requires_all_inputs_for_reuse() {
        let payload = serde_json::json!({"Image":{"Config":{"Labels":{
            "org.kyth.build.upstream-base":"fedora", "org.kyth.build.kernel-flavor":"fedora",
            "org.kyth.build.cachyos-kernel-version":"none", "org.kyth.build.base-src-hash":"abc"
        }}} , "manifest":{"digest":"sha256:123"}});
        let labels = extract_labels(&payload);
        let decision = decide_base_reuse(
            &labels,
            extract_digest(&payload).as_deref(),
            "fedora",
            "fedora",
            "none",
            "abc",
        );
        assert!(decision.reuse);
        assert_eq!(decision.reason, "match");
        assert_eq!(extract_digest(&payload).as_deref(), Some("sha256:123"));
    }

    #[test]
    fn source_hash_changes_with_path_or_content() {
        let directory = tempdir().unwrap();
        std::fs::write(directory.path().join("a"), "one").unwrap();
        let first = source_hash(directory.path()).unwrap();
        std::fs::write(directory.path().join("a"), "two").unwrap();
        assert_ne!(first, source_hash(directory.path()).unwrap());
    }
}
