//! Apply the fixed Kyth Kickoff icon preference for the current user.

use regex::Regex;
use std::{env, path::PathBuf, time::Duration};

fn kickoff_groups(content: &str) -> Vec<(String, String)> {
    let section = Regex::new(r"(?m)^\[Containments\]\[(\d+)\]\[Applets\]\[(\d+)\]").unwrap();
    let kickoff = Regex::new(r"(?m)^plugin=org\.kde\.plasma\.kickoff\s*$").unwrap();
    let mut groups = Vec::new();
    for found in section.find_iter(content) {
        let header = section.captures(found.as_str()).unwrap();
        let end = section
            .find_at(content, found.end())
            .map(|next| next.start())
            .unwrap_or(content.len());
        if kickoff.is_match(&content[found.end()..end]) {
            groups.push((header[1].to_string(), header[2].to_string()));
        }
    }
    groups
}

fn run(argv: Vec<String>) {
    let _ = kyth_shared::system::process::run_bounded(&argv, Duration::from_secs(5));
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let home = PathBuf::from(env::var_os("HOME").unwrap_or_else(|| "/root".into()));
    let aprc = args
        .windows(2)
        .find(|pair| pair[0] == "--aprc")
        .map(|pair| PathBuf::from(&pair[1]))
        .unwrap_or_else(|| home.join(".config/plasma-org.kde.plasma.desktop-appletsrc"));
    let autostart = args
        .windows(2)
        .find(|pair| pair[0] == "--autostart")
        .map(|pair| PathBuf::from(&pair[1]))
        .unwrap_or_else(|| home.join(".config/autostart/kyth-set-kickoff-icon.desktop"));
    let binary = ["kwriteconfig6", "kwriteconfig-qt6", "kwriteconfig"]
        .into_iter()
        .find(|candidate| {
            std::env::var("PATH").ok().is_some_and(|path| {
                path.split(':')
                    .map(|dir| PathBuf::from(dir).join(candidate))
                    .any(|path| path.is_file())
            })
        })
        .unwrap_or("kwriteconfig6");
    run(vec![
        binary.into(),
        "--file".into(),
        "kickoffrc".into(),
        "--group".into(),
        "General".into(),
        "--key".into(),
        "highlightNewlyInstalledApps".into(),
        "--type".into(),
        "bool".into(),
        "false".into(),
    ]);
    if let Ok(content) = std::fs::read_to_string(&aprc) {
        for (containment, applet) in kickoff_groups(&content) {
            let groups = [
                "Containments",
                containment.as_str(),
                "Applets",
                applet.as_str(),
                "Configuration",
                "General",
            ];
            let mut icon = vec![
                binary.into(),
                "--file".into(),
                aprc.to_string_lossy().into_owned(),
            ];
            for group in groups {
                icon.extend(["--group".into(), group.into()]);
            }
            icon.extend(["--key".into(), "icon".into(), "kyth-kickoff".into()]);
            run(icon);
            let mut highlight = vec![
                binary.into(),
                "--file".into(),
                aprc.to_string_lossy().into_owned(),
            ];
            for group in groups {
                highlight.extend(["--group".into(), group.into()]);
            }
            highlight.extend([
                "--key".into(),
                "highlightNewlyInstalledApps".into(),
                "--type".into(),
                "bool".into(),
                "false".into(),
            ]);
            run(highlight);
        }
    }
    let _ = std::fs::remove_file(autostart);
}

#[cfg(test)]
mod tests {
    use super::kickoff_groups;

    #[test]
    fn finds_only_kickoff_applets() {
        let content = "[Containments][3][Applets][7]\nplugin=org.kde.plasma.kickoff\n\n[Containments][3][Applets][8]\nplugin=org.kde.plasma.taskmanager\n";
        assert_eq!(kickoff_groups(content), [("3".into(), "7".into())]);
    }

    #[test]
    fn ignores_malformed_or_nested_section_ids() {
        assert!(
            kickoff_groups("[Containments][x][Applets][1]\nplugin=org.kde.plasma.kickoff\n")
                .is_empty()
        );
    }
}
