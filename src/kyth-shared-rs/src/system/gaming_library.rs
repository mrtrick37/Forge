//! Gaming library scan — read-only, ports the library-scan part of
//! `src/kyth-welcome/page_gaming_library.py` + `kyth_shared.gaming`.
//! Lists which launchers are installed and how many library entries each
//! has, by inspecting `~/.steam`, Heroic `~/.config/heroic`, Lutris
//! `~/.local/share/lutris`, and Bottles `~/.local/share/bottles` — all
//! plain filesystem reads, no writes, no root.

use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct LauncherEntry {
    pub id: String,
    pub label: String,
    pub installed: bool,
    pub library_count: Option<usize>,
    pub path: String,
}

fn home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/root"))
}

fn count_entries(dir: &std::path::Path, glob_ext: &str) -> Option<usize> {
    if !dir.exists() {
        return None;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    let mut n = 0;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if glob_ext == "*" || name.ends_with(glob_ext) {
            n += 1;
        }
    }
    Some(n)
}

pub fn gaming_library_scan() -> Vec<LauncherEntry> {
    let h = home();
    let steam_path = h.join(".steam/steam/steamapps");
    let heroic_path = h.join(".config/heroic/GamesConfig");
    let lutris_path = h.join(".local/share/lutris/runners");
    let bottles_path = h.join(".local/share/bottles/bottles");
    vec![
        LauncherEntry {
            id: "com.valvesoftware.Steam".to_string(),
            label: "Steam".to_string(),
            installed: which_exists("steam") || steam_path.exists(),
            library_count: count_entries(&steam_path, ".acf"),
            path: steam_path.display().to_string(),
        },
        LauncherEntry {
            id: "com.heroicgameslauncher.hgl".to_string(),
            label: "Heroic Games Launcher".to_string(),
            installed: which_exists("heroic") || heroic_path.exists(),
            library_count: count_entries(&heroic_path, ".json"),
            path: heroic_path.display().to_string(),
        },
        LauncherEntry {
            id: "net.lutris.Lutris".to_string(),
            label: "Lutris".to_string(),
            installed: which_exists("lutris") || lutris_path.exists(),
            library_count: count_entries(&lutris_path, "*"),
            path: lutris_path.display().to_string(),
        },
        LauncherEntry {
            id: "com.usebottles.bottles".to_string(),
            label: "Bottles".to_string(),
            installed: which_exists("bottles") || bottles_path.exists(),
            library_count: count_entries(&bottles_path, "*"),
            path: bottles_path.display().to_string(),
        },
    ]
}

fn which_exists(bin: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| {
        let candidate = directory.join(bin);
        let Ok(metadata) = std::fs::metadata(candidate) else {
            return false;
        };
        if !metadata.is_file() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))]
        {
            true
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scan_returns_four_entries() {
        let v = gaming_library_scan();
        assert_eq!(v.len(), 4);
        assert!(v.iter().any(|e| e.id == "com.valvesoftware.Steam"));
    }
}
