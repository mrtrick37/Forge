//! Plasma desktop-layout preset: discovery, command projections, and the
//! apply gate shared with the native `kyth-apply-desktop-layout` binary.
//!
//! Discovery helpers and argv projections are side-effect free: callers
//! supply filesystem roots or execute the returned argv themselves.  Only
//! the `*_bin.rs` entry point invokes `kreadconfig`/`kwriteconfig`, `qdbus`,
//! or a Plasma script.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LauncherChoice {
    Single(String),
    Alternatives(Vec<String>),
}

pub const LAYOUT_VERSION: &str = "kyth-comfort-v4";
pub const CONFIG_FILE: &str = "plasma-org.kde.plasma.desktop-appletsrc";

pub const TRAY_ITEMS: [&str; 9] = [
    "org.kde.plasma.networkmanagement",
    "org.kde.plasma.volume",
    "org.kde.plasma.bluetooth",
    "org.kde.plasma.battery",
    "org.kde.plasma.notifications",
    "org.kde.plasma.clipboard",
    "org.kde.plasma.devicenotifier",
    "org.kde.plasma.printmanager",
    "org.kde.kdeconnect",
];

pub const HIDDEN_TRAY_ITEMS: [&str; 2] = [
    "org.kde.plasma.keyboardindicator",
    "org.kde.plasma.mediacontroller",
];

/// Mirrors `apply_desktop_layout`'s version gate: an already-stamped layout
/// (current or legacy marker) is current unless `--force` is given.
pub fn is_layout_current(current: Option<&str>, legacy: Option<&str>) -> bool {
    matches!(current, Some("kyth-comfort-v4" | "kyth-comfort-v2" | "kyth-comfort-v3"))
        || legacy == Some("windows-familiar-v1")
}

/// Outcome of the launcher gate before any Plasma mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutDecision {
    AlreadyCurrent,
    Refused,
    Apply,
}

/// Mirrors the Python launcher's exit codes: `0` when the stamped layout is
/// already current, `64` when neither `--force` nor `--initial` was passed,
/// otherwise proceed to the qdbus apply step.
pub fn decide_layout(force: bool, initial: bool, current: Option<&str>, legacy: Option<&str>) -> LayoutDecision {
    if !force && is_layout_current(current, legacy) {
        LayoutDecision::AlreadyCurrent
    } else if !force && !initial {
        LayoutDecision::Refused
    } else {
        LayoutDecision::Apply
    }
}

/// Renders the Plasma evaluate-script for the given launcher/tray CSVs,
/// byte-identical in shape to the Python `js_template.format(...)` output.
pub fn render_layout_script(launchers_csv: &str, tray_csv: &str, hidden_csv: &str) -> String {
    format!(
        "var launchers = \"{launchers_csv}\";\nvar trayItems = \"{tray_csv}\";\nvar hiddenTrayItems = \"{hidden_csv}\";\n{LAYOUT_SCRIPT_TAIL}"
    )
}

const LAYOUT_SCRIPT_TAIL: &str = r#"
function safeSet(object, key, value) {
    try {
        object[key] = value;
    } catch (e) {
    }
}

function writeConfig(object, groups, values) {
    try {
        object.currentConfigGroup = groups;
        for (var key in values) {
            object.writeConfig(key, values[key]);
        }
        object.reloadConfig();
    } catch (e) {
    }
}

function removeExistingPanels() {
    var ids = [];
    for (var i = 0; i < panelIds.length; ++i) {
        ids.push(panelIds[i]);
    }
    for (var i = 0; i < ids.length; ++i) {
        var panel = panelById(ids[i]);
        if (panel) {
            panel.remove();
        }
    }
}

function uniqueScreens() {
    var seen = [];
    var desktopsArray = desktops();
    for (var i = 0; i < desktopsArray.length; ++i) {
        var screen = desktopsArray[i].screen;
        if (seen.indexOf(screen) === -1) {
            seen.push(screen);
        }
    }
    if (seen.length === 0) {
        seen.push(0);
    }
    return seen;
}

function configureDesktops() {
    var desktopsArray = desktops();
    for (var i = 0; i < desktopsArray.length; ++i) {
        var desktop = desktopsArray[i];
        desktop.wallpaperPlugin = "org.kde.image";
        writeConfig(desktop, ["Wallpaper", "org.kde.image", "General"], {
            "Image": "/usr/share/wallpapers/kyth/contents/images/1920x1080.svg"
        });
        writeConfig(desktop, ["General"], {
            "ToolBoxButtonState": "topcenter"
        });
    }
}

function addKythDefaultPanel(screen) {
    var panel = new Panel;
    safeSet(panel, "screen", screen);
    panel.location = "bottom";
    panel.height = 42;
    safeSet(panel, "alignment", "left");
    safeSet(panel, "floating", false);
    safeSet(panel, "floatingApplets", false);

    var kickoff = panel.addWidget("org.kde.plasma.kickoff");
    writeConfig(kickoff, ["General"], {
        "icon": "kyth-kickoff",
        "favoritesPortedToKAstats": true,
        "alphaSort": true,
        "showActionButtonCaptions": true
    });

    var tasks = panel.addWidget("org.kde.plasma.icontasks");
    writeConfig(tasks, ["General"], {
        "launchers": launchers,
        "showOnlyCurrentDesktop": false,
        "showOnlyCurrentScreen": false,
        "showOnlyCurrentActivity": false,
        "groupingStrategy": 1,
        "maxStripes": 1,
        "showToolTips": true,
        "wheelEnabled": "AllTask",
        "indicateAudioStreams": true,
        "highlightWindows": true,
        "middleClickAction": "NewInstance"
    });

    panel.addWidget("org.kde.plasma.marginsseparator");
    panel.addWidget("org.kde.plasma.panelspacer");

    var tray = panel.addWidget("org.kde.plasma.systemtray");
    writeConfig(tray, ["General"], {
        "extraItems": trayItems,
        "hiddenItems": hiddenTrayItems,
        "knownItems": trayItems + "," + hiddenTrayItems,
        "showAllItems": false
    });

    var clock = panel.addWidget("org.kde.plasma.digitalclock");
    writeConfig(clock, ["Appearance"], {
        "showDate": false,
        "dateFormat": "shortDate",
        "showSeconds": false
    });

    panel.addWidget("org.kde.plasma.showdesktop");
}

removeExistingPanels();
configureDesktops();
var screens = uniqueScreens();
for (var i = 0; i < screens.length; ++i) {
    addKythDefaultPanel(screens[i]);
}
"#;

pub fn desktop_name(value: &str) -> &str {
    value.strip_prefix("applications:").unwrap_or(value)
}

pub fn desktop_exists(value: &str, roots: &[impl AsRef<Path>]) -> bool {
    let name = desktop_name(value);
    roots.iter().any(|root| root.as_ref().join(name).is_file())
}

pub fn filter_available_launchers(choices: &[LauncherChoice], roots: &[impl AsRef<Path>]) -> Vec<String> {
    choices.iter().filter_map(|choice| {
        let candidates = match choice {
            LauncherChoice::Single(value) => std::slice::from_ref(value),
            LauncherChoice::Alternatives(values) => values.as_slice(),
        };
        candidates.iter().find(|candidate| desktop_exists(candidate, roots)).cloned()
    }).collect()
}

pub fn default_launchers() -> Vec<LauncherChoice> {
    vec![
        LauncherChoice::Single("applications:kyth-welcome.desktop".into()),
        LauncherChoice::Single("applications:kyth-app-store.desktop".into()),
        LauncherChoice::Alternatives(vec!["applications:com.valvesoftware.Steam.desktop".into(), "applications:steam.desktop".into()]),
        LauncherChoice::Alternatives(vec!["applications:com.brave.Browser.desktop".into(), "applications:chromium-browser.desktop".into()]),
        LauncherChoice::Single("applications:org.kde.dolphin.desktop".into()),
        LauncherChoice::Single("applications:org.kde.konsole.desktop".into()),
    ]
}

pub fn qdbus_candidates() -> [&'static str; 3] {
    ["qdbus6", "qdbus-qt6", "qdbus"]
}

pub fn kreadconfig_argv(binary: &str, file: &str, group: &str, key: &str) -> Vec<String> {
    vec![binary.into(), "--file".into(), file.into(), "--group".into(), group.into(), "--key".into(), key.into()]
}

pub fn kwriteconfig_argv(binary: &str, file: &str, groups: &[&str], key: &str, value: &str, value_type: Option<&str>) -> Vec<String> {
    let mut argv = vec![binary.into(), "--file".into(), file.into()];
    for group in groups { argv.extend(["--group".into(), (*group).into()]); }
    argv.extend(["--key".into(), key.into()]);
    if let Some(value_type) = value_type { argv.extend(["--type".into(), value_type.into()]); }
    argv.push(value.into());
    argv
}

pub fn evaluate_plasma_argv(qdbus: &str, script: &str) -> Vec<String> {
    vec![qdbus.into(), "org.kde.plasmashell".into(), "/PlasmaShell".into(), "org.kde.PlasmaShell.evaluateScript".into(), script.into()]
}

/// Launcher sets for `kyth-apply-role-preset`, mirroring
/// `apply_role_preset` (`work`/`both` use the everyday set).
pub const EVERYDAY_LAUNCHERS: [&str; 7] = [
    "applications:kyth-welcome.desktop",
    "applications:kyth-app-store.desktop",
    "applications:com.brave.Browser.desktop",
    "applications:org.kde.dolphin.desktop",
    "applications:org.libreoffice.LibreOffice.desktop",
    "applications:eu.betterbird.Betterbird.desktop",
    "applications:org.kde.konsole.desktop",
];

pub const GAMING_LAUNCHERS: [&str; 7] = [
    "applications:kyth-welcome.desktop",
    "applications:kyth-app-store.desktop",
    "applications:com.valvesoftware.Steam.desktop",
    "applications:com.brave.Browser.desktop",
    "applications:dev.vencord.Vesktop.desktop",
    "applications:org.kde.dolphin.desktop",
    "applications:org.kde.konsole.desktop",
];

/// Mirrors the launcher's alias normalization (`work`/`both` → `everyday`);
/// unknown profiles are `None` (usage + exit 64 upstream).
pub fn normalize_role_arg(arg: &str) -> Option<&'static str> {
    match arg {
        "work" | "both" | "everyday" => Some("everyday"),
        "gaming" => Some("gaming"),
        "dev" => Some("dev"),
        "creator" => Some("creator"),
        _ => None,
    }
}

/// Which layout set a validated profile applies (`dev`/`creator` reuse the
/// everyday layout, exactly as the Python launcher passed it).
pub fn role_layout_target(profile: &str) -> &'static str {
    match profile {
        "gaming" => "gaming",
        _ => "everyday",
    }
}

pub fn role_launchers(layout: &str) -> Option<&'static [&'static str; 7]> {
    match layout {
        "gaming" => Some(&GAMING_LAUNCHERS),
        "everyday" => Some(&EVERYDAY_LAUNCHERS),
        _ => None,
    }
}

/// Profile stamp written by `apply_role_preset`
/// (`~/.local/share/kyth/profile`, best-effort upstream).
pub fn profile_stamp_path(home: impl AsRef<Path>) -> PathBuf {
    home.as_ref().join(".local/share/kyth/profile")
}

/// State file consulted by `kyth-refresh-taskbar-pins` before touching
/// Plasma (`~/.local/share/kyth/taskbar-pins`).
pub fn taskbar_pins_state_path(home: impl AsRef<Path>) -> PathBuf {
    home.as_ref().join(".local/share/kyth/taskbar-pins")
}

/// Renders the taskbar-pins refresh script, matching the Python
/// `js_script` f-string shape (570 chars with a 3-char CSV).
pub fn render_pins_script(launchers_csv: &str) -> String {
    format!("var launchers = \"{launchers_csv}\";\n{PINS_SCRIPT_TAIL}\n")
}

const PINS_SCRIPT_TAIL: &str = r#"for (var i = 0; i < panelIds.length; ++i) {
    var panel = panelById(panelIds[i]);
    if (!panel) {
        continue;
    }
    var ids = panel.widgetIds;
    for (var j = 0; j < ids.length; ++j) {
        var widget = panel.widgetById(ids[j]);
        if (widget && widget.type === "org.kde.plasma.icontasks") {
            try {
                widget.currentConfigGroup = ["General"];
                widget.writeConfig("launchers", launchers);
                widget.reloadConfig();
            } catch (e) {
            }
        }
    }
}"#;

/// Renders the role-preset Plasma widget-update script, matching the Python
/// `js_script` f-string shape (double-quoted CSV header, single braces).
pub fn render_role_script(launchers_csv: &str, tray_csv: &str, hidden_csv: &str) -> String {
    format!(
        "var launchers = \"{launchers_csv}\";\nvar trayItems = \"{tray_csv}\";\nvar hiddenTrayItems = \"{hidden_csv}\";\n{ROLE_SCRIPT_TAIL}\n"
    )
}

const ROLE_SCRIPT_TAIL: &str = r#"
function writeConfig(object, groups, values) {
    try {
        object.currentConfigGroup = groups;
        for (var key in values) {
            object.writeConfig(key, values[key]);
        }
        object.reloadConfig();
    } catch (e) {
    }
}

for (var p = 0; p < panelIds.length; ++p) {
    var panel = panelById(panelIds[p]);
    if (!panel || !panel.widgets) {
        continue;
    }
    var widgets = panel.widgets();
    for (var i = 0; i < widgets.length; ++i) {
        var widget = widgets[i];
        if (widget.type === "org.kde.plasma.icontasks") {
            writeConfig(widget, ["General"], {
                "launchers": launchers,
                "showOnlyCurrentDesktop": false,
                "showOnlyCurrentScreen": false,
                "showOnlyCurrentActivity": false,
                "groupingStrategy": 1,
                "maxStripes": 1,
                "showToolTips": true,
                "wheelEnabled": "AllTask",
                "indicateAudioStreams": true,
                "highlightWindows": true,
                "middleClickAction": "NewInstance"
            });
        } else if (widget.type === "org.kde.plasma.systemtray") {
            writeConfig(widget, ["General"], {
                "extraItems": trayItems,
                "hiddenItems": hiddenTrayItems,
                "knownItems": trayItems + "," + hiddenTrayItems,
                "showAllItems": false
            });
        } else if (widget.type === "org.kde.plasma.digitalclock") {
            writeConfig(widget, ["Appearance"], {
                "showDate": false,
                "dateFormat": "shortDate",
                "showSeconds": false
            });
        }
    }
}"#;

/// Common roots used by the Python launcher discovery helper.
pub fn default_application_roots(home: impl AsRef<Path>) -> Vec<PathBuf> {
    let home = home.as_ref();
    vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
        home.join(".local/share/applications"),
        home.join(".local/share/flatpak/exports/share/applications"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn filters_first_available_launcher_alternative() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("steam.desktop"), "[Desktop Entry]\n").unwrap();
        let choices = vec![LauncherChoice::Alternatives(vec!["applications:missing.desktop".into(), "applications:steam.desktop".into()])];
        assert_eq!(filter_available_launchers(&choices, &[directory.path()]), vec!["applications:steam.desktop"]);
    }

    #[test]
    fn projects_nested_kwriteconfig_and_qdbus_argv() {
        assert_eq!(kreadconfig_argv("kreadconfig6", "kwinrc", "Compositing", "AllowTearing"), vec!["kreadconfig6", "--file", "kwinrc", "--group", "Compositing", "--key", "AllowTearing"]);
        assert_eq!(kwriteconfig_argv("kwriteconfig6", "kwinrc", &["Containments", "1", "General"], "foo", "bar", Some("string")), vec!["kwriteconfig6", "--file", "kwinrc", "--group", "Containments", "--group", "1", "--group", "General", "--key", "foo", "--type", "string", "bar"]);
        assert_eq!(evaluate_plasma_argv("qdbus6", "print('ok')")[3], "org.kde.PlasmaShell.evaluateScript");
    }

    #[test]
    fn default_launcher_shape_remains_stable() {
        assert_eq!(default_launchers().len(), 6);
        assert_eq!(qdbus_candidates(), ["qdbus6", "qdbus-qt6", "qdbus"]);
        assert_eq!(desktop_name("applications:foo.desktop"), "foo.desktop");
        assert_eq!(LAYOUT_VERSION, "kyth-comfort-v4");
    }

    #[test]
    fn layout_gate_mirrors_python_exit_codes() {
        // Already stamped: exit 0 without --force.
        assert_eq!(decide_layout(false, false, Some("kyth-comfort-v4"), None), LayoutDecision::AlreadyCurrent);
        assert_eq!(decide_layout(false, false, Some("kyth-comfort-v2"), None), LayoutDecision::AlreadyCurrent);
        assert_eq!(decide_layout(false, false, Some("kyth-comfort-v3"), None), LayoutDecision::AlreadyCurrent);
        assert_eq!(decide_layout(false, false, None, Some("windows-familiar-v1")), LayoutDecision::AlreadyCurrent);
        // No stamp, no flags: exit 64.
        assert_eq!(decide_layout(false, false, None, None), LayoutDecision::Refused);
        assert_eq!(decide_layout(false, false, Some("other"), None), LayoutDecision::Refused);
        // --force or --initial proceeds.
        assert_eq!(decide_layout(true, false, Some("kyth-comfort-v4"), None), LayoutDecision::Apply);
        assert_eq!(decide_layout(false, true, None, None), LayoutDecision::Apply);
        // Stamp check runs before the flag check: --initial on a stamped
        // layout still exits 0 unless --force is given.
        assert_eq!(decide_layout(false, true, Some("kyth-comfort-v4"), None), LayoutDecision::AlreadyCurrent);
    }

    #[test]
    fn role_arg_normalization_and_layout_target() {
        assert_eq!(normalize_role_arg("work"), Some("everyday"));
        assert_eq!(normalize_role_arg("both"), Some("everyday"));
        assert_eq!(normalize_role_arg("dev"), Some("dev"));
        assert_eq!(normalize_role_arg("nope"), None);
        assert_eq!(role_layout_target("dev"), "everyday");
        assert_eq!(role_layout_target("gaming"), "gaming");
        assert_eq!(role_launchers("everyday").unwrap().len(), 7);
        assert!(role_launchers("dev").is_none());
    }

    #[test]
    fn role_script_matches_python_template_shape() {
        let script = render_role_script("A,B", "t1", "h1");
        // Pinned against the Python js_script f-string output, verified
        // byte-identical (1774 chars with these CSVs; 2066 with real ones).
        assert_eq!(script.len(), 1774);
        assert!(script.starts_with("var launchers = \"A,B\";\nvar trayItems = \"t1\";\nvar hiddenTrayItems = \"h1\";\n\nfunction writeConfig"));
        assert!(script.ends_with("    }\n}\n"));
        assert!(!script.contains("{{") && !script.contains("}}"));
        assert!(script.contains("panelById(panelIds[p])"));
    }

    #[test]
    fn pins_script_matches_python_template_shape() {
        let script = render_pins_script("A,B");
        assert_eq!(script.len(), 570);
        assert!(script.starts_with("var launchers = \"A,B\";\nfor (var i = 0; i < panelIds.length; ++i)"));
        assert!(script.ends_with("    }\n}\n"));
        assert!(!script.contains("{{") && !script.contains("}}"));
    }

    #[test]
    fn layout_script_matches_python_template_shape() {
        let script = render_layout_script("a,b", "t1", "h1");
        // Pinned against the Python js_template.format() output (3383 chars):
        // any brace, blank-line, or widget drift changes the length.
        assert_eq!(script.len(), 3383);
        assert!(script.starts_with("var launchers = \"a,b\";\nvar trayItems = \"t1\";\nvar hiddenTrayItems = \"h1\";\n\nfunction safeSet"));
        assert!(script.ends_with("addKythDefaultPanel(screens[i]);\n}\n"));
        assert!(!script.contains("{{") && !script.contains("}}"));
        for marker in ["org.kde.plasma.kickoff", "org.kde.plasma.icontasks", "org.kde.plasma.systemtray", "org.kde.plasma.digitalclock", "org.kde.plasma.showdesktop", "/usr/share/wallpapers/kyth/contents/images/1920x1080.svg", "kyth-kickoff"] {
            assert!(script.contains(marker), "{marker}");
        }
    }
}
