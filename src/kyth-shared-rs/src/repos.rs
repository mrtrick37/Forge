//! Offline third-party repository specifications.
//!
//! Rendering repository text is safe and deterministic; enabling repositories
//! or importing signing keys remains an explicit package-management action.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSpec {
    pub name: String,
    pub description: String,
    pub baseurl: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(rename = "type", default = "default_repo_type")]
    pub repo_type: String,
    #[serde(default = "default_true")]
    pub repo_gpgcheck: bool,
    #[serde(default)]
    pub gpgcheck: bool,
    #[serde(default)]
    pub gpgkey: String,
}

fn default_true() -> bool {
    true
}
fn default_repo_type() -> String {
    "rpm".into()
}

impl RepoSpec {
    pub fn render_yum_repo(&self) -> String {
        let mut lines = vec![
            format!("[{}]", self.name),
            format!("name={}", self.description),
            format!("baseurl={}", self.baseurl),
            format!("enabled={}", i32::from(self.enabled)),
            format!("type={}", self.repo_type),
            format!("repo_gpgcheck={}", i32::from(self.repo_gpgcheck)),
            format!("gpgcheck={}", i32::from(self.gpgcheck)),
        ];
        if !self.gpgkey.is_empty() {
            lines.push(format!("gpgkey={}", self.gpgkey));
        }
        format!("{}\n", lines.join("\n"))
    }
}

pub const GAMING_COPRS: [&str; 7] = [
    "ublue-os/bazzite",
    "ublue-os/bazzite-multilib",
    "ublue-os/staging",
    "ublue-os/packages",
    "ublue-os/obs-vkcapture",
    "lukenukem/asus-linux",
    "ycollet/audinux",
];

pub fn load_repo_specs(path: impl AsRef<Path>) -> Result<Vec<RepoSpec>, String> {
    let raw = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn renders_defaults_and_optional_gpg_key() {
        let spec: RepoSpec = serde_json::from_value(serde_json::json!({
            "name": "demo", "description": "Demo repo", "baseurl": "https://example.test/rpm", "gpgkey": "https://example.test/key"
        })).unwrap();
        assert!(spec.enabled);
        assert_eq!(spec.repo_type, "rpm");
        assert!(spec.render_yum_repo().contains("repo_gpgcheck=1"));
        assert!(spec
            .render_yum_repo()
            .ends_with("gpgkey=https://example.test/key\n"));
    }

    #[test]
    fn loads_json_specs() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("repos.json");
        std::fs::write(
            &path,
            r#"[{"name":"demo","description":"Demo","baseurl":"https://example.test"}]"#,
        )
        .unwrap();
        assert_eq!(load_repo_specs(&path).unwrap()[0].name, "demo");
    }
}
