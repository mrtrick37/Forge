//! Port of `kyth_shared.system.pipewire` — quantum presets (N32).

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

const PRESETS: &[(&str, &str)] = &[("gaming", "128"), ("work", "256"), ("balanced", "256")];

pub fn available_audio_presets() -> Vec<String> {
    let mut v: Vec<String> = PRESETS.iter().map(|(k, _)| k.to_string()).collect();
    v.sort();
    v
}

pub fn apply_pipewire_quantum(preset: &str, dry_run: bool) -> (bool, String) {
    let q = match PRESETS.iter().find(|(k, _)| *k == preset).map(|(_, v)| *v) {
        Some(v) => v,
        None => return (false, format!("unknown preset {}", preset)),
    };
    if dry_run {
        return (true, format!("dry-run ok: {} quantum {}", preset, q));
    }
    let xdg = std::env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
    let conf_dir = Path::new(&xdg).join("pipewire/pipewire.conf.d");
    if let Err(e) = std::fs::create_dir_all(&conf_dir) {
        return (false, format!("mkdir failed: {}", e));
    }
    let target = conf_dir.join("99-kyth-quantum.conf");
    let tmp = conf_dir.join("99-kyth-quantum.conf.tmp");
    let content = format!(
        "# kyth quantum {}\ncontext.properties = {{\n  default.clock.quantum = {}\n}}\n",
        preset, q
    );
    if let Err(e) = std::fs::write(&tmp, &content) {
        return (false, format!("write failed: {}", e));
    }
    let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644));
    if let Err(e) = std::fs::rename(&tmp, &target) {
        return (false, format!("rename failed: {}", e));
    }
    (true, format!("applied {} quantum {}", preset, q))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_sorted() {
        let v = available_audio_presets();
        assert!(v.contains(&"gaming".to_string()));
        assert_eq!(v, {
            let mut s = v.clone();
            s.sort();
            s
        });
    }
    #[test]
    fn dry_run() {
        let (ok, msg) = apply_pipewire_quantum("gaming", true);
        assert!(ok);
        assert!(msg.contains("128"));
    }
    #[test]
    fn unknown() {
        let (ok, _) = apply_pipewire_quantum("bad", true);
        assert!(!ok);
    }
}
