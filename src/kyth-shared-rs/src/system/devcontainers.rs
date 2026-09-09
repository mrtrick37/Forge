//! Native replacement for the declarative half of the Python
//! `kyth-setup-devcontainer` launcher.
//!
//! Parses `devcontainers.toml` (`[containers."name"]` tables with `image`
//! and `init` keys) and renders the `distrobox create` argv each box needs.
//! A missing file, an undecodable document, and non-table entries all yield
//! no boxes, matching `load_devcontainers`. `devcontainers.py` stays as the
//! Phase 3 fixture.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const DEFAULT_IMAGE: &str = "quay.io/toolbx/ubuntu-toolbox:24.04";
pub const NO_CONTAINERS_MESSAGE: &str =
    "kyth-setup-devcontainer: no containers in devcontainers.toml";
pub const CREATE_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevContainer {
    pub image: String,
    pub init: bool,
}

impl DevContainer {
    fn from_table(table: &toml::map::Map<String, toml::Value>) -> Self {
        let image = table
            .get("image")
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| value.to_string())
            })
            .unwrap_or_else(|| DEFAULT_IMAGE.to_string());
        Self {
            image,
            init: table
                .get("init")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
        }
    }
}

/// Resolve the declarative config path: `$XDG_CONFIG_HOME/kyth/` wins when
/// set and non-empty, otherwise `$HOME/.config/kyth/`.
pub fn devcontainers_path(home: &Path, xdg_config_home: Option<&str>) -> PathBuf {
    if let Some(xdg) = xdg_config_home.filter(|value| !value.is_empty()) {
        Path::new(xdg).join("kyth/devcontainers.toml")
    } else {
        home.join(".config/kyth/devcontainers.toml")
    }
}

/// Parse declarative TOML text; anything undecodable or misshapen yields no
/// boxes, exactly like the Python loader.
pub fn parse_devcontainers(text: &str) -> BTreeMap<String, DevContainer> {
    let Ok(value) = text.parse::<toml::Value>() else {
        return BTreeMap::new();
    };
    let Some(containers) = value.get("containers").and_then(toml::Value::as_table) else {
        return BTreeMap::new();
    };
    containers
        .iter()
        .filter_map(|(name, entry)| {
            entry
                .as_table()
                .map(|table| (name.clone(), DevContainer::from_table(table)))
        })
        .collect()
}

/// Load the declarative config; a missing or undecodable file yields no
/// boxes so the caller prints the no-containers line and exits zero.
pub fn load_devcontainers(path: &Path) -> BTreeMap<String, DevContainer> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_devcontainers(&text),
        Err(_) => BTreeMap::new(),
    }
}

/// Best-effort creation argv: `distrobox create --name <name> --image <img> --yes`.
pub fn create_argv(name: &str, image: &str) -> Vec<String> {
    [
        "distrobox",
        "create",
        "--name",
        name,
        "--image",
        image,
        "--yes",
    ]
    .iter()
    .map(|part| (*part).to_string())
    .collect()
}

/// Status line printed before each best-effort creation.
pub fn describe(name: &str, image: &str) -> String {
    format!("kyth-setup-devcontainer: {name} \u{2192} {image}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[containers."web"]
image = "quay.io/toolbx/ubuntu-toolbox:24.04"
init = true

[containers."bare"]
"#;

    #[test]
    fn parses_declared_boxes_with_python_defaults() {
        let boxes = parse_devcontainers(SAMPLE);
        assert_eq!(boxes.len(), 2);
        assert_eq!(
            boxes["web"],
            DevContainer {
                image: DEFAULT_IMAGE.to_string(),
                init: true
            }
        );
        assert_eq!(
            boxes["bare"],
            DevContainer {
                image: DEFAULT_IMAGE.to_string(),
                init: false
            }
        );
    }

    #[test]
    fn missing_undecodable_and_misshapen_inputs_yield_no_boxes() {
        assert!(parse_devcontainers("not = [valid").is_empty());
        assert!(parse_devcontainers("[other]\nkey = 1\n").is_empty());
        assert!(parse_devcontainers("containers = [1, 2]\n").is_empty());
        assert!(parse_devcontainers("[containers]\nplain = 1\n").is_empty());
        assert!(load_devcontainers(Path::new("/nonexistent/kyth-devcontainers.toml")).is_empty());
    }

    #[test]
    fn resolves_config_path_from_xdg_or_home() {
        let home = Path::new("/home/demo");
        assert_eq!(
            devcontainers_path(home, Some("/etc/xdg")),
            PathBuf::from("/etc/xdg/kyth/devcontainers.toml")
        );
        assert_eq!(
            devcontainers_path(home, None),
            PathBuf::from("/home/demo/.config/kyth/devcontainers.toml")
        );
        assert_eq!(
            devcontainers_path(home, Some("")),
            PathBuf::from("/home/demo/.config/kyth/devcontainers.toml")
        );
    }

    #[test]
    fn renders_best_effort_argv_and_status_line() {
        assert_eq!(
            create_argv("web", "img:latest"),
            vec![
                "distrobox",
                "create",
                "--name",
                "web",
                "--image",
                "img:latest",
                "--yes"
            ]
        );
        assert_eq!(
            describe("web", "img:latest"),
            "kyth-setup-devcontainer: web \u{2192} img:latest"
        );
    }
}
