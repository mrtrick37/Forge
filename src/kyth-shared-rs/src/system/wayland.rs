//! Pure Wayland/software-compositor policy helpers.
//!
//! The privileged/session setup writers remain outside this crate. These
//! functions let native callers make the same deterministic rescue decision
//! and generate the same compositor arguments without spawning a shell.

use std::collections::BTreeMap;
use std::path::Path;

pub const PLASMA_WAYLAND_SESSION: &str = "plasma.desktop";
pub const PLM_SESSION_CONF: &str =
    "[General]\nDefaultSession=plasma.desktop\n\n[Autologin]\nSession=plasma.desktop\n";

pub fn software_compose_env() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("LIBGL_ALWAYS_SOFTWARE", "1"),
        ("GALLIUM_DRIVER", "llvmpipe"),
        ("MESA_LOADER_DRIVER_OVERRIDE", "llvmpipe"),
        ("QT_QUICK_BACKEND", "software"),
        ("KWIN_COMPOSE", "Q"),
    ])
}

fn has_token(cmdline: Option<&str>, token: &str) -> bool {
    cmdline
        .unwrap_or_default()
        .split_whitespace()
        .any(|value| value == token)
}

pub fn hwgl_forced(cmdline: Option<&str>) -> bool {
    has_token(cmdline, "kyth.hwgl=1")
}
pub fn is_live_image(cmdline: Option<&str>) -> bool {
    cmdline
        .unwrap_or_default()
        .split_whitespace()
        .any(|value| value == "kyth.live" || value.starts_with("kyth.live="))
}
pub fn nomodeset_requested(cmdline: Option<&str>) -> bool {
    has_token(cmdline, "nomodeset")
}

pub fn has_drm_render_node(dri: impl AsRef<Path>) -> bool {
    dri.as_ref()
        .read_dir()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().starts_with("renderD"))
}

pub fn has_drm_card(dri: impl AsRef<Path>) -> bool {
    dri.as_ref()
        .read_dir()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().starts_with("card"))
}

pub fn greeter_session_conf() -> &'static str {
    PLM_SESSION_CONF
}

pub fn needs_software_compose(dri: impl AsRef<Path>, cmdline: Option<&str>) -> bool {
    if hwgl_forced(cmdline) {
        return false;
    }
    nomodeset_requested(cmdline) || is_live_image(cmdline) || !has_drm_render_node(dri)
}

pub fn software_compose_rescue_justified(cmdline: Option<&str>) -> bool {
    nomodeset_requested(cmdline) || is_live_image(cmdline)
}

pub fn compositor_argv(extra: &[String]) -> Vec<String> {
    if !extra.is_empty() {
        return std::iter::once("kwin_wayland".into())
            .chain(extra.iter().cloned())
            .collect();
    }
    vec![
        "kwin_wayland",
        "--no-lockscreen",
        "--no-global-shortcuts",
        "--no-kactivities",
        "--inputmethod",
        "plasma-keyboard",
        "--locale1",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

pub fn session_is_plasma_x11(value: &str) -> bool {
    let token = value
        .trim()
        .trim_matches(['"', '\''])
        .to_ascii_lowercase()
        .replace('\\', "/");
    if token.is_empty() {
        return false;
    }
    let name = token.rsplit('/').next().unwrap_or(&token);
    name.contains("plasmax11") || token.contains("/xsessions/")
}

/// Rewrite only an active Session key when it still points to Plasma X11.
pub fn rewrite_session_key(text: &str, key: &str) -> (String, bool) {
    let prefix = format!("{key}=").to_ascii_lowercase();
    let mut changed = false;
    let rewritten = text
        .split_inclusive('\n')
        .map(|line_with_ending| {
            let (line, ending) = if let Some(line) = line_with_ending.strip_suffix('\n') {
                if let Some(line) = line.strip_suffix('\r') {
                    (line, "\r\n")
                } else {
                    (line, "\n")
                }
            } else {
                (line_with_ending, "")
            };
            let stripped = line.trim_start();
            if stripped.to_ascii_lowercase().starts_with(&prefix) && !stripped.starts_with('#') {
                let value = stripped
                    .split_once('=')
                    .map(|(_, value)| value.trim())
                    .unwrap_or_default();
                if session_is_plasma_x11(value) {
                    changed = true;
                    let indent = &line[..line.len() - line.trim_start().len()];
                    return format!("{indent}{key}={PLASMA_WAYLAND_SESSION}{ending}");
                }
            }
            line_with_ending.to_string()
        })
        .collect::<String>();
    (rewritten, changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn selects_software_rescue_only_for_explicit_modes() {
        let directory = tempdir().unwrap();
        std::fs::write(directory.path().join("renderD128"), "").unwrap();
        std::fs::write(directory.path().join("card0"), "").unwrap();
        assert!(!needs_software_compose(directory.path(), Some("quiet")));
        assert!(has_drm_card(directory.path()));
        assert!(needs_software_compose(directory.path(), Some("nomodeset")));
        assert!(!needs_software_compose(
            directory.path(),
            Some("nomodeset kyth.hwgl=1")
        ));
        assert!(software_compose_rescue_justified(Some("kyth.live")));
        assert!(!software_compose_rescue_justified(Some("quiet")));
    }

    #[test]
    fn recognizes_and_rewrites_plasma_x11_sessions() {
        assert!(session_is_plasma_x11("plasmaX11.desktop"));
        assert!(session_is_plasma_x11("/usr/share/xsessions/plasma.desktop"));
        assert!(!session_is_plasma_x11("plasma.desktop"));
        let (text, changed) = rewrite_session_key("[Last]\nSession=plasmaX11.desktop\n", "Session");
        assert!(changed);
        assert!(text.contains("Session=plasma.desktop"));
        let (crlf, changed) = rewrite_session_key("Session=plasmaX11.desktop\r\n", "Session");
        assert!(changed);
        assert!(crlf.ends_with("\r\n"));
        assert_eq!(greeter_session_conf(), PLM_SESSION_CONF);
    }
}
