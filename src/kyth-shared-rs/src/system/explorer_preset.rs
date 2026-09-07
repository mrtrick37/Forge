//! Explorer parity — Dolphin double-click, preview, and drives-on-desktop
//! preference. Ports `kyth_shared.explorer_preset` in full, including
//! `apply_explorer`'s `kwriteconfig5` application step (see that function's
//! doc comment for the fixed argv and the quirks it deliberately preserves).

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::system::process::run_bounded;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerConfig {
    pub click: String,
    pub preview: bool,
    pub preview_pane: bool,
    pub drives_on_desktop: bool,
}

impl Default for ExplorerConfig {
    fn default() -> Self {
        Self { click: "double".into(), preview: true, preview_pane: true, drives_on_desktop: true }
    }
}

pub fn explorer_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join("kyth/explorer.toml");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into())).join(".config/kyth/explorer.toml")
}

pub fn load_explorer(path: impl AsRef<Path>) -> ExplorerConfig {
    let Ok(raw) = std::fs::read_to_string(path) else { return ExplorerConfig::default(); };
    let Ok(value) = raw.parse::<toml::Value>() else { return ExplorerConfig::default(); };
    let click = match value.get("click").and_then(toml::Value::as_str) {
        Some("single") => "single",
        _ => "double",
    };
    let flag = |key: &str| value.get(key).and_then(toml::Value::as_bool).unwrap_or(true);
    ExplorerConfig {
        click: click.into(),
        preview: flag("preview"),
        preview_pane: flag("preview_pane"),
        drives_on_desktop: flag("drives_on_desktop"),
    }
}

pub fn save_explorer(path: impl AsRef<Path>, config: &ExplorerConfig) -> std::io::Result<()> {
    let click = if config.click == "single" { "single" } else { "double" };
    let content = format!(
        "# Kyth Explorer parity — Windows double-click + preview + drives\nclick = \"{click}\"\npreview = {}\npreview_pane = {}\ndrives_on_desktop = {}\n",
        config.preview, config.preview_pane, config.drives_on_desktop,
    );
    crate::atomic_io::atomic_write_text(path, &content, None)
}

/// Apply `config` via `kwriteconfig5`, reproducing
/// `kyth_shared.explorer_preset.apply_explorer` byte for byte, including two
/// quirks that look incidental but are pinned Python behavior, not bugs to
/// fix here:
/// - The binary is hardcoded to `kwriteconfig5`, not the `kwriteconfig6`
///   fallback chain used elsewhere in this crate (`plasma_hdr`, `vrr`,
///   `window_snap`). On the shipped Kinoite 44 (Plasma 6) image
///   `kwriteconfig5` is not installed, so both writes below fail to spawn
///   and are silently swallowed — this apply step is a no-op on every
///   currently shipped image. Fixing that is a separate, deliberate change,
///   not part of this port.
/// - Only the `SingleClick` write is recorded into the returned list, even
///   when both writes spawn successfully; the `ShowPreview` write's result
///   is discarded the same way Python's second `try` block never appends to
///   `applied`.
///
/// The TTL marker write is unconditional, independent of whether either
/// `kwriteconfig5` call above it succeeded, matching Python's separate
/// `try` block for `/run/kyth-explorer-ttl`.
/// Project `config`'s `SingleClick` write to the fixed `kwriteconfig5` argv.
/// Kept separate from `apply_explorer` so the projection is testable without
/// depending on whether `kwriteconfig5` is actually installed.
pub fn single_click_argv(config: &ExplorerConfig) -> Vec<String> {
    let single = if config.click == "single" { "true" } else { "false" };
    ["kwriteconfig5", "--file", "kdeglobals", "--group", "KDE", "--key", "SingleClick", single].map(String::from).to_vec()
}

/// Project `config`'s `ShowPreview` write to the fixed `kwriteconfig5` argv.
pub fn show_preview_argv(config: &ExplorerConfig) -> Vec<String> {
    let preview = if config.preview { "true" } else { "false" };
    ["kwriteconfig5", "--file", "dolphinrc", "--group", "General", "--key", "ShowPreview", preview].map(String::from).to_vec()
}

pub fn apply_explorer(config: &ExplorerConfig) -> Vec<String> {
    let mut applied = Vec::new();
    if run_bounded(&single_click_argv(config), Duration::from_secs(5)).is_ok() {
        let single = if config.click == "single" { "true" } else { "false" };
        applied.push(format!("SingleClick={single}"));
    }
    let _ = run_bounded(&show_preview_argv(config), Duration::from_secs(5));

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = crate::atomic_io::atomic_write_text("/run/kyth-explorer-ttl", &(now + 30).to_string(), None);

    applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn defaults_when_missing_or_malformed() {
        let directory = tempdir().unwrap();
        assert_eq!(load_explorer(directory.path().join("missing.toml")), ExplorerConfig::default());
        let malformed = directory.path().join("bad.toml");
        std::fs::write(&malformed, "not valid toml {{{").unwrap();
        assert_eq!(load_explorer(&malformed), ExplorerConfig::default());
    }

    #[test]
    fn round_trips_and_rejects_invalid_click() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("explorer.toml");
        let config = ExplorerConfig { click: "single".into(), preview: false, preview_pane: false, drives_on_desktop: false };
        save_explorer(&path, &config).unwrap();
        assert_eq!(load_explorer(&path), config);

        std::fs::write(&path, "click = \"sideways\"\n").unwrap();
        assert_eq!(load_explorer(&path).click, "double");
    }

    #[test]
    fn explorer_path_honors_an_explicit_override() {
        // Env-var fallback branches are exercised by inspection against the
        // Python original rather than by mutating process-global XDG_*
        // state here — see MIGRATION.md on keeping tests parallel-safe.
        assert_eq!(explorer_path(Some("/tmp/x.toml")), PathBuf::from("/tmp/x.toml"));
    }

    #[test]
    fn single_click_argv_reflects_click_mode() {
        let single = ExplorerConfig { click: "single".into(), ..ExplorerConfig::default() };
        assert_eq!(
            single_click_argv(&single),
            vec!["kwriteconfig5", "--file", "kdeglobals", "--group", "KDE", "--key", "SingleClick", "true"],
        );
        let double = ExplorerConfig { click: "double".into(), ..ExplorerConfig::default() };
        assert_eq!(single_click_argv(&double).last().unwrap(), "false");
    }

    #[test]
    fn show_preview_argv_reflects_preview_flag() {
        let off = ExplorerConfig { preview: false, ..ExplorerConfig::default() };
        assert_eq!(
            show_preview_argv(&off),
            vec!["kwriteconfig5", "--file", "dolphinrc", "--group", "General", "--key", "ShowPreview", "false"],
        );
    }

    #[test]
    fn apply_explorer_swallows_a_missing_binary_without_panicking() {
        // kwriteconfig5 is absent from this sandbox (and from the shipped
        // Kinoite 44 image — see apply_explorer's doc comment), so this
        // exercises the same dead-path Python's try/except swallows: no
        // panic, and SingleClick is only recorded when the spawn succeeds.
        let config = ExplorerConfig { click: "single".into(), ..ExplorerConfig::default() };
        let applied = apply_explorer(&config);
        assert!(applied.is_empty() || applied == vec!["SingleClick=true"]);
    }
}
