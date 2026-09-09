//! Port of `kyth_welcome.services.security` / `services.security_container` —
//! the Kali Linux distrobox toolbox lifecycle (create/export/remove) and the
//! host-side (Flatpak) security tools grid.
//!
//! The create/export/remove commands are still `["bash", "-c", <script>]`
//! here, matching the Python original exactly (`Worker(cmd)` in
//! `page_software_security_kali.py`) rather than the shell-free argv
//! convention most of this crate uses — the script itself does privileged
//! work (`sudo -A podman`, `distrobox create --root`) that only makes sense
//! as a shell pipeline, and `box_name`/`box_image` are always this module's
//! own constants, never caller-supplied text, so there is nothing here for a
//! caller to inject through.
//!
//! Not ported: `KaliInstallProgressTracker`'s line-by-line percentage
//! parser. The Tauri shell's job model (see `commands/security.rs`) reports
//! running/complete/failed like every other long-running Hub action
//! (`just_run`, Flatpak install, Guardian repairs) rather than a live
//! progress bar — a deliberate, documented simplification, not a silent
//! feature gap; the container still gets created identically either way.

use std::time::Duration;

pub const DEFAULT_KALI_BOX: &str = "kali";
pub const DEFAULT_KALI_IMAGE: &str = "docker.io/kalilinux/kali-rolling";

/// Wraps a shell word in single quotes the way Python's `!r` does for the
/// plain identifiers this module ever calls it on (no embedded quotes).
/// `box_name`/`box_image` only ever come from this module's own constants,
/// never from a caller — this exists for defense in depth, not because
/// untrusted text reaches it.
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KaliContainerInfo {
    pub image: String,
    pub privileged: bool,
    pub security_options: Vec<String>,
}

impl KaliContainerInfo {
    pub fn socket_capable(&self) -> bool {
        self.image.contains("kali")
            && self.privileged
            && self
                .security_options
                .iter()
                .any(|opt| opt == "label=disable")
    }
}

/// Parses the deliberately line-oriented `podman inspect --format` output:
/// image name, then `true`/`false`, then a space-joined security-opt list.
pub fn parse_kali_inspect_output(output: &str) -> KaliContainerInfo {
    let lines: Vec<&str> = output.lines().collect();
    KaliContainerInfo {
        image: lines
            .first()
            .map(|l| l.trim().to_string())
            .unwrap_or_default(),
        privileged: lines.get(1).map(|l| l.trim() == "true").unwrap_or(false),
        security_options: lines
            .get(2)
            .map(|l| l.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default(),
    }
}

fn inspect_argv(name: &str) -> Vec<String> {
    [
        "sudo", "-n", "podman", "inspect", name, "--format",
        "{{.ImageName}}\n{{.HostConfig.Privileged}}\n{{range .HostConfig.SecurityOpt}}{{.}} {{end}}",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

pub fn inspect_kali_box(name: &str) -> Option<KaliContainerInfo> {
    let output =
        crate::system::process::run_bounded(&inspect_argv(name), Duration::from_secs(10)).ok()?;
    if !output.status.success() {
        return None;
    }
    Some(parse_kali_inspect_output(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

pub fn is_socket_capable_kali_box(name: &str) -> bool {
    inspect_kali_box(name).is_some_and(|info| info.socket_capable())
}

/// The three Kali metapackage tiers the create card offers. Fixed set —
/// never a free-form package name — so the create command stays a bounded
/// template, not a generic "apt install anything" bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KaliTier {
    Headless,
    Default,
    Everything,
}

impl KaliTier {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "headless" => Some(Self::Headless),
            "default" => Some(Self::Default),
            "everything" => Some(Self::Everything),
            _ => None,
        }
    }

    pub fn meta_package(self) -> &'static str {
        match self {
            Self::Headless => "kali-linux-headless",
            Self::Default => "kali-linux-default",
            Self::Everything => "kali-linux-everything",
        }
    }

    pub fn has_gui(self) -> bool {
        matches!(self, Self::Default | Self::Everything)
    }
}

/// Run once after any bulk `distrobox-export --app ...`: tags Kali-exported
/// launchers with a distinct menu category, strips NoDisplay/OnlyShowIn/
/// NotShowIn so they actually surface in the host menu, rewrites any
/// pkexec/kdesu/gksu escalation to `sudo -E`, and repoints Zenmap through
/// kyth-distrobox-root-launch. Lives in `build_files/kyth-kali-desktop-fixup`
/// (installed to /usr/bin) so this flow and the `ujust setup-kali-box`/
/// `export-kali-apps` CLI recipes fix up launchers identically.
const DESKTOP_FILE_REWRITE_SCRIPT: &str = "kyth-kali-desktop-fixup";

/// Recreate the box if it exists but isn't rootful/privileged, or if its
/// security posture looks right yet it won't actually start (stale runtime
/// state under /run/containers/storage after e.g. a reboot or storage
/// driver change). Otherwise install the chosen tool metapackage with
/// debconf preseeded to avoid install-time prompts, grant the container
/// user passwordless sudo, and (GUI tiers only) bulk-export .desktop
/// launchers and fix them up for the host menu. Mirrors
/// `build_kali_create_command` in `services/security.py` exactly.
pub fn build_kali_create_command(box_name: &str, box_image: &str, tier: KaliTier) -> Vec<String> {
    let meta = tier.meta_package();
    let has_gui = tier.has_gui();
    let box_q = shell_single_quote(box_name);
    let image_q = shell_single_quote(box_image);
    let export_step = if has_gui {
        format!(
            " && distrobox enter --root {box_name} -- bash -c 'for f in /usr/share/applications/*.desktop; do app=$(basename $f .desktop); distrobox-export --app $app 2>/dev/null || true; done'\n{DESKTOP_FILE_REWRITE_SCRIPT}"
        )
    } else {
        String::new()
    };
    let script = format!(
        r#"set -euo pipefail
box={box_q}
image={image_q}
rootless_exists=0
distrobox list --no-color 2>/dev/null | grep -q "^${{box}}\b" && rootless_exists=1 || true
if [[ "${{rootless_exists}}" -eq 1 ]]; then
    echo "Removing rootless ${{box}}; raw socket tools require a rootful Kali box..."
    distrobox stop "${{box}}" --yes 2>/dev/null || distrobox stop "${{box}}" 2>/dev/null || true
    distrobox rm --force "${{box}}" 2>/dev/null || distrobox rm "${{box}}" --yes 2>/dev/null || true
    podman rm -f "${{box}}" 2>/dev/null || true
fi
rootful_exists=0
sudo -A podman inspect "${{box}}" >/dev/null 2>&1 && rootful_exists=1 || true
if [[ "${{rootful_exists}}" -eq 1 ]]; then
    _image=$(sudo -A podman inspect "${{box}}" --format '{{{{.ImageName}}}}' 2>/dev/null || true)
    _privileged=$(sudo -A podman inspect "${{box}}" --format '{{{{.HostConfig.Privileged}}}}' 2>/dev/null || true)
    _security_opts=$(sudo -A podman inspect "${{box}}" --format '{{{{range .HostConfig.SecurityOpt}}}}{{{{.}}}} {{{{end}}}}' 2>/dev/null || true)
    _needs_recreate=0
    if [[ "${{_image}}" != *kali* ]] || [[ "${{_privileged}}" != "true" ]] || [[ "${{_security_opts}}" != *label=disable* ]]; then
        _needs_recreate=1
    elif ! sudo -A podman start "${{box}}" >/dev/null 2>&1; then
        echo "${{box}} matches the security policy but will not start (stale runtime state); recreating..."
        _needs_recreate=1
    fi
    if [[ "${{_needs_recreate}}" -eq 1 ]]; then
        echo "Recreating ${{box}} with privileged rootful networking and SELinux label disabled..."
        distrobox stop --root "${{box}}" --yes 2>/dev/null || distrobox stop --root "${{box}}" 2>/dev/null || true
        distrobox rm --root --force "${{box}}" 2>/dev/null || distrobox rm --root "${{box}}" --yes 2>/dev/null || true
        sudo -A podman rm -f "${{box}}" 2>/dev/null || true
        rootful_exists=0
    fi
fi
if [[ "${{rootful_exists}}" -eq 0 ]]; then
    distrobox create --root --image "${{image}}" --name "${{box}}" --yes --additional-flags '--privileged --security-opt label=disable'
fi
distrobox enter --root {box_name} -- bash -c "export DEBIAN_FRONTEND=noninteractive; (printf '%s\n' 'popularity-contest popularity-contest/participate boolean false' 'encfs encfs/security-information boolean true' 'encfs encfs/security-information seen true' 'console-setup console-setup/charmap47 select UTF-8' 'samba-common samba-common/dhcp boolean false' 'macchanger macchanger/automatically_run boolean false' 'kismet-capture-common kismet-capture-common/install-users string' 'kismet-capture-common kismet-capture-common/install-setuid boolean true' 'wireshark-common wireshark-common/install-setuid boolean true' 'sslh sslh/inetd_or_standalone select standalone' | sudo debconf-set-selections) || true; sudo -E apt-get install -y -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold {meta}" && distrobox enter --root {box_name} -- bash -c "echo '${{USER}} ALL=(root) NOPASSWD: ALL' | sudo tee /etc/sudoers.d/kali-user-nopasswd > /dev/null; sudo chmod 0440 /etc/sudoers.d/kali-user-nopasswd; mkdir -p /root/.config/gtk-3.0; printf '[Settings]\ngtk-icon-theme-name = hicolor\n' > /root/.config/gtk-3.0/settings.ini; if command -v nmap >/dev/null 2>&1; then printf '#!/bin/sh\nexec sudo /usr/bin/nmap \"\$@\"\n' | sudo tee /usr/local/bin/nmap > /dev/null; sudo chmod 755 /usr/local/bin/nmap; fi"{export_step}"#
    );
    vec!["bash".to_string(), "-c".to_string(), script]
}

/// Bulk-export every .desktop file the container ships (exit 2 if none),
/// grant passwordless sudo, then fix up the exported launchers for the host
/// menu. Mirrors the GUI-tier export step in `build_kali_create_command` for
/// a box that already exists.
pub fn build_kali_export_command(box_name: &str) -> Vec<String> {
    let script = format!(
        r#"distrobox enter --root {box_name} -- bash -c 'shopt -s nullglob; files=(/usr/share/applications/*.desktop); if [ ${{#files[@]}} -eq 0 ]; then exit 2; fi; n=0; for f in ${{files[@]}}; do app=$(basename $f .desktop); distrobox-export --app $app 2>&1 && n=$((n+1)) || echo skip: $app; done; echo EXPORTED:$n'
_rc=$?; [ "$_rc" -eq 2 ] && exit 2
distrobox enter --root {box_name} -- bash -c "echo '${{USER}} ALL=(root) NOPASSWD: ALL' | sudo tee /etc/sudoers.d/kali-user-nopasswd > /dev/null; sudo chmod 0440 /etc/sudoers.d/kali-user-nopasswd"
{DESKTOP_FILE_REWRITE_SCRIPT}"#
    );
    vec!["bash".to_string(), "-c".to_string(), script]
}

/// Parses the `EXPORTED:<n>` marker line `build_kali_export_command` emits.
pub fn parse_kali_export_count(stdout: &str) -> Option<u32> {
    stdout
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("EXPORTED:")?.trim().parse().ok())
}

/// Stop and remove both a rootless and rootful box by this name, forcing
/// backend container removal if distrobox still lists it afterward, then
/// delete any exported launchers pointing at it.
pub fn build_kali_remove_command(box_name: &str) -> Vec<String> {
    let box_q = shell_single_quote(box_name);
    let script = format!(
        r#"set -euo pipefail
box={box_q}
appdir="${{HOME}}/.local/share/applications"

echo "Stopping ${{box}} if it is running..."
distrobox stop "${{box}}" --yes 2>/dev/null || distrobox stop "${{box}}" 2>/dev/null || true
distrobox stop --root "${{box}}" --yes 2>/dev/null || distrobox stop --root "${{box}}" 2>/dev/null || true

echo "Removing ${{box}}..."
distrobox rm --force "${{box}}" || distrobox rm "${{box}}" --yes || true
distrobox rm --root --force "${{box}}" 2>/dev/null || distrobox rm --root "${{box}}" --yes 2>/dev/null || true

if distrobox list --no-color 2>/dev/null | grep -q "^${{box}}\b" || sudo -A podman inspect "${{box}}" >/dev/null 2>&1; then
    echo "Distrobox still lists ${{box}}; forcing backend container removal..."
    if command -v podman >/dev/null 2>&1; then
        podman rm -f "${{box}}" 2>/dev/null || true
        sudo -A podman rm -f "${{box}}" 2>/dev/null || true
    fi
    if command -v docker >/dev/null 2>&1; then
        docker rm -f "${{box}}" 2>/dev/null || true
    fi
fi

if distrobox list --no-color 2>/dev/null | grep -q "^${{box}}\b" || sudo -A podman inspect "${{box}}" >/dev/null 2>&1; then
    echo "ERROR: ${{box}} still exists after removal attempts." >&2
    exit 1
fi

if [[ -d "${{appdir}}" ]]; then
    removed=0
    while IFS= read -r -d "" f; do
        if grep -qE -- "--name[[:space:]]+${{box}}|-n[[:space:]]+${{box}}|kyth-distrobox-root-launch[[:space:]]+${{box}}\b" "$f" 2>/dev/null; then
            rm -f "$f"
            removed=$((removed + 1))
        fi
    done < <(find "${{appdir}}" -maxdepth 1 -type f -name "*.desktop" -print0)
    echo "Removed ${{removed}} exported launcher(s)."
fi

update-desktop-database "${{appdir}}" 2>/dev/null || true
kbuildsycoca6 --noincremental 2>/dev/null || true
echo "Kali box is stopped and removed."
"#
    );
    vec!["bash".to_string(), "-c".to_string(), script]
}

/// Terminal candidates in preference order, mirroring the
/// `shutil.which` chain in `page_software_security_kali.py::_sec_enter_box`
/// — narrowed to fixed `/usr/bin` paths rather than a PATH search, matching
/// how the rest of this crate checks for optional binaries (e.g.
/// `commands/updates.rs`'s `ksshaskpass` check).
const TERMINAL_CANDIDATES: [(&str, &str); 3] = [
    ("/usr/bin/konsole", "konsole"),
    ("/usr/bin/xdg-terminal-exec", "xdg-terminal-exec"),
    ("/usr/bin/xterm", "xterm"),
];

pub fn detect_terminal_with(exists: impl Fn(&str) -> bool) -> Option<&'static str> {
    TERMINAL_CANDIDATES
        .iter()
        .find(|(path, _)| exists(path))
        .map(|(_, name)| *name)
}

pub fn detect_terminal() -> Option<&'static str> {
    detect_terminal_with(|path| std::path::Path::new(path).exists())
}

/// Argv to open a terminal running `distrobox enter --root <box_name>`.
/// konsole takes `-e argv...` directly; other terminals (xdg-terminal-exec,
/// xterm) take the command after `--`, matching the Python original.
pub fn kali_enter_argv(terminal: &str, box_name: &str) -> Vec<String> {
    let inner = ["distrobox", "enter", "--root", box_name];
    let mut argv = vec![terminal.to_string()];
    argv.push(if terminal == "konsole" {
        "-e".to_string()
    } else {
        "--".to_string()
    });
    argv.extend(inner.into_iter().map(String::from));
    argv
}

/// A host-side (Flatpak) security tool the grid can install/launch/remove.
/// Fixed catalog — the Tauri bridge validates any incoming id against this
/// list rather than accepting an arbitrary Flatpak id, so the grid can't
/// become a generic "install/run any Flatpak" surface.
#[derive(Debug, Clone)]
pub struct SecHostTool {
    pub flatpak: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
}

pub const SEC_HOST_TOOLS: [SecHostTool; 2] = [
    SecHostTool {
        flatpak: "org.wireshark.Wireshark",
        name: "Wireshark",
        desc: "Network packet capture and protocol analyser. Live capture and deep inspection of hundreds of protocols.",
    },
    SecHostTool {
        flatpak: "com.portswigger.BurpSuite",
        name: "Burp Suite Community",
        desc: "Web application security testing — proxy, scanner, intruder, repeater, and decoder.",
    },
];

pub fn find_sec_host_tool(flatpak_id: &str) -> Option<&'static SecHostTool> {
    SEC_HOST_TOOLS
        .iter()
        .find(|tool| tool.flatpak == flatpak_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_socket_capable_inspect_output() {
        let info = parse_kali_inspect_output(
            "docker.io/kalilinux/kali-rolling\ntrue\nlabel=disable seccomp=unconfined \n",
        );
        assert!(info.socket_capable());
        assert_eq!(
            info.security_options,
            vec!["label=disable", "seccomp=unconfined"]
        );
    }

    #[test]
    fn non_kali_image_is_not_socket_capable() {
        let info = parse_kali_inspect_output("docker.io/library/ubuntu\ntrue\nlabel=disable\n");
        assert!(!info.socket_capable());
    }

    #[test]
    fn unprivileged_box_is_not_socket_capable() {
        let info =
            parse_kali_inspect_output("docker.io/kalilinux/kali-rolling\nfalse\nlabel=disable\n");
        assert!(!info.socket_capable());
    }

    #[test]
    fn missing_security_opt_is_not_socket_capable() {
        let info = parse_kali_inspect_output(
            "docker.io/kalilinux/kali-rolling\ntrue\nseccomp=unconfined\n",
        );
        assert!(!info.socket_capable());
    }

    #[test]
    fn truncated_output_defaults_closed() {
        let info = parse_kali_inspect_output("docker.io/kalilinux/kali-rolling\n");
        assert!(!info.socket_capable());
        assert!(!info.privileged);
        assert!(info.security_options.is_empty());
    }

    #[test]
    fn tier_parses_known_values_and_rejects_others() {
        assert_eq!(KaliTier::parse("headless"), Some(KaliTier::Headless));
        assert_eq!(KaliTier::parse("default"), Some(KaliTier::Default));
        assert_eq!(KaliTier::parse("everything"), Some(KaliTier::Everything));
        assert_eq!(KaliTier::parse("kali-linux-everything"), None);
        assert_eq!(KaliTier::parse(""), None);
    }

    #[test]
    fn only_gui_tiers_report_has_gui() {
        assert!(!KaliTier::Headless.has_gui());
        assert!(KaliTier::Default.has_gui());
        assert!(KaliTier::Everything.has_gui());
    }

    #[test]
    fn create_command_is_a_bash_script_naming_the_chosen_metapackage() {
        let argv =
            build_kali_create_command(DEFAULT_KALI_BOX, DEFAULT_KALI_IMAGE, KaliTier::Headless);
        assert_eq!(argv[0], "bash");
        assert_eq!(argv[1], "-c");
        assert!(argv[2].contains("kali-linux-headless"));
        assert!(argv[2].contains(DEFAULT_KALI_IMAGE));
        // Headless has no GUI apps to export.
        assert!(!argv[2].contains("distrobox-export"));
    }

    #[test]
    fn gui_tier_create_command_exports_desktop_files() {
        let argv =
            build_kali_create_command(DEFAULT_KALI_BOX, DEFAULT_KALI_IMAGE, KaliTier::Default);
        assert!(argv[2].contains("kali-linux-default"));
        assert!(argv[2].contains("distrobox-export"));
        assert!(argv[2].contains(DESKTOP_FILE_REWRITE_SCRIPT));
    }

    #[test]
    fn export_command_targets_the_named_box_and_rewrites_launchers() {
        let argv = build_kali_export_command("kali");
        assert!(argv[2].contains("distrobox enter --root kali"));
        assert!(argv[2].contains("EXPORTED:"));
        assert!(argv[2].contains(DESKTOP_FILE_REWRITE_SCRIPT));
    }

    #[test]
    fn parses_exported_count_from_trailing_marker_line() {
        assert_eq!(parse_kali_export_count("skip: foo\nEXPORTED:5\n"), Some(5));
        assert_eq!(parse_kali_export_count("no marker here"), None);
        assert_eq!(parse_kali_export_count("EXPORTED:0"), Some(0));
    }

    #[test]
    fn remove_command_stops_and_removes_both_rootless_and_rootful_boxes() {
        let argv = build_kali_remove_command("kali");
        assert!(argv[2].contains("distrobox stop \"${box}\""));
        assert!(argv[2].contains("distrobox rm --root"));
        assert!(argv[2].contains("kbuildsycoca6"));
    }

    #[test]
    fn detects_first_available_terminal_in_preference_order() {
        assert_eq!(detect_terminal_with(|_| false), None);
        assert_eq!(
            detect_terminal_with(|p| p == "/usr/bin/xterm"),
            Some("xterm")
        );
        assert_eq!(
            detect_terminal_with(|p| p == "/usr/bin/konsole" || p == "/usr/bin/xterm"),
            Some("konsole"),
            "konsole must win when multiple terminals are present"
        );
    }

    #[test]
    fn konsole_uses_dash_e_other_terminals_use_dash_dash() {
        assert_eq!(
            kali_enter_argv("konsole", "kali"),
            vec!["konsole", "-e", "distrobox", "enter", "--root", "kali"],
        );
        assert_eq!(
            kali_enter_argv("xterm", "kali"),
            vec!["xterm", "--", "distrobox", "enter", "--root", "kali"],
        );
    }

    #[test]
    fn sec_host_tools_catalog_is_the_two_curated_tools() {
        assert_eq!(SEC_HOST_TOOLS.len(), 2);
        assert!(find_sec_host_tool("org.wireshark.Wireshark").is_some());
        assert!(find_sec_host_tool("com.portswigger.BurpSuite").is_some());
        assert!(find_sec_host_tool("org.not.Curated").is_none());
    }
}
