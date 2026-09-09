//! Declarative, offline role-preset model.
//!
//! This ports the durable preset TOML contract. Installing Flatpaks,
//! creating Distroboxes, and installing editor extensions remain explicit
//! service actions and are intentionally not hidden behind this model.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Everyday,
    Gaming,
    Dev,
    Creator,
}

impl Role {
    pub fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("everyday") {
            "gaming" => Self::Gaming,
            "dev" => Self::Dev,
            "creator" => Self::Creator,
            _ => Self::Everyday,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Everyday => "everyday",
            Self::Gaming => "gaming",
            Self::Dev => "dev",
            Self::Creator => "creator",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolePreset {
    pub profile: Role,
    pub flatpaks: Vec<String>,
    pub distroboxes: Vec<String>,
    pub vscode_extensions: Vec<String>,
}

fn values(profile: Role) -> RolePreset {
    let (flatpaks, distroboxes, vscode_extensions) = match profile {
        Role::Everyday => (
            vec!["com.brave.Browser", "com.valvesoftware.Steam"],
            vec![],
            vec![],
        ),
        Role::Gaming => (
            vec![
                "com.valvesoftware.Steam",
                "net.lutris.Lutris",
                "com.heroicgameslauncher.hgl",
                "com.github.Matoking.protontricks",
            ],
            vec![],
            vec![],
        ),
        Role::Dev => (
            vec![
                "com.visualstudio.code",
                "com.github.flathub.flatpak-external-data-checker",
            ],
            vec!["kyth-ai-dev"],
            vec!["ms-python.python", "rust-lang.rust-analyzer"],
        ),
        Role::Creator => (
            vec!["com.obsproject.Studio", "org.kde.kdenlive"],
            vec![],
            vec![],
        ),
    };
    RolePreset {
        profile,
        flatpaks: flatpaks.into_iter().map(String::from).collect(),
        distroboxes: distroboxes.into_iter().map(String::from).collect(),
        vscode_extensions: vscode_extensions.into_iter().map(String::from).collect(),
    }
}

impl Default for RolePreset {
    fn default() -> Self {
        values(Role::Everyday)
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join("kyth/preset.toml");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
        .join(".config/kyth/preset.toml")
}

fn strings(value: Option<&toml::Value>, fallback: &[String]) -> Vec<String> {
    value
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_else(|| fallback.to_vec())
}

pub fn load(path: impl AsRef<Path>) -> RolePreset {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return RolePreset::default();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return RolePreset::default();
    };
    let profile = Role::parse(value.get("profile").and_then(toml::Value::as_str));
    let defaults = values(profile);
    RolePreset {
        profile,
        flatpaks: strings(value.get("flatpaks"), &defaults.flatpaks),
        distroboxes: strings(value.get("distroboxes"), &defaults.distroboxes),
        vscode_extensions: strings(value.get("vscode_extensions"), &defaults.vscode_extensions),
    }
}

fn array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| format!("{value:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Defaults for a validated profile, mirroring the launcher's overwrite of
/// the TOML preset before applying (no TOML merge when a profile is given).
pub fn defaults_for(profile: Role) -> RolePreset {
    values(profile)
}

/// Parses `flatpak list --app --columns=application` output.
pub fn parse_flatpak_list(text: &str) -> std::collections::HashSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

/// Parses `distrobox list --no-color` output (third whitespace column).
pub fn parse_distrobox_list(text: &str) -> std::collections::HashSet<String> {
    text.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            parts.get(2).map(|name| name.to_string())
        })
        .collect()
}

/// Parses `code --list-extensions` output (compared lowercased upstream).
pub fn parse_extension_list(text: &str) -> std::collections::HashSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.to_lowercase())
        .collect()
}

pub fn flatpak_install_argv(app: &str) -> Vec<String> {
    vec![
        "flatpak".to_string(),
        "install".to_string(),
        "-y".to_string(),
        "flathub".to_string(),
        app.to_string(),
    ]
}

pub fn distrobox_create_argv(name: &str) -> Vec<String> {
    vec![
        "distrobox".to_string(),
        "create".to_string(),
        "--yes".to_string(),
        "--name".to_string(),
        name.to_string(),
        "--image".to_string(),
        "registry.fedoraproject.org/fedora-toolbox:44".to_string(),
    ]
}

pub fn extension_install_argv(binary: &str, extension: &str) -> Vec<String> {
    vec![
        binary.to_string(),
        "--install-extension".to_string(),
        extension.to_string(),
    ]
}

pub const VSCODE_BINARIES: [&str; 3] = ["code", "codium", "code-insiders"];
pub const VSCODE_INSTALL_BINARIES: [&str; 2] = ["code", "codium"];

/// Splits a preset into already-present (`skipped`) and to-install
/// (`installed`) items, mirroring `apply_preset`'s loops exactly (extension
/// comparison is lowercase; install order is flatpaks, boxes, extensions).
pub fn plan_installs(
    preset: &RolePreset,
    have_flatpaks: &std::collections::HashSet<String>,
    have_boxes: &std::collections::HashSet<String>,
    have_extensions: &std::collections::HashSet<String>,
) -> (Vec<String>, Vec<String>) {
    let mut installed = Vec::new();
    let mut skipped = Vec::new();
    for app in &preset.flatpaks {
        if have_flatpaks.contains(app) {
            skipped.push(app.clone());
        } else {
            installed.push(app.clone());
        }
    }
    for name in &preset.distroboxes {
        if have_boxes.contains(name) {
            skipped.push(name.clone());
        } else {
            installed.push(name.clone());
        }
    }
    for extension in &preset.vscode_extensions {
        if have_extensions.contains(&extension.to_lowercase()) {
            skipped.push(extension.clone());
        } else {
            installed.push(extension.clone());
        }
    }
    (installed, skipped)
}

pub fn save(path: impl AsRef<Path>, preset: &RolePreset) -> std::io::Result<()> {
    let text = format!(
        "profile = {:?}\nflatpaks = {}\ndistroboxes = {}\nvscode_extensions = {}\n",
        preset.profile.as_str(),
        array(&preset.flatpaks),
        array(&preset.distroboxes),
        array(&preset.vscode_extensions)
    );
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn supplies_role_defaults_and_unknown_profile_falls_back() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("preset.toml");
        std::fs::write(&path, "profile = \"dev\"\n").unwrap();
        let preset = load(&path);
        assert_eq!(preset.profile, Role::Dev);
        assert_eq!(preset.distroboxes, vec!["kyth-ai-dev"]);
        assert_eq!(Role::parse(Some("unknown")), Role::Everyday);
    }

    #[test]
    fn plans_installs_against_have_sets() {
        let preset = defaults_for(Role::Dev);
        let have_flatpaks = ["com.visualstudio.code".to_string()].into_iter().collect();
        let have_boxes = std::collections::HashSet::new();
        let have_extensions = ["ms-python.python".to_string()].into_iter().collect();
        let (installed, skipped) =
            plan_installs(&preset, &have_flatpaks, &have_boxes, &have_extensions);
        assert!(skipped.contains(&"com.visualstudio.code".to_string()));
        assert!(skipped.contains(&"ms-python.python".to_string()));
        assert!(installed.contains(&"kyth-ai-dev".to_string()));
        // Third whitespace column, exactly like Python (including a header
        // row's third column, and skipping short lines).
        assert_eq!(
            parse_distrobox_list("id image name\n1 img kyth-ai-dev extra\nshort\n"),
            ["name".to_string(), "kyth-ai-dev".to_string()]
                .into_iter()
                .collect()
        );
        assert!(parse_extension_list("MS-Python.Python\n").contains("ms-python.python"));
    }

    #[test]
    fn preserves_explicit_lists_and_round_trips() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("preset.toml");
        let preset = RolePreset {
            profile: Role::Creator,
            flatpaks: vec!["org.example.App".into()],
            distroboxes: vec![],
            vscode_extensions: vec!["rust-lang.rust-analyzer".into()],
        };
        save(&path, &preset).unwrap();
        assert_eq!(load(&path), preset);
    }
}
