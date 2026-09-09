//! Pinned gaming-tool version metadata.
//!
//! This ports the offline data model and OCI-label projection from
//! gaming_resolve.py. Resolving versions remotely and writing the runtime
//! cache remain build/runtime-owned operations.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const DEFAULT_REPO: &str = "CachyOS/proton-cachyos";
pub const CACHE_PATH: &str = "/var/lib/kyth/gaming-versions.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GamingVersions {
    #[serde(default)]
    pub umu_version: String,
    #[serde(default)]
    pub proton_cachyos_version: String,
    #[serde(default = "default_repo")]
    pub proton_cachyos_repo: String,
    #[serde(default)]
    pub mesa_git_copr: String,
}

fn default_repo() -> String {
    DEFAULT_REPO.into()
}

impl Default for GamingVersions {
    fn default() -> Self {
        Self {
            umu_version: String::new(),
            proton_cachyos_version: String::new(),
            proton_cachyos_repo: default_repo(),
            mesa_git_copr: String::new(),
        }
    }
}

impl GamingVersions {
    pub fn is_pinned(&self) -> bool {
        !self.umu_version.is_empty() && !self.proton_cachyos_version.is_empty()
    }

    pub fn from_value(value: &Value) -> Self {
        let Some(object) = value.as_object() else {
            return Self::default();
        };
        Self {
            umu_version: value_string(object.get("umu_version")),
            proton_cachyos_version: value_string(object.get("proton_cachyos_version")),
            proton_cachyos_repo: value_string(object.get("proton_cachyos_repo"))
                .if_empty_then(default_repo),
            mesa_git_copr: value_string(object.get("mesa_git_copr")),
        }
    }

    pub fn load(path: impl AsRef<Path>) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .map(|value| Self::from_value(&value))
            .unwrap_or_default()
    }

    /// Candidate paths used by the build image and installed runtime.
    ///
    /// The repository-relative path intentionally remains first so local
    /// build tooling behaves like the Python resolver, while the image paths
    /// keep the installed Hub usable without the source checkout.
    pub fn candidate_paths() -> [PathBuf; 4] {
        [
            PathBuf::from("config/gaming-versions.json"),
            PathBuf::from("/ctx/config/gaming-versions.json"),
            PathBuf::from("/usr/share/kyth/config/gaming-versions.json"),
            PathBuf::from("/etc/kyth/config/gaming-versions.json"),
        ]
    }

    /// Load the first valid object from the resolver's candidate paths.
    pub fn load_candidates(paths: impl IntoIterator<Item = impl AsRef<Path>>) -> Option<Self> {
        paths.into_iter().find_map(|path| {
            let value = std::fs::read_to_string(path)
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())?;
            value.as_object()?;
            Some(Self::from_value(&value))
        })
    }

    /// Resolve config, offline cache, and environment overrides in Python's
    /// precedence order without touching process-global environment state.
    pub fn resolve(
        file_values: Option<&Value>,
        cache_values: Option<&Value>,
        umu_env: Option<&str>,
        proton_env: Option<&str>,
    ) -> Self {
        let source = file_values
            .filter(|value| value.as_object().is_some())
            .or_else(|| cache_values.filter(|value| value.as_object().is_some()));
        let defaults = source.map(Self::from_value).unwrap_or_default();
        Self {
            umu_version: nonempty_or(umu_env, defaults.umu_version),
            proton_cachyos_version: nonempty_or(proton_env, defaults.proton_cachyos_version),
            ..defaults
        }
    }

    /// Resolve installed/runtime metadata using the same precedence as the
    /// Python entry point. Cache persistence remains caller-owned.
    pub fn load_runtime() -> Self {
        let file = Self::candidate_paths()
            .iter()
            .find_map(|path| read_object(path));
        let cache = read_object(Path::new(CACHE_PATH));
        let umu = std::env::var("UMU_VERSION").ok();
        let proton = std::env::var("PROTON_CACHYOS_VER").ok();
        Self::resolve(
            file.as_ref(),
            cache.as_ref(),
            umu.as_deref(),
            proton.as_deref(),
        )
    }

    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if !self.umu_version.is_empty() {
            parts.push(format!("umu@{}", self.umu_version));
        }
        if !self.proton_cachyos_version.is_empty() {
            parts.push(format!("proton-cachyos@{}", self.proton_cachyos_version));
        }
        if !self.mesa_git_copr.is_empty() {
            parts.push(format!("mesa-git:{}", self.mesa_git_copr));
        }
        if parts.is_empty() {
            "unpinned".into()
        } else {
            parts.join(", ")
        }
    }
}

fn read_object(path: &Path) -> Option<Value> {
    let value = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())?;
    value.as_object()?;
    Some(value)
}

fn nonempty_or(value: Option<&str>, fallback: String) -> String {
    value
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or(fallback)
}

trait EmptyFallback {
    fn if_empty_then(self, fallback: impl FnOnce() -> String) -> String;
}

impl EmptyFallback for String {
    fn if_empty_then(self, fallback: impl FnOnce() -> String) -> String {
        if self.is_empty() {
            fallback()
        } else {
            self
        }
    }
}

fn value_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_pinned_versions_and_formats_label() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("gaming-versions.json");
        std::fs::write(
            &path,
            r#"{"umu_version":"0.10","proton_cachyos_version":"9.0","mesa_git_copr":"user/mesa"}"#,
        )
        .unwrap();
        let versions = GamingVersions::load(&path);
        assert!(versions.is_pinned());
        assert_eq!(versions.proton_cachyos_repo, "CachyOS/proton-cachyos");
        assert_eq!(
            versions.label(),
            "umu@0.10, proton-cachyos@9.0, mesa-git:user/mesa"
        );
    }

    #[test]
    fn defaults_to_unpinned_and_ignores_invalid_json() {
        assert_eq!(
            GamingVersions::from_value(&serde_json::json!({})),
            GamingVersions::default()
        );
        let versions = GamingVersions::from_value(
            &serde_json::json!({"umu_version": 12, "proton_cachyos_version": true}),
        );
        assert!(versions.is_pinned());
        assert_eq!(versions.label(), "umu@12, proton-cachyos@true");
    }

    #[test]
    fn resolves_file_cache_and_environment_precedence() {
        let file = serde_json::json!({
            "umu_version": "file-umu",
            "proton_cachyos_version": "file-proton",
            "mesa_git_copr": "file/mesa"
        });
        let cache = serde_json::json!({
            "umu_version": "cache-umu",
            "proton_cachyos_version": "cache-proton"
        });
        let from_file = GamingVersions::resolve(Some(&file), Some(&cache), None, None);
        assert_eq!(from_file.umu_version, "file-umu");
        assert_eq!(from_file.mesa_git_copr, "file/mesa");

        let from_cache = GamingVersions::resolve(None, Some(&cache), Some("env-umu"), Some(""));
        assert_eq!(from_cache.umu_version, "env-umu");
        assert_eq!(from_cache.proton_cachyos_version, "cache-proton");
    }

    #[test]
    fn candidates_skip_invalid_json_and_non_objects() {
        let directory = tempdir().unwrap();
        let invalid = directory.path().join("invalid.json");
        let valid = directory.path().join("valid.json");
        std::fs::write(&invalid, "[]").unwrap();
        std::fs::write(&valid, r#"{"umu_version":"0.11"}"#).unwrap();
        let versions = GamingVersions::load_candidates([invalid, valid]).unwrap();
        assert_eq!(versions.umu_version, "0.11");
    }
}
