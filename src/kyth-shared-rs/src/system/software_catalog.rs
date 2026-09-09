//! Static App Store catalogs — ports `src/kyth-welcome/services/software_catalogs.py`.
//! Pure data, no I/O, no root. The Python page composes these into the
//! "Starter Packs" / "Familiar Apps" choosers; the web Hub reads the same
//! lists via `starter_packs` / `familiar_apps` Tauri commands so a single
//! place can evolve curated app IDs without touching widget code.

use serde::Serialize;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

fn command_output(argv: &[&str], timeout: Duration) -> Option<std::process::Output> {
    let args = argv
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    crate::system::process::run_bounded(&args, timeout).ok()
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogApp {
    pub id: String,
    pub label: String,
    pub selected: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StarterPack {
    pub name: String,
    pub desc: String,
    pub apps: Vec<CatalogApp>,
}

pub fn starter_packs() -> Vec<StarterPack> {
    vec![
        StarterPack {
            name: "Gaming".to_string(),
            desc: "Steam, Epic/GOG, compatibility launchers, saves, and standalone .exe support."
                .to_string(),
            apps: vec![
                CatalogApp {
                    id: "com.valvesoftware.Steam".to_string(),
                    label: "Steam".to_string(),
                    selected: true,
                    description:
                        "Digital game store and library for Windows and Linux-native titles."
                            .to_string(),
                },
                CatalogApp {
                    id: "com.heroicgameslauncher.hgl".to_string(),
                    label: "Heroic Games Launcher".to_string(),
                    selected: true,
                    description: "Launcher for Epic Games Store and GOG libraries.".to_string(),
                },
                CatalogApp {
                    id: "net.lutris.Lutris".to_string(),
                    label: "Lutris".to_string(),
                    selected: true,
                    description:
                        "Open-source game manager for Windows, GOG, Amazon, and emulated titles."
                            .to_string(),
                },
                CatalogApp {
                    id: "com.usebottles.bottles".to_string(),
                    label: "Bottles".to_string(),
                    selected: true,
                    description: "Run Windows software and games in isolated, sandboxed prefixes."
                        .to_string(),
                },
                CatalogApp {
                    id: "com.github.mtkennerly.ludusavi".to_string(),
                    label: "Ludusavi".to_string(),
                    selected: true,
                    description:
                        "Back up and restore PC game save files across hundreds of titles."
                            .to_string(),
                },
            ],
        },
        StarterPack {
            name: "Creator".to_string(),
            desc: "Streaming, editing, audio, images, and 3D creation.".to_string(),
            apps: vec![
                CatalogApp {
                    id: "com.obsproject.Studio".to_string(),
                    label: "OBS Studio".to_string(),
                    selected: true,
                    description:
                        "Screen recording and live streaming with obs-vkcapture-ready game capture."
                            .to_string(),
                },
                CatalogApp {
                    id: "org.kde.kdenlive".to_string(),
                    label: "Kdenlive".to_string(),
                    selected: true,
                    description: "Open-source non-linear video editor.".to_string(),
                },
                CatalogApp {
                    id: "org.audacityteam.Audacity".to_string(),
                    label: "Audacity".to_string(),
                    selected: true,
                    description: "Multi-track audio editor and recorder.".to_string(),
                },
                CatalogApp {
                    id: "org.gimp.GIMP".to_string(),
                    label: "GIMP".to_string(),
                    selected: true,
                    description: "GNU Image Manipulation Program.".to_string(),
                },
                CatalogApp {
                    id: "org.blender.Blender".to_string(),
                    label: "Blender".to_string(),
                    selected: true,
                    description: "3D modeling, animation, and rendering suite.".to_string(),
                },
            ],
        },
        StarterPack {
            name: "Everyday".to_string(),
            desc: "Browser, chat, media, passwords, app management, and local file sharing."
                .to_string(),
            apps: vec![
                CatalogApp {
                    id: "com.brave.Browser".to_string(),
                    label: "Brave Browser".to_string(),
                    selected: true,
                    description: "Privacy-focused web browser.".to_string(),
                },
                CatalogApp {
                    id: "com.discordapp.Discord".to_string(),
                    label: "Discord".to_string(),
                    selected: true,
                    description: "Voice, video, and text chat for communities.".to_string(),
                },
                CatalogApp {
                    id: "org.videolan.VLC".to_string(),
                    label: "VLC".to_string(),
                    selected: true,
                    description: "Plays virtually any video or audio file format.".to_string(),
                },
                CatalogApp {
                    id: "com.spotify.Client".to_string(),
                    label: "Spotify".to_string(),
                    selected: true,
                    description: "Stream music, podcasts, and playlists.".to_string(),
                },
                CatalogApp {
                    id: "org.localsend.localsend_app".to_string(),
                    label: "LocalSend".to_string(),
                    selected: true,
                    description: "Send files to nearby devices over the local network.".to_string(),
                },
            ],
        },
    ]
}

#[derive(Debug, Clone, Serialize)]
pub struct FamiliarApp {
    pub windows_name: String,
    pub description: String,
    pub flatpak_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppStreamApp {
    pub id: String,
    pub name: String,
    pub summary: String,
    /// Flathub's AppStream icon is deterministic from the validated app id.
    /// Returning it from the native catalog keeps the webview from inventing
    /// package metadata or shell commands.
    pub icon_url: String,
}

fn flathub_icon_url(app_id: &str) -> String {
    format!("https://dl.flathub.org/repo/appstream/x86_64/icons/128x128/{app_id}.png")
}

fn valid_flatpak_id(app_id: &str) -> bool {
    !app_id.is_empty()
        && app_id.len() <= 200
        && app_id.contains('.')
        && app_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

pub fn parse_appstream_results(raw: &str) -> Vec<AppStreamApp> {
    raw.lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let id = fields.next()?.trim();
            let name = fields.next()?.trim();
            let summary = fields.next().unwrap_or("").trim();
            if !valid_flatpak_id(id) {
                return None;
            }
            Some(AppStreamApp {
                id: id.into(),
                name: name.into(),
                summary: summary.into(),
                icon_url: flathub_icon_url(id),
            })
        })
        .take(30)
        .collect()
}

/// Query the installed Flathub catalog. This is intentionally bounded and
/// read-only; Flatpak remains the authority for what is currently available.
pub fn appstream_search(query: &str) -> Vec<AppStreamApp> {
    let query = query.trim();
    if query.is_empty()
        || query.len() > 80
        || !query
            .chars()
            .all(|c| c.is_alphanumeric() || c.is_ascii_punctuation() || c.is_whitespace())
    {
        return Vec::new();
    }
    let Some(output) = command_output(
        &[
            "flatpak",
            "search",
            "--columns=application,name,summary",
            query,
        ],
        Duration::from_secs(10),
    ) else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_appstream_results(&String::from_utf8_lossy(&output.stdout))
}

/// Project one catalog result into the explicit, user-scoped install argv
/// used by native surfaces. The caller still owns confirmation and execution;
/// arbitrary shell text never crosses this boundary.
pub fn flatpak_install_argv(app_id: &str) -> Option<Vec<String>> {
    if !valid_flatpak_id(app_id) {
        return None;
    }
    Some(
        ["flatpak", "install", "--user", "-y", "flathub", app_id]
            .into_iter()
            .map(String::from)
            .collect(),
    )
}

#[derive(Debug, Clone, Serialize)]
pub struct AppImageEntry {
    pub name: String,
    pub path: String,
    pub executable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstalledFlatpak {
    pub id: String,
    pub name: String,
    pub version: String,
    pub branch: String,
    pub arch: String,
    pub scope: String,
    pub icon_url: String,
}

pub fn parse_installed_flatpaks(raw: &str, scope: &str) -> Vec<InstalledFlatpak> {
    raw.lines()
        .filter_map(|line| {
            let mut fields = line.split('\t').map(str::trim);
            let id = fields.next()?.to_string();
            if !valid_flatpak_id(&id) {
                return None;
            }
            Some(InstalledFlatpak {
                id: id.clone(),
                name: fields.next().unwrap_or("").into(),
                version: fields.next().unwrap_or("").into(),
                branch: fields.next().unwrap_or("").into(),
                arch: fields.next().unwrap_or("").into(),
                scope: scope.into(),
                icon_url: flathub_icon_url(&id),
            })
        })
        .collect()
}

/// Return installed applications only.  This is deliberately read-only; the
/// Hub uses it to make uninstall choices explicit instead of accepting an
/// arbitrary application id from the webview.
pub fn installed_flatpaks() -> Vec<InstalledFlatpak> {
    let mut apps = Vec::new();
    for (scope, scope_arg) in [("user", "--user"), ("system", "--system")] {
        let Some(output) = command_output(
            &[
                "flatpak",
                "list",
                scope_arg,
                "--app",
                "--columns=application,name,version,branch,arch",
            ],
            Duration::from_secs(10),
        ) else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        apps.extend(parse_installed_flatpaks(
            &String::from_utf8_lossy(&output.stdout),
            scope,
        ));
    }
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps
}

/// Port of `services.flatpak.is_installed`'s no-cache fallback path: check
/// live installed state rather than trusting a cache this crate doesn't
/// maintain. Used by the App Store's own installed list and the Security
/// tab's host-tools grid to decide Install vs. Launch/Uninstall.
pub fn is_flatpak_installed(app_id: &str) -> bool {
    installed_flatpaks().iter().any(|app| app.id == app_id)
}

/// Make a discovered AppImage runnable.  Restrict the path to the three
/// directories that appimages() scans so the webview cannot chmod an
/// arbitrary user file.
pub fn make_appimage_executable(path: &str) -> Result<String, String> {
    let requested = PathBuf::from(path);
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/root"));
    let allowed = [
        home.join("Applications"),
        home.join(".local/bin"),
        home.join("Downloads"),
    ];
    let canonical = requested
        .canonicalize()
        .map_err(|_| "AppImage does not exist".to_string())?;
    if !canonical
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("appimage"))
        || !allowed.iter().any(|dir| canonical.starts_with(dir))
    {
        return Err("AppImage must be inside Applications, .local/bin, or Downloads".to_string());
    }
    let metadata =
        std::fs::metadata(&canonical).map_err(|_| "Could not inspect AppImage".to_string())?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(permissions.mode() | 0o111);
    std::fs::set_permissions(&canonical, permissions)
        .map_err(|err| format!("Could not make AppImage executable: {err}"))?;
    Ok(format!("{} is executable now.", canonical.display()))
}

pub fn import_appimage(path: &str) -> Result<String, String> {
    let source = PathBuf::from(path)
        .canonicalize()
        .map_err(|_| "AppImage does not exist".to_string())?;
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/root"));
    let allowed = [
        home.join("Applications"),
        home.join(".local/bin"),
        home.join("Downloads"),
    ];
    if !source
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("appimage"))
        || !allowed.iter().any(|dir| source.starts_with(dir))
    {
        return Err("Choose an AppImage from Applications, .local/bin, or Downloads".to_string());
    }
    let target_dir = home.join("Applications");
    std::fs::create_dir_all(&target_dir)
        .map_err(|err| format!("Could not create Applications folder: {err}"))?;
    let target = target_dir.join(
        source
            .file_name()
            .ok_or_else(|| "AppImage has no filename".to_string())?,
    );
    if source != target {
        std::fs::copy(&source, &target)
            .map_err(|err| format!("Could not import AppImage: {err}"))?;
    }
    make_appimage_executable(target.to_string_lossy().as_ref())
}

pub fn appimages() -> Vec<AppImageEntry> {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/root"));
    let dirs = [
        home.join("Applications"),
        home.join(".local/bin"),
        home.join("Downloads"),
    ];
    let mut result = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("appimage"))
            {
                let executable = std::fs::metadata(&path)
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false);
                result.push(AppImageEntry {
                    name: path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    path: path.display().to_string(),
                    executable,
                });
            }
        }
    }
    result.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    result
}

/// Curated “Windows → Koji” map — same list as `FAMILIAR_APPS` in Python, trimmed to
/// the most-searched entries so the bridge stays fast. Full list remains in Python for
/// the Qt Hub’s typeahead; this is enough for the web Hub’s fallback chooser.
pub fn familiar_apps() -> Vec<FamiliarApp> {
    vec![
        FamiliarApp {
            windows_name: "Photoshop".to_string(),
            description: "Use GIMP for photo editing and compositing.".to_string(),
            flatpak_id: "org.gimp.GIMP".to_string(),
        },
        FamiliarApp {
            windows_name: "Office".to_string(),
            description: "LibreOffice is the drop-in Office suite.".to_string(),
            flatpak_id: "org.libreoffice.LibreOffice".to_string(),
        },
        FamiliarApp {
            windows_name: "Steam".to_string(),
            description: "Install Steam from Flatpak.".to_string(),
            flatpak_id: "com.valvesoftware.Steam".to_string(),
        },
        FamiliarApp {
            windows_name: "Discord".to_string(),
            description: "Install Discord from Flatpak.".to_string(),
            flatpak_id: "com.discordapp.Discord".to_string(),
        },
        FamiliarApp {
            windows_name: "Spotify".to_string(),
            description: "Install Spotify from Flatpak.".to_string(),
            flatpak_id: "com.spotify.Client".to_string(),
        },
        FamiliarApp {
            windows_name: "VLC".to_string(),
            description: "Install VLC from Flatpak — plays everything.".to_string(),
            flatpak_id: "org.videolan.VLC".to_string(),
        },
        FamiliarApp {
            windows_name: "Chrome".to_string(),
            description: "Use Brave Browser for a familiar Chromium experience.".to_string(),
            flatpak_id: "com.brave.Browser".to_string(),
        },
        FamiliarApp {
            windows_name: "GeForce Experience".to_string(),
            description: "NVIDIA driver settings live in the Control Center — no extra app needed."
                .to_string(),
            flatpak_id: "".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_appstream_rows_and_skips_non_ids() {
        let apps = parse_appstream_results("Application\tName\tSummary\norg.example.App\tDemo\tA test app\ninvalid\tIgnored\tNo ID\norg.example.Bad/Path\tUnsafe\tNo icon path\n");
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].id, "org.example.App");
        assert!(apps[0].icon_url.ends_with("/org.example.App.png"));
    }

    #[test]
    fn parses_installed_flatpaks_with_explicit_scope() {
        let apps = parse_installed_flatpaks(
            "org.example.App\tDemo\t1.0\tstable\tx86_64\nheader\tbad\n",
            "user",
        );
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].scope, "user");
        assert_eq!(apps[0].version, "1.0");
        assert!(apps[0].icon_url.ends_with("/org.example.App.png"));
    }

    #[test]
    fn projects_only_valid_user_scoped_flatpak_installs() {
        assert_eq!(
            flatpak_install_argv("org.example.App").unwrap(),
            vec![
                "flatpak",
                "install",
                "--user",
                "-y",
                "flathub",
                "org.example.App"
            ],
        );
        assert!(flatpak_install_argv("not-an-app").is_none());
        assert!(flatpak_install_argv("org.example.App;reboot").is_none());
    }
}
