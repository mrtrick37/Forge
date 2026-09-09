//! Native exporter for Steam Flatpak desktop launchers.

use std::path::{Path, PathBuf};
use std::time::Duration;

fn run(program: &str, args: &[&str]) {
    let argv = std::iter::once(program.to_string())
        .chain(args.iter().map(|arg| (*arg).to_string()))
        .collect::<Vec<_>>();
    let _ = kyth_shared::system::process::run_bounded(&argv, Duration::from_secs(15));
}

fn copy_matching_icon(icon: &str, source: &Path, destination: &Path) {
    fn walk(icon: &str, current: &Path, root: &Path, destination: &Path) {
        let Ok(entries) = std::fs::read_dir(current) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(icon, &path, root, destination);
                continue;
            }
            if !path.is_file() || path.file_stem().and_then(|name| name.to_str()) != Some(icon) {
                continue;
            }
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let target = destination.join(relative);
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::copy(path, target);
        }
    }
    walk(icon, source, source, destination);
}

fn main() -> std::process::ExitCode {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/root".into());
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let source_apps = home.join(".var/app/com.valvesoftware.Steam/.local/share/applications");
    let source_icons = home.join(".var/app/com.valvesoftware.Steam/.local/share/icons/hicolor");
    let destination_apps = data_home.join("applications");
    let destination_icons = data_home.join("icons/hicolor");
    if !source_apps.is_dir() {
        println!("Steam game launcher export complete: 0 exported, 0 skipped.");
        return std::process::ExitCode::SUCCESS;
    }
    if std::fs::create_dir_all(&destination_apps).is_err() {
        return std::process::ExitCode::from(1);
    }
    let mut exported = 0;
    let mut skipped = 0;
    let Ok(entries) = std::fs::read_dir(source_apps) else {
        return std::process::ExitCode::from(1);
    };
    for entry in entries
        .flatten()
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("desktop"))
    {
        let source = entry.path();
        let Ok(content) = std::fs::read_to_string(&source) else {
            skipped += 1;
            continue;
        };
        let Some(rewrite) = kyth_shared::system::desktop_shortcuts::rewrite_steam_desktop(&content)
        else {
            skipped += 1;
            continue;
        };
        let target = destination_apps.join(format!("kyth-steam-{}.desktop", rewrite.appid));
        if kyth_shared::atomic_io::atomic_write_text(&target, &rewrite.content, Some(0o644))
            .is_err()
        {
            skipped += 1;
            continue;
        }
        if let Some(icon) = rewrite.icon.as_deref() {
            copy_matching_icon(icon, &source_icons, &destination_icons);
        }
        println!("Exported {}", rewrite.name);
        exported += 1;
    }
    run(
        "update-desktop-database",
        &[destination_apps.to_string_lossy().as_ref()],
    );
    if destination_icons.is_dir() {
        run(
            "gtk-update-icon-cache",
            &["-q", "-t", destination_icons.to_string_lossy().as_ref()],
        );
    }
    run("kbuildsycoca6", &["--noincremental"]);
    println!("Steam game launcher export complete: {exported} exported, {skipped} skipped.");
    std::process::ExitCode::SUCCESS
}
