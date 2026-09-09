//! Port of `kyth_welcome.services.gaming.tools`'s static catalog and command
//! builders — the Gaming section's install/launch/uninstall tool grid and
//! the two one-shot capture fixes (Discord screen share, OBS PipeWire).
//!
//! Not ported here: the installed-tool probes (`_mangohud_installed`,
//! `_gamescope_installed`, `_vkbasalt_installed`, `_proton_cachyos_version`,
//! `_ntsync_state`, `_vulkan_state`) — those back the Gaming dashboard's
//! status grid, which the Tauri shell already covers a different way
//! (`audit-cache` + `gaming_library`, per `PARITY.md`'s Play row). Also not
//! ported: `page_gaming_tools_perf.py`'s per-game launch-option/sched-ext
//! profile builder — a separate, larger piece of remaining work, tracked in
//! `PARITY.md` rather than silently declared done here.

/// A Gaming tool tile: install/launch/uninstall, same shape as
/// `security_container::SecHostTool` but with its own catalog and a launch
/// argv that isn't always `flatpak run <id>` (OpenRGB launches a native
/// binary once installed).
#[derive(Debug, Clone)]
pub struct GamingTool {
    pub flatpak: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub launch: &'static [&'static str],
}

pub const GAMING_TOOLS: [GamingTool; 14] = [
    GamingTool { flatpak: "com.valvesoftware.Steam", name: "Steam", desc: "Valve's gaming platform plus PC games through Proton.", launch: &["flatpak", "run", "com.valvesoftware.Steam"] },
    GamingTool { flatpak: "net.lutris.Lutris", name: "Lutris", desc: "Battle.net, EA App, Ubisoft Connect, and other compatibility launchers.", launch: &["flatpak", "run", "net.lutris.Lutris"] },
    GamingTool { flatpak: "com.heroicgameslauncher.hgl", name: "Heroic Games Launcher", desc: "Epic Games, GOG, and Amazon Games library in one place.", launch: &["flatpak", "run", "com.heroicgameslauncher.hgl"] },
    GamingTool { flatpak: "com.usebottles.bottles", name: "Bottles", desc: "Best for running standalone .exe and .msi installers in isolated app environments.", launch: &["flatpak", "run", "com.usebottles.bottles"] },
    GamingTool { flatpak: "com.github.mtkennerly.ludusavi", name: "Ludusavi", desc: "Back up and restore game saves across Steam, Heroic, Lutris, and PC migrations.", launch: &["flatpak", "run", "com.github.mtkennerly.ludusavi"] },
    GamingTool { flatpak: "org.prismlauncher.PrismLauncher", name: "Prism Launcher", desc: "Minecraft launcher with modpacks, multiple instances, and Java version control.", launch: &["flatpak", "run", "org.prismlauncher.PrismLauncher"] },
    GamingTool { flatpak: "io.itch.itch", name: "Itch.io", desc: "Indie game store and library manager.", launch: &["flatpak", "run", "io.itch.itch"] },
    GamingTool { flatpak: "org.libretro.RetroArch", name: "RetroArch", desc: "Multi-system emulator frontend (NES, SNES, PS1, N64, …).", launch: &["flatpak", "run", "org.libretro.RetroArch"] },
    GamingTool { flatpak: "org.freedesktop.Piper", name: "Piper", desc: "GUI for configuring gaming mice — DPI, buttons, and LEDs.", launch: &["flatpak", "run", "org.freedesktop.Piper"] },
    GamingTool { flatpak: "org.openrgb.OpenRGB", name: "OpenRGB", desc: "Unified RGB lighting control for motherboards, RAM, GPUs, and peripherals. Pre-installed — RGB profiles are applied automatically at login.", launch: &["openrgb"] },
    GamingTool { flatpak: "io.github.benjamimgois.goverlay", name: "GOverlay", desc: "Graphical tuning for MangoHud and vkBasalt overlays — adjust metrics, colors, and presets without editing config files.", launch: &["flatpak", "run", "io.github.benjamimgois.goverlay"] },
    GamingTool { flatpak: "io.github.radiolamp.mangojuice", name: "MangoJuice", desc: "Lightweight MangoHud configuration editor for overlay layout and metrics.", launch: &["flatpak", "run", "io.github.radiolamp.mangojuice"] },
    GamingTool { flatpak: "com.dec05eba.gpu_screen_recorder", name: "GPU Screen Recorder", desc: "Near-zero overhead gameplay capture and instant replay using AMD/NVIDIA GPU encoding.", launch: &["flatpak", "run", "com.dec05eba.gpu_screen_recorder"] },
    GamingTool { flatpak: "dev.vencord.Vesktop", name: "Vesktop", desc: "Discord client with native Wayland support, better screenshare, and no telemetry.", launch: &["flatpak", "run", "dev.vencord.Vesktop"] },
];

pub fn find_gaming_tool(flatpak_id: &str) -> Option<&'static GamingTool> {
    GAMING_TOOLS.iter().find(|tool| tool.flatpak == flatpak_id)
}

/// `services/gaming/tools.py::discord_screenshare_fix_command` — grants the
/// Flatpak Discord client the Wayland/portal permissions screen share needs.
pub fn discord_screenshare_fix_command() -> Vec<String> {
    [
        "bash",
        "-c",
        "flatpak override --user com.discordapp.Discord \
         --env=ELECTRON_OZONE_PLATFORM_HINT=auto \
         --socket=wayland --socket=fallback-x11 --device=dri \
         --talk-name=org.freedesktop.portal.Desktop \
         --talk-name=org.kde.StatusNotifierWatcher",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// `services/gaming/tools.py::obs_pipewire_fix_command` — grants the
/// Flatpak OBS client the Wayland/PipeWire permissions capture needs.
pub fn obs_pipewire_fix_command() -> Vec<String> {
    [
        "bash",
        "-c",
        "flatpak override --user com.obsproject.Studio \
         --socket=wayland --socket=pulseaudio --device=dri \
         --talk-name=org.freedesktop.portal.Desktop",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// `page_gaming_fixes.py::_copy_prefix_reset_hint`'s static text — a safe,
/// non-destructive way to reset a Proton prefix (moves it aside as a
/// dated backup rather than deleting it).
pub fn prefix_reset_hint() -> &'static str {
    "# Replace APPID with the Steam app id. This moves the Proton prefix aside as a backup.\n\
     mv ~/.local/share/Steam/steamapps/compatdata/APPID \
     ~/.local/share/Steam/steamapps/compatdata/APPID.bak-$(date +%Y%m%d-%H%M%S)"
}

/// `page_gaming_fixes.py::_copy_support_snapshot_command`'s static text.
pub fn support_snapshot_command() -> &'static str {
    "kyth-device-info | tee ~/kyth-device-info.txt"
}

/// The two folders `page_gaming_fixes.py::_open_user_path` opens, keyed by a
/// fixed name so the Tauri bridge validates against this list instead of
/// accepting an arbitrary path from the webview.
pub fn game_folder_path(key: &str) -> Option<&'static str> {
    match key {
        "compatdata" => Some("~/.local/share/Steam/steamapps/compatdata"),
        "shadercache" => Some("~/.local/share/Steam/steamapps/shadercache"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_no_duplicate_flatpak_ids() {
        let mut ids: Vec<&str> = GAMING_TOOLS.iter().map(|tool| tool.flatpak).collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before);
    }

    #[test]
    fn finds_known_tool_and_rejects_unknown() {
        assert_eq!(
            find_gaming_tool("com.valvesoftware.Steam").unwrap().name,
            "Steam"
        );
        assert!(find_gaming_tool("org.not.Curated").is_none());
    }

    #[test]
    fn openrgb_launches_a_native_binary_not_flatpak_run() {
        let tool = find_gaming_tool("org.openrgb.OpenRGB").unwrap();
        assert_eq!(tool.launch, &["openrgb"]);
    }

    #[test]
    fn discord_fix_grants_wayland_and_portal_permissions() {
        let argv = discord_screenshare_fix_command();
        assert_eq!(argv[0], "bash");
        assert!(argv[2].contains("com.discordapp.Discord"));
        assert!(argv[2].contains("--socket=wayland"));
    }

    #[test]
    fn obs_fix_grants_pipewire_permissions() {
        let argv = obs_pipewire_fix_command();
        assert!(argv[2].contains("com.obsproject.Studio"));
        assert!(argv[2].contains("--socket=pulseaudio"));
    }

    #[test]
    fn game_folder_path_is_restricted_to_the_known_keys() {
        assert!(game_folder_path("compatdata").is_some());
        assert!(game_folder_path("shadercache").is_some());
        assert!(game_folder_path("../etc/passwd").is_none());
    }
}
