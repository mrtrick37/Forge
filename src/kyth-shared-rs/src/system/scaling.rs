//! Offline fractional display-scaling configuration.
//!
//! This ports the data and projection half of `kyth_shared.scaling`.
//! KScreen discovery, ICC deployment, and display mutation stay outside the
//! shared crate because they are guarded desktop actions.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct ScalingOutput {
    pub scale: f64,
    pub icc: String,
}

pub type ScalingConfig = BTreeMap<String, ScalingOutput>;

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    base.join("kyth/scaling.toml")
}

fn parse_scale(value: Option<&toml::Value>) -> f64 {
    value
        .and_then(|value| value.as_float().or_else(|| value.as_integer().map(|value| value as f64)))
        .unwrap_or(1.0)
        .clamp(1.0, 3.0)
}

pub fn load(path: impl AsRef<Path>) -> ScalingConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return ScalingConfig::new();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return ScalingConfig::new();
    };
    value
        .get("outputs")
        .and_then(toml::Value::as_table)
        .map(|outputs| {
            outputs
                .iter()
                .filter_map(|(name, value)| {
                    let table = value.as_table()?;
                    Some((
                        name.clone(),
                        ScalingOutput {
                            scale: parse_scale(table.get("scale")),
                            icc: table.get("icc").and_then(toml::Value::as_str).unwrap_or_default().to_string(),
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn save(path: impl AsRef<Path>, outputs: &ScalingConfig) -> std::io::Result<()> {
    let quote = |value: &str| toml::Value::String(value.to_string()).to_string();
    let mut lines = vec!["# Kyth scaling per-output".to_string(), String::new()];
    for (name, output) in outputs {
        lines.push(format!("[outputs.{}]", quote(name)));
        lines.push(format!("scale = {}", output.scale));
        if !output.icc.is_empty() {
            lines.push(format!("icc = {}", quote(&output.icc)));
        }
        lines.push(String::new());
    }
    crate::atomic_io::atomic_write_text(path, &format!("{}\n", lines.join("\n")), Some(0o600))
}

pub const ICC_DEST_DIR: &str = "/usr/share/color/icc/kyth";
pub const TTL_PATH: &str = "/run/kyth-scaling-ttl";
pub const TTL_SECS: u64 = 30;

/// Mirrors Python `f"{scale:.2f}".rstrip("0").rstrip(".")` (`1.25`, `2`).
pub fn scale_arg(scale: f64) -> String {
    let text = format!("{scale:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Mirrors the output-name gate shared with the HDR launcher.
pub fn is_output_name_valid(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

pub fn scale_argv(conn: &str, scale: &str) -> Vec<String> {
    vec!["kscreen-doctor".to_string(), format!("output.{conn}.scale.{scale}")]
}

/// Mirrors `os.access(dir, os.W_OK)`: root can write anywhere; otherwise a
/// write bit must be set.
pub fn dir_writable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else { return false };
    if !metadata.is_dir() {
        return false;
    }
    if unsafe { libc::geteuid() } == 0 {
        return true;
    }
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o222 != 0
}

/// Outcome of the per-output ICC step, mirroring the Python note branches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IccOutcome {
    Deployed(String),
    NotDeployed(String),
    Failed(String),
    Skipped,
}

pub fn icc_outcome(conn: &str, icc: &str, dest_dir: &Path) -> IccOutcome {
    let icc = icc.trim();
    if icc.is_empty() || !Path::new(icc).is_file() {
        return IccOutcome::Skipped;
    }
    if !dir_writable(dest_dir) {
        return IccOutcome::NotDeployed(format!("{conn}.icc={icc} (not deployed — needs root icc dir)"));
    }
    let file_name = Path::new(icc).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let dest = dest_dir.join(file_name);
    match std::fs::read(icc).and_then(|bytes| std::fs::write(&dest, bytes).map(|_| dest)) {
        Ok(dest) => IccOutcome::Deployed(format!("{conn}.icc={}", dest.display())),
        Err(error) => IccOutcome::Failed(format!("{conn}.icc failed: {error}")),
    }
}

pub fn kwin_config(outputs: &ScalingConfig) -> Value {
    json!({
        "outputs": outputs.iter().map(|(name, output)| json!({
            "name": name,
            "scale": output.scale,
            "icc": output.icc,
        })).collect::<Vec<_>>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_clamped_scaling_and_projects_kwin_data() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("scaling.toml");
        std::fs::write(&path, "[outputs.\"DP-1\"]\nscale = 4\nicc = \"/tmp/display.icc\"\n").unwrap();
        let config = load(&path);
        assert_eq!(config["DP-1"].scale, 3.0);
        assert_eq!(kwin_config(&config)["outputs"][0]["name"], "DP-1");
    }

    #[test]
    fn formats_scale_args_like_python() {
        assert_eq!(scale_arg(1.25), "1.25");
        assert_eq!(scale_arg(2.0), "2");
        assert_eq!(scale_arg(1.5), "1.5");
        assert_eq!(scale_argv("DP-1", "1.25"), vec!["kscreen-doctor", "output.DP-1.scale.1.25"]);
        assert!(is_output_name_valid("DP-1"));
        assert!(!is_output_name_valid("DP 1"));
    }

    #[test]
    fn icc_notes_cover_all_branches() {
        let dir = tempdir().unwrap();
        let profile = dir.path().join("display.icc");
        std::fs::write(&profile, b"icc-bytes").unwrap();
        let profile = profile.to_string_lossy().into_owned();
        assert_eq!(icc_outcome("DP-1", "  ", Path::new("/nonexistent")), IccOutcome::Skipped);
        assert_eq!(
            icc_outcome("DP-1", &profile, Path::new("/nonexistent-icc-dir")),
            IccOutcome::NotDeployed(format!("DP-1.icc={profile} (not deployed — needs root icc dir)"))
        );
        let dest_dir = dir.path().join("icc");
        std::fs::create_dir(&dest_dir).unwrap();
        match icc_outcome("DP-1", &profile, &dest_dir) {
            IccOutcome::Deployed(note) => assert!(note.starts_with("DP-1.icc=")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn saves_sorted_outputs_and_round_trips() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("nested/scaling.toml");
        let config = ScalingConfig::from([
            ("HDMI-1".into(), ScalingOutput { scale: 1.25, icc: String::new() }),
            ("DP-1".into(), ScalingOutput { scale: 2.0, icc: "/tmp/a.icc".into() }),
        ]);
        save(&path, &config).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded["DP-1"].icc, "/tmp/a.icc");
        assert_eq!(loaded["HDMI-1"].scale, 1.25);
    }
}
