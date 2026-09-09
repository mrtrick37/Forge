//! Packaging-only KRunner entry generator from the React route manifest.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
struct Manifest {
    destinations: Vec<Destination>,
}
#[derive(Deserialize)]
struct Destination {
    sections: Vec<Section>,
}
#[derive(Deserialize)]
struct Section {
    key: String,
    title: String,
    description: String,
}

fn slug(value: &str) -> String {
    let mut result = String::new();
    let mut separator = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if separator && !result.is_empty() {
                result.push('-');
            }
            result.push(ch.to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    if result.is_empty() {
        "page".into()
    } else {
        result
    }
}

fn main() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: kyth-hub-desktop-entries MANIFEST DEST_DIR".into());
    }
    let manifest: Manifest =
        serde_json::from_str(&std::fs::read_to_string(&args[0]).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let destination = PathBuf::from(&args[1]);
    std::fs::create_dir_all(&destination).map_err(|e| e.to_string())?;
    for section in manifest
        .destinations
        .into_iter()
        .flat_map(|destination| destination.sections)
    {
        let content = format!("[Desktop Entry]\nType=Application\nNoDisplay=true\nName=Kyth Hub: {}\nComment={}\nKeywords={};{};\nExec=/usr/bin/kyth-welcome-launch --page \"{}\"\nIcon=kyth\nTerminal=false\nCategories=Settings;\nX-KDE-StartupNotify=false\n", section.title, section.description, section.title, section.key, section.key);
        let path = destination.join(format!("kyth-hub-{}.desktop", slug(&section.key)));
        kyth_shared::atomic_io::atomic_write_text(path, &content, Some(0o644))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
