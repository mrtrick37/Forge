//! Native replacement for the `kyth-user-polish` login/session utility.
//!
//! The Python implementation remains in the source tree as a parity fixture
//! until the image and rollback acceptance gates close.  This binary owns the
//! installed entry point and deliberately keeps every external operation
//! bounded, best-effort, and shell-free.

use regex::Regex;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use kyth_shared::atomic_io::atomic_write_text;
use kyth_shared::desktop_polish::{self, FOLDER_METADATA, MIME_DEFAULTS, USER_FOLDERS};
use kyth_shared::system::desktop_plasma::{kreadconfig_argv, kwriteconfig_argv};
use kyth_shared::system::process::{run_bounded, run_bounded_command};

const WALLPAPER: &str = "/usr/share/wallpapers/kyth/contents/images/1920x1080.svg";
const KSPLASH_THEME: &str = "org.kythos.desktop";
const FAVORITES: &str = "applications:kyth-welcome.desktop,applications:kyth-app-store.desktop,applications:com.valvesoftware.Steam.desktop,applications:com.brave.Browser.desktop,applications:chromium-browser.desktop,applications:dev.vencord.Vesktop.desktop,applications:org.kde.konsole.desktop";

struct RunLock {
    _file: File,
}

fn home() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"))
}

fn which(program: &str) -> Option<String> {
    env::var_os("PATH")
        .map(|path| {
            env::split_paths(&path)
                .map(|dir| dir.join(program))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter()
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
}

fn run(program: &str, args: &[&str], timeout: u64) -> Option<std::process::Output> {
    let mut argv = vec![program.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    run_bounded(&argv, Duration::from_secs(timeout)).ok()
}

fn run_path(path: &Path, args: &[&str], timeout: u64) -> Option<std::process::Output> {
    let mut command = std::process::Command::new(path);
    command.args(args);
    run_bounded_command(command, Duration::from_secs(timeout)).ok()
}

fn successful(program: &str, args: &[&str], timeout: u64) -> bool {
    run(program, args, timeout).is_some_and(|output| output.status.success())
}

fn acquire_run_lock(path: &Path) -> Option<RunLock> {
    let parent = path.parent()?;
    if fs::create_dir_all(parent).is_err() {
        return None;
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)
        .ok()?;
    // The descriptor stays inside RunLock for the complete process lifetime.
    let result = unsafe {
        libc::flock(
            std::os::fd::AsRawFd::as_raw_fd(&file),
            libc::LOCK_EX | libc::LOCK_NB,
        )
    };
    if result == 0 {
        Some(RunLock { _file: file })
    } else {
        None
    }
}

fn stamp_path(home: &Path, name: &str) -> PathBuf {
    home.join(".local/share/kyth").join(name)
}

fn already_run(home: &Path, name: &str) -> bool {
    stamp_path(home, name).is_file()
}

fn has_polish_stamp(home: &Path) -> bool {
    fs::read_dir(home.join(".local/share/kyth"))
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("user-polish-")
        })
}

fn mark_run(home: &Path, name: &str) {
    let _ = atomic_write_text(stamp_path(home, name), "", Some(0o644));
}

fn write_config(
    binary: &str,
    file: &str,
    groups: &[&str],
    key: &str,
    value: &str,
    value_type: Option<&str>,
) {
    let _ = run_bounded(
        &kwriteconfig_argv(binary, file, groups, key, value, value_type),
        Duration::from_secs(5),
    );
}

fn read_config(binary: &str, file: &str, group: &str, key: &str) -> String {
    run_bounded(
        &kreadconfig_argv(binary, file, group, key),
        Duration::from_secs(5),
    )
    .ok()
    .filter(|output| output.status.success())
    .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
    .unwrap_or_default()
}

fn apply_foundation(home: &Path) {
    let _ = fs::create_dir_all(home.join(".config"));
    for folder in USER_FOLDERS {
        let _ = fs::create_dir_all(home.join(folder));
    }
    for (relative, content) in FOLDER_METADATA {
        let path = home.join(relative);
        if !path.exists() {
            let _ = atomic_write_text(path, content, Some(0o644));
        }
    }

    let bluetooth = home.join(".config/bluedevilglobalrc");
    if let Ok(content) = fs::read_to_string(&bluetooth) {
        let filtered = content
            .lines()
            .filter(|line| {
                let value = line.trim();
                !(value.ends_with("_powered=false")
                    && value[..value.len() - "_powered=false".len()]
                        .chars()
                        .all(|c| c.is_ascii_hexdigit() || c == ':'))
            })
            .map(|line| format!("{line}\n"))
            .collect::<String>();
        if filtered != content {
            let _ = atomic_write_text(bluetooth, &filtered, None);
        }
    }
    if which("bluetoothctl").is_some() {
        let _ = run("bluetoothctl", &["power", "on"], 10);
    }

    if which("xdg-mime").is_some() {
        for entry in MIME_DEFAULTS {
            let _ = run("xdg-mime", &["default", entry.0, entry.1], 5);
        }
    }

    let dmrc = home.join(".dmrc");
    if let Ok(content) = fs::read_to_string(&dmrc) {
        let mut changed = false;
        let rewritten = content
            .lines()
            .map(|line| {
                let trimmed = line.trim_start();
                if trimmed.to_ascii_lowercase().starts_with("session=") {
                    let value = trimmed
                        .split_once('=')
                        .map(|(_, value)| value.trim().trim_matches(['"', '\'']))
                        .unwrap_or_default();
                    let lowered = value.to_ascii_lowercase().replace('\\', "/");
                    if lowered.contains("plasmax11") || lowered.contains("/xsessions/") {
                        changed = true;
                        return format!("Session=plasma.desktop\n");
                    }
                }
                format!("{line}\n")
            })
            .collect::<String>();
        if changed {
            let _ = atomic_write_text(dmrc, &rewritten, None);
        }
    }

    let legacy = home.join(".config/plasma-workspace/env/10-kyth-qemu-safe.sh");
    if let Ok(content) = fs::read_to_string(&legacy) {
        if content.contains("systemd-detect-virt") {
            let _ = fs::remove_file(legacy);
        }
    }
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn place_definitions(home: &Path) -> Vec<(String, &'static str, &'static str)> {
    let folders = [
        ("", "Home", "user-home"),
        ("Desktop", "Desktop", "user-desktop"),
        ("Documents", "Documents", "folder-documents"),
        ("Downloads", "Downloads", "folder-download"),
        ("Games", "Games", "applications-games"),
        ("Music", "Music", "folder-music"),
        ("Pictures", "Pictures", "folder-pictures"),
        ("Screenshots", "Screenshots", "folder-pictures"),
        ("Public", "Public", "folder-publicshare"),
        ("Templates", "Templates", "folder-templates"),
        ("Videos", "Videos", "folder-videos"),
    ];
    let mut result = folders
        .into_iter()
        .map(|(folder, title, icon)| {
            let suffix = if folder.is_empty() {
                String::new()
            } else {
                format!("/{folder}")
            };
            (format!("file://{}{suffix}", home.display()), title, icon)
        })
        .collect::<Vec<_>>();
    result.extend([
        (String::from("trash:/"), "Trash", "user-trash"),
        (String::from("network:/"), "Network", "network-workgroup"),
    ]);
    result
}

fn bookmark(href: &str, title: &str, icon: &str) -> String {
    format!(" <bookmark href=\"{}\">\n  <title>{}</title>\n  <info>\n   <metadata owner=\"http://freedesktop.org\">\n    <bookmark:icon name=\"{}\" />\n   </metadata>\n  </info>\n </bookmark>\n", escape_xml(href), escape_xml(title), escape_xml(icon))
}

fn apply_places(home: &Path) -> Result<bool, String> {
    let path = home.join(".local/share/user-places.xbel");
    let existing = fs::read_to_string(&path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            error
        } else {
            error
        }
    });
    let (mut content, new_file) = match existing {
        Ok(content) => {
            if content.contains("<!DOCTYPE") {
                return Err(format!(
                    "{}: DOCTYPE declarations are not allowed",
                    path.display()
                ));
            }
            if !content.contains("<xbel") || !content.contains("</xbel>") {
                return Err("unexpected root element: xbel".into());
            }
            (content, false)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (
            String::from(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xbel version=\"1.0\">\n</xbel>\n",
            ),
            true,
        ),
        Err(error) => return Err(error.to_string()),
    };
    let href_re = Regex::new(r#"<bookmark\b[^>]*\bhref\s*=\s*\"([^\"]*)\""#)
        .map_err(|error| error.to_string())?;
    let present = href_re
        .captures_iter(&content)
        .filter_map(|capture| capture.get(1).map(|value| value.as_str().to_string()))
        .collect::<std::collections::HashSet<_>>();
    let additions = place_definitions(home)
        .into_iter()
        .filter(|(href, _, _)| !present.contains(href))
        .map(|(href, title, icon)| bookmark(&href, title, icon))
        .collect::<String>();
    if additions.is_empty() && !new_file {
        return Ok(false);
    }
    if let Some(position) = content.rfind("</xbel>") {
        content.insert_str(position, &additions);
    } else {
        return Err("unexpected root element: xbel".into());
    }
    atomic_write_text(path, &content, Some(0o644)).map_err(|error| error.to_string())?;
    Ok(true)
}

fn cleanup_autostart(home: &Path) {
    for name in [
        "kyth-windows-friendly-defaults.desktop",
        "kyth-user-polish.desktop",
    ] {
        let _ = fs::remove_file(home.join(".config/autostart").join(name));
    }
}

fn apply_plasma(binary: &str, home: &Path, force: bool) {
    write_config(
        binary,
        "ksplashrc",
        &["KSplash"],
        "Engine",
        "KSplashQML",
        None,
    );
    write_config(
        binary,
        "ksplashrc",
        &["KSplash"],
        "Theme",
        KSPLASH_THEME,
        None,
    );
    write_config(
        binary,
        "plasma-localerc",
        &["Translations"],
        "LANGUAGE",
        "en_US",
        None,
    );
    write_config(
        binary,
        "plasma-localerc",
        &["Formats"],
        "LC_TIME",
        "en_US.UTF-8",
        None,
    );
    for (key, value, ty) in [
        ("Enabled", "true", Some("bool")),
        ("Default Wallet", "kdewallet", None),
        ("Local Wallet", "kdewallet", None),
        ("Use One Wallet", "true", Some("bool")),
        ("Close When Idle", "false", Some("bool")),
        ("Close on Screensaver", "false", Some("bool")),
        ("Leave Open", "true", Some("bool")),
    ] {
        write_config(binary, "kwalletrc", &["Wallet"], key, value, ty);
    }
    let color = read_config(binary, "kdeglobals", "General", "ColorScheme");
    let theme = read_config(binary, "plasmarc", "General", "Theme");
    if force || (color.is_empty() && theme.is_empty()) {
        for (key, value) in [
            ("ColorScheme", "KythDark"),
            ("font", "Inter,10,-1,5,400,0,0,0,0,0,Regular"),
            ("fixed", "Cascadia Code,10,-1,5,400,0,0,0,0,0,Regular"),
            ("smallestReadableFont", "Inter,8,-1,5,400,0,0,0,0,0,Regular"),
            ("toolBarFont", "Inter,9,-1,5,400,0,0,0,0,0,Regular"),
            ("menuFont", "Inter,10,-1,5,400,0,0,0,0,0,Regular"),
        ] {
            write_config(binary, "kdeglobals", &["General"], key, value, None);
        }
        write_config(
            binary,
            "kdeglobals",
            &["Icons"],
            "Theme",
            "Papirus-Dark",
            None,
        );
        write_config(
            binary,
            "kdeglobals",
            &["KDE"],
            "LookAndFeelPackage",
            "org.kde.breezedark.desktop",
            None,
        );
        write_config(binary, "plasmarc", &["Theme"], "name", "kyth-dark", None);
        if Path::new(WALLPAPER).is_file() {
            write_config(
                binary,
                "plasma-org.kde.plasma.desktop-appletsrc",
                &["Containments", "1", "Wallpaper", "org.kde.image", "General"],
                "Image",
                WALLPAPER,
                None,
            );
        }
    }
    let favorites = read_config(binary, "kickoffrc", "Favorites", "FavoriteURLs");
    if force || favorites.is_empty() {
        write_config(
            binary,
            "kickoffrc",
            &["Favorites"],
            "FavoriteURLs",
            FAVORITES,
            None,
        );
    }

    let mission_center = which("flatpak")
        .is_some_and(|_| successful("flatpak", &["info", "io.missioncenter.MissionCenter"], 10));
    if mission_center {
        write_config(
            binary,
            "kglobalshortcutsrc",
            &["services", "io.missioncenter.MissionCenter.desktop"],
            "_launch",
            "Ctrl+Shift+Esc",
            None,
        );
        write_config(
            binary,
            "kglobalshortcutsrc",
            &["org.kde.plasma-systemmonitor.desktop"],
            "_launch",
            "none,none,System Monitor",
            None,
        );
    } else {
        write_config(
            binary,
            "kglobalshortcutsrc",
            &["org.kde.plasma-systemmonitor.desktop"],
            "_launch",
            "Ctrl+Shift+Esc,none,System Monitor",
            None,
        );
    }
    write_config(
        binary,
        "kdeglobals",
        &["KDE"],
        "SingleClick",
        "false",
        Some("bool"),
    );
    write_config(
        binary,
        "kickoffrc",
        &["General"],
        "highlightNewlyInstalledApps",
        "false",
        None,
    );
    write_config(
        binary,
        "klipperrc",
        &["General"],
        "KeepClipboardContents",
        "true",
        None,
    );
    write_config(
        binary,
        "klipperrc",
        &["General"],
        "MaxClipItems",
        "25",
        None,
    );
    write_config(
        binary,
        "kglobalshortcutsrc",
        &["org.kde.klipper.desktop"],
        "show_clipboard_history",
        "Meta+V,Ctrl+Alt+V,Show Clipboard History",
        None,
    );
    write_config(
        binary,
        "kglobalshortcutsrc",
        &["services", "org.kde.dolphin.desktop"],
        "_launch",
        "Meta+E",
        None,
    );
    write_config(
        binary,
        "kglobalshortcutsrc",
        &["org.kde.spectacle.desktop"],
        "RectangularRegionScreenShot",
        "Meta+Shift+S,Meta+Shift+S,Capture Rectangular Region",
        None,
    );
    let screenshot_uri = format!("file://{}/Screenshots", home.display());
    let screenshot_path = format!("{}/Screenshots", home.display());
    write_config(
        binary,
        "spectaclerc",
        &["General"],
        "defaultSaveLocation",
        &screenshot_uri,
        None,
    );
    write_config(
        binary,
        "spectaclerc",
        &["General"],
        "lastSaveAsLocation",
        &screenshot_uri,
        None,
    );
    write_config(
        binary,
        "spectaclerc",
        &["General"],
        "useReleaseToCapture",
        "true",
        Some("bool"),
    );
    write_config(
        binary,
        "spectaclerc",
        &["ImageSave"],
        "translatedScreenshotsFolder",
        &screenshot_path,
        None,
    );
    for (key, value, ty) in [
        ("Autolock", "true", Some("bool")),
        ("LockGracePeriod", "5", None),
        ("LockOnResume", "true", Some("bool")),
        ("Timeout", "15", None),
    ] {
        write_config(binary, "kscreenlockerrc", &["Daemon"], key, value, ty);
    }
    write_config(
        binary,
        "kscreenlockerrc",
        &["Greeter", "Wallpaper", "org.kde.image", "General"],
        "Image",
        WALLPAPER,
        None,
    );
    for (group, key, value, ty) in [
        ("TabBox", "LayoutName", "thumbnail_grid", None),
        ("TabBox", "ShowDesktop", "false", Some("bool")),
        ("TabBoxAlternative", "LayoutName", "thumbnail_grid", None),
        ("org.kde.kdecoration2", "ButtonsOnLeft", "", None),
        ("org.kde.kdecoration2", "ButtonsOnRight", "IAX", None),
        ("org.kde.kdecoration2", "library", "org.kde.breeze", None),
        ("org.kde.kdecoration2", "theme", "Breeze", None),
        ("Plugins", "desktopchangeosdEnabled", "false", Some("bool")),
    ] {
        write_config(binary, "kwinrc", &[group], key, value, ty);
    }
    if let Some(qdbus) = which("qdbus6")
        .or_else(|| which("qdbus-qt6"))
        .or_else(|| which("qdbus"))
    {
        let _ = run_path(
            Path::new(&qdbus),
            &["org.kde.KWin", "/KWin", "reconfigure"],
            10,
        );
    }
    if !Path::new("/etc/xdg/kwinrc.d/99-kyth-latency.conf").is_file() {
        write_config(
            binary,
            "kwinrc",
            &["Compositing"],
            "AllowTearing",
            "false",
            Some("bool"),
        );
    }
    write_config(
        binary,
        "plasma-discoverrc",
        &["UpdatesNotifier"],
        "UseNotifications",
        "false",
        Some("bool"),
    );
    for (key, value, ty) in [
        ("RememberOpenedTabs", "true", Some("bool")),
        ("ShowFullPath", "true", Some("bool")),
        ("UseTabForSplitViewSwitch", "true", Some("bool")),
        ("ShowSpaceInfo", "true", Some("bool")),
        ("BrowseThroughArchives", "true", Some("bool")),
        ("ShowToolTips", "true", Some("bool")),
        ("PreviewSize", "32", None),
    ] {
        write_config(
            binary,
            "dolphinrc",
            &[if key == "PreviewSize" {
                "DetailsMode"
            } else {
                "General"
            }],
            key,
            value,
            ty,
        );
    }
    write_config(binary, "dolphinrc", &["PreviewSettings"], "Plugins", "audiothumbnail,comicbookthumbnail,cursorthumbnail,djvuthumbnail,ebookthumbnail,exrthumbnail,ffmpegthumbs,imagethumbnail,jpegthumbnail,kraorathumbnail,windowsexethumbnail", None);
}

fn rewrite_brave(home: &Path) {
    let source = [
        "/var/lib/flatpak/exports/share/applications/com.brave.Browser.desktop",
        "/usr/share/applications/com.brave.Browser.desktop",
        "/usr/local/share/applications/com.brave.Browser.desktop",
    ]
    .iter()
    .map(Path::new)
    .find(|path| path.is_file());
    let Some(source) = source else {
        return;
    };
    let destination = home.join(".local/share/applications/com.brave.Browser.desktop");
    let Ok(content) = fs::read_to_string(source) else {
        return;
    };
    let browser = Regex::new(r"(com\.brave\.Browser)(\s|$)").expect("valid brave regex");
    let executable =
        Regex::new(r"(brave-browser|brave)(\s|$)").expect("valid brave executable regex");
    let rewritten = content
        .lines()
        .map(|line| {
            if !line.starts_with("Exec=") {
                return format!("{line}\n");
            }
            let mut line = line.replace("--password-store=basic", "--password-store=kwallet5");
            if !line.contains("--password-store=kwallet5")
                && !line.contains("--password-store=kwallet")
            {
                line = browser
                    .replace(&line, "$1 --password-store=kwallet5$2")
                    .into_owned();
                if !line.contains("flatpak run") {
                    line = executable
                        .replace(&line, "$1 --password-store=kwallet5$2")
                        .into_owned();
                }
            }
            format!("{line}\n")
        })
        .collect::<String>();
    let _ = atomic_write_text(&destination, &rewritten, Some(0o644));
}

fn copy_if_needed(source: &Path, destination: &Path, mode: Option<u32>) {
    if let Some(parent) = destination.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(data) = fs::read(source) {
        let _ = atomic_write_text(destination, &String::from_utf8_lossy(&data), mode);
    }
}

fn run_optional(program: &str, args: &[&str]) {
    if which(program).is_some() {
        let _ = run(program, args, 30);
    }
}

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let force = args.iter().any(|arg| arg == "--force");
    if args.iter().any(|arg| arg != "--force") {
        eprintln!("Usage: kyth-user-polish [--force]");
        return ExitCode::from(2);
    }
    let home = home();
    let stamp_name = format!("user-polish-{}", desktop_polish::VERSION);
    let first_polish = !has_polish_stamp(&home);
    if already_run(&home, &stamp_name) && !force {
        cleanup_autostart(&home);
        return ExitCode::SUCCESS;
    }
    let _ = fs::create_dir_all(home.join(".local/share/kyth"));
    let Some(_lock) = acquire_run_lock(&home.join(".local/share/kyth/.user-polish.lock")) else {
        cleanup_autostart(&home);
        return ExitCode::SUCCESS;
    };
    if already_run(&home, &stamp_name) && !force {
        cleanup_autostart(&home);
        return ExitCode::SUCCESS;
    }

    run_optional("xdg-user-dirs-update", &[]);
    apply_foundation(&home);
    if let Err(error) = apply_places(&home) {
        eprintln!(
            "kyth-user-polish: places-{}: {error}",
            desktop_polish::PLACES_VERSION
        );
    }
    if let Some(binary) = which("kwriteconfig6") {
        apply_plasma(&binary, &home, force);
    }
    rewrite_brave(&home);
    run_optional("kyth-set-kickoff-icon", &[]);

    let layout = Path::new("/usr/bin/kyth-apply-desktop-layout");
    let layout_arg = force
        .then_some("--force")
        .or_else(|| first_polish.then_some("--initial"));
    if layout.is_file() {
        if let Some(arg) = layout_arg {
            let Some(output) = run_path(layout, &[arg], 30) else {
                eprintln!("kyth-user-polish: desktop layout failed to start");
                return ExitCode::from(1);
            };
            if !output.status.success() {
                eprintln!("kyth-user-polish: desktop layout failed");
                return ExitCode::from(1);
            }
        }
    }
    run_optional("kbuildsycoca6", &["--noincremental"]);
    run_optional("kyth-steam-game-export", &[]);
    run_optional("kyth-web-app-categorize", &[]);
    run_optional("kyth-vscode-wallet", &[]);
    if (force || first_polish) && Path::new("/usr/bin/kyth-apply-role-preset").is_file() {
        let profile = fs::read_to_string(home.join(".local/share/kyth/profile"))
            .ok()
            .and_then(|text| {
                text.lines()
                    .next()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "everyday".into());
        let _ = run_path(
            Path::new("/usr/bin/kyth-apply-role-preset"),
            &[&profile],
            30,
        );
    }

    let desktop = home.join("Desktop");
    let welcome = Path::new("/usr/share/applications/kyth-welcome.desktop");
    let welcome_destination = desktop.join("kyth-welcome.desktop");
    if desktop.is_dir() && welcome.is_file() {
        let existing = fs::read_to_string(&welcome_destination).unwrap_or_default();
        let shipped = fs::read_to_string(welcome).unwrap_or_default();
        let refresh = desktop_polish::should_refresh_pulse_desktop_shortcut(&existing, &shipped);
        if !shipped.is_empty()
            && ((!welcome_destination.exists() && (force || first_polish)) || refresh)
        {
            copy_if_needed(welcome, &welcome_destination, Some(0o700));
        }
    }
    let recycle = Path::new("/usr/share/kyth/kyth-recycle-bin.desktop");
    let recycle_destination = desktop.join("kyth-recycle-bin.desktop");
    if desktop.is_dir() && !recycle_destination.exists() && recycle.is_file() {
        copy_if_needed(recycle, &recycle_destination, Some(0o644));
    }
    mark_run(&home, &stamp_name);
    cleanup_autostart(&home);
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn places_are_idempotent_and_reject_doctype() {
        let directory = tempdir().unwrap();
        assert!(apply_places(directory.path()).unwrap());
        assert!(!apply_places(directory.path()).unwrap());
        let places =
            fs::read_to_string(directory.path().join(".local/share/user-places.xbel")).unwrap();
        assert_eq!(places.matches("href=\"trash:/\"").count(), 1);
        fs::write(
            directory.path().join(".local/share/user-places.xbel"),
            "<!DOCTYPE xbel><xbel></xbel>",
        )
        .unwrap();
        assert!(apply_places(directory.path())
            .unwrap_err()
            .contains("DOCTYPE"));
    }

    #[test]
    fn manifest_matches_python_shape() {
        assert_eq!(MIME_DEFAULTS.len(), 29);
        assert!(MIME_DEFAULTS.contains(&("org.videolan.VLC.desktop", "audio/flac")));
        assert_eq!(USER_FOLDERS.len(), 10);
    }
}
