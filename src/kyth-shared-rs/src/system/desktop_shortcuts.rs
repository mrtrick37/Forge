//! Pure desktop-file transformations used by shortcut/export helpers.
//!
//! Filesystem traversal, icon copying, cache refresh, and process launching
//! stay with the caller.  These functions only parse or transform text and
//! therefore can be shared by native Rust surfaces without importing Qt.

use regex::{Regex, RegexBuilder};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SteamDesktopRewrite {
    pub appid: String,
    pub name: String,
    pub icon: Option<String>,
    pub content: String,
}

/// Convert an application name into the filename-safe identifier used by
/// the Python shortcut exporter.
pub fn safe_id(name: &str) -> String {
    let mut result = String::new();
    let mut pending_separator = false;
    for character in name.to_lowercase().chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
            if pending_separator && !result.is_empty() { result.push('-'); }
            pending_separator = false;
            result.push(character);
        } else {
            pending_separator = true;
        }
    }
    result.trim_matches('-').to_string()
}

/// Rewrite one Steam `Exec=` line to the Flatpak Steam URI form.
pub fn rewrite_steam_exec(exec_line: &str) -> Option<String> {
    let target = exec_line.split_once('=')?.1.trim();
    let uri = Regex::new(r"steam://rungameid/([0-9]+)").ok()?;
    if let Some(capture) = uri.captures(target) {
        return Some(format!("Exec=flatpak run com.valvesoftware.Steam steam://rungameid/{}", &capture[1]));
    }
    let applaunch = Regex::new(r"-applaunch\s+([0-9]+)").ok()?;
    applaunch.captures(target).map(|capture| format!("Exec=flatpak run com.valvesoftware.Steam steam://rungameid/{}", &capture[1]))
}

/// Transform a Steam desktop file and return its stable app id and content.
/// Invalid or non-Steam entries return `None` and are left to the caller.
pub fn rewrite_steam_desktop(content: &str) -> Option<SteamDesktopRewrite> {
    let mut name = String::new();
    let mut icon = None;
    let mut rewritten_exec = None;
    let mut lines = Vec::new();
    let mut saw_categories = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(value) = line.strip_prefix("Name=") { name = value.trim().to_string(); }
        if let Some(value) = line.strip_prefix("Icon=") { icon = Some(value.trim().to_string()); }
        if trimmed.starts_with("Exec=") {
            if let Some(rewritten) = rewrite_steam_exec(trimmed) {
                let appid = Regex::new(r"rungameid/([0-9]+)").ok()?.captures(&rewritten)?[1].to_string();
                rewritten_exec = Some((appid, rewritten.clone()));
                lines.push(rewritten);
                continue;
            }
        }
        if trimmed.starts_with("Categories=") {
            saw_categories = true;
            lines.push("Categories=Game;".to_string());
        } else if trimmed.starts_with("NoDisplay=") || trimmed.starts_with("Hidden=") {
            continue;
        } else {
            lines.push(line.to_string());
        }
    }

    let (appid, exec) = rewritten_exec?;
    let exec_index = lines.iter().position(|line| line.trim().starts_with("Exec="));
    if let Some(index) = exec_index { lines[index] = exec; } else { return None; }
    if !saw_categories { lines.push("Categories=Game;".to_string()); }
    lines.push("X-KythExportedSteamGame=true".to_string());
    lines.push("X-Flatpak=com.valvesoftware.Steam".to_string());
    Some(SteamDesktopRewrite { appid, name, icon, content: format!("{}\n", lines.join("\n")) })
}

/// Insert the Kyth web-app category when a desktop file is an app launcher.
/// `None` means no change is needed.
pub fn categorize_web_app(content: &str) -> Option<String> {
    let app = RegexBuilder::new(r"--app(-id)?=").multi_line(true).build().ok()?;
    let categories = RegexBuilder::new(r"^Categories=").multi_line(true).build().ok()?;
    if !app.is_match(content) || categories.is_match(content) { return None; }
    let mut lines = Vec::new();
    let mut inserted = false;
    for line in content.lines() {
        lines.push(line.to_string());
        if !inserted && line.trim() == "[Desktop Entry]" {
            lines.push("Categories=X-KythWebApp;".to_string());
            inserted = true;
        }
    }
    Some(format!("{}\n", lines.join("\n")))
}

/// Zenmap-specific fixup for Kali exports: whole-line `Exec=`/`TryExec=`
/// replacement routing through the rootful Kali box. Like the Python loop,
/// the caller writes the result (and marks changed) whenever the Kali gate
/// passes, even when neither line matched.
pub fn rewrite_zenmap_desktop(content: &str) -> Option<String> {
    if !content.contains("--name kali") && !content.contains("-n kali") { return None; }
    let exec = Regex::new(r"(?m)^Exec=.*$").ok()?;
    let try_exec = Regex::new(r"(?m)^TryExec=.*$").ok()?;
    let content = exec.replace_all(content, "Exec=kyth-distrobox-root-launch --root kali /usr/bin/zenmap").into_owned();
    Some(try_exec.replace_all(&content, "TryExec=kyth-distrobox-root-launch").into_owned())
}

/// Apply the safe text-only Kali launcher fixups.  The caller decides which
/// files are in the Kali export directory and persists the returned text.
pub fn rewrite_kali_desktop(content: &str) -> Option<String> {
    if !content.contains("--name kali") && !content.contains("-n kali") { return None; }
    let privilege = Regex::new(r"\b(pkexec|kdesu|gksu|gksudo)\s+").ok()?;
    let hidden = RegexBuilder::new(r"^NoDisplay\s*=\s*true").case_insensitive(true).multi_line(false).build().ok()?;
    let mut lines = Vec::new();
    let mut has_categories = false;
    for line in content.lines() {
        if hidden.is_match(line) || line.starts_with("OnlyShowIn=") || line.starts_with("NotShowIn=") { continue; }
        if line.starts_with("Categories=") {
            lines.push("Categories=X-KythSecurity;".to_string());
            has_categories = true;
            continue;
        }
        lines.push(privilege.replace_all(line, "sudo -E ").into_owned());
    }
    if !has_categories { lines.push("Categories=X-KythSecurity;".to_string()); }
    Some(format!("{}\n", lines.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_ids_like_python_exporter() {
        assert_eq!(safe_id("  My Fancy App! 2.0 "), "my-fancy-app-2.0");
        assert_eq!(safe_id("A__B"), "a__b");
    }

    #[test]
    fn rewrites_uri_and_applaunch_forms() {
        assert_eq!(rewrite_steam_exec("Exec=steam -silent steam://rungameid/123"), Some("Exec=flatpak run com.valvesoftware.Steam steam://rungameid/123".into()));
        assert_eq!(rewrite_steam_exec("Exec=steam -applaunch 456"), Some("Exec=flatpak run com.valvesoftware.Steam steam://rungameid/456".into()));
        assert!(rewrite_steam_exec("Exec=steam --help").is_none());
    }

    #[test]
    fn rewrites_desktop_metadata_and_drops_hidden_flags() {
        let result = rewrite_steam_desktop("[Desktop Entry]\nName=Test Game\nIcon=123\nExec=steam -applaunch 123\nHidden=true\n").unwrap();
        assert_eq!(result.appid, "123");
        assert!(result.content.contains("Categories=Game;"));
        assert!(!result.content.contains("Hidden=true"));
        assert!(result.content.contains("X-KythExportedSteamGame=true"));
    }

    #[test]
    fn zenmap_exec_lines_route_through_kali_box() {
        let fixed = rewrite_zenmap_desktop("Exec=/usr/bin/zenmap %F\nTryExec=/usr/bin/zenmap\nName=x --name kali\n").unwrap();
        assert!(fixed.contains("Exec=kyth-distrobox-root-launch --root kali /usr/bin/zenmap"));
        assert!(fixed.contains("TryExec=kyth-distrobox-root-launch"));
        assert!(rewrite_zenmap_desktop("Exec=/usr/bin/other\n").is_none());
    }

    #[test]
    fn web_and_kali_rewrites_are_opt_in() {
        let web = categorize_web_app("[Desktop Entry]\nExec=brave --app=https://example.test\n").unwrap();
        assert!(web.contains("Categories=X-KythWebApp;"));
        assert!(categorize_web_app("[Desktop Entry]\nExec=brave --new-window\n").is_none());
        let kali = rewrite_kali_desktop("[Desktop Entry]\nName=Kali\nExec=pkexec tool --name kali\nNoDisplay=true\n").unwrap();
        assert!(kali.contains("Exec=sudo -E tool --name kali"));
        assert!(!kali.contains("NoDisplay"));
    }
}
