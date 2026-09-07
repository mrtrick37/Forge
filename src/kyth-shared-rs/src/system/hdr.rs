//! Per-display HDR configuration and EDID luminance parsing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdrDisplay {
    pub peak_nits: i64,
    pub hdr_enabled: bool,
    pub sdr_nits: i64,
}

impl Default for HdrDisplay {
    fn default() -> Self { Self { peak_nits: 400, hdr_enabled: false, sdr_nits: 200 } }
}

fn clamp(display: HdrDisplay) -> HdrDisplay {
    HdrDisplay { peak_nits: display.peak_nits.clamp(100, 4_000), hdr_enabled: display.hdr_enabled, sdr_nits: display.sdr_nits.clamp(80, 600) }
}

pub fn load(path: impl AsRef<Path>) -> BTreeMap<String, HdrDisplay> {
    let Ok(raw) = std::fs::read_to_string(path) else { return BTreeMap::new(); };
    let Ok(value) = raw.parse::<toml::Value>() else { return BTreeMap::new(); };
    value.get("displays").and_then(toml::Value::as_table).map(|displays| displays.iter().filter_map(|(name, value)| {
        let entry = value.as_table()?;
        Some((name.clone(), clamp(HdrDisplay {
            peak_nits: entry.get("peak_nits").and_then(toml::Value::as_integer).unwrap_or(400),
            hdr_enabled: entry.get("hdr_enabled").and_then(toml::Value::as_bool).unwrap_or(false),
            sdr_nits: entry.get("sdr_nits").and_then(toml::Value::as_integer).unwrap_or(200),
        })))
    }).collect()).unwrap_or_default()
}

pub fn save(path: impl AsRef<Path>, displays: &BTreeMap<String, HdrDisplay>) -> std::io::Result<()> {
    let mut text = String::from("# Kyth per-display HDR mastering — EDID + KWin\n");
    for (name, display) in displays {
        let display = clamp(display.clone());
        text.push_str(&format!("[displays.{name:?}]\npeak_nits = {}\nhdr_enabled = {}\nsdr_nits = {}\n\n", display.peak_nits, display.hdr_enabled, display.sdr_nits));
    }
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

pub fn parse_edid_peak_nits(data: &[u8]) -> Option<i64> {
    if data.len() < 128 { return None; }
    let extension_count = data[126];
    if extension_count == 0 || data.len() < 256 { return None; }
    data.iter().enumerate().skip(128).take(384).find_map(|(index, byte)| {
        let value = *byte;
        let maybe_peak = *data.get(index + 2)?;
        (value == 0x06 && (1..=10).contains(&maybe_peak)).then_some(i64::from(maybe_peak) * 100)
    })
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("kyth/display-hdr.toml");
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".config/kyth/display-hdr.toml")
}

/// Mirrors `_OUTPUT_NAME_RE` (`^[A-Za-z0-9._-]+$`): only validated names may
/// reach a `kscreen-doctor` argv.
pub fn is_output_name_valid(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

fn force_default(enabled: bool) -> HdrDisplay {
    HdrDisplay { peak_nits: 400, hdr_enabled: enabled, sdr_nits: 200 }
}

/// Mirrors `apply_display_hdr`'s target selection: configured entries for
/// connected outputs win; an empty config with `force_enable` (or an empty
/// selection under `force_enable`) falls back to force defaults for every
/// connected output; `force_enable` overrides `hdr_enabled` on selection.
pub fn select_targets(
    connected: &[String],
    displays: &BTreeMap<String, HdrDisplay>,
    force_enable: Option<bool>,
) -> BTreeMap<String, HdrDisplay> {
    let mut targets = BTreeMap::new();
    if displays.is_empty() {
        if let Some(enabled) = force_enable {
            for name in connected {
                targets.insert(name.clone(), force_default(enabled));
            }
        }
    } else {
        for name in connected {
            if let Some(display) = displays.get(name) {
                targets.insert(name.clone(), display.clone());
            }
        }
    }
    if let Some(enabled) = force_enable {
        for display in targets.values_mut() {
            display.hdr_enabled = enabled;
        }
        if targets.is_empty() {
            for name in connected {
                targets.insert(name.clone(), force_default(enabled));
            }
        }
    }
    targets
}

/// Mirrors the per-output `kscreen-doctor` argv (`sdr-brightness` only when
/// enabling, exactly as the Python launcher orders it).
pub fn kscreen_apply_argv(name: &str, display: &HdrDisplay) -> Vec<String> {
    let action = if display.hdr_enabled { "enable" } else { "disable" };
    let mut argv = vec![
        "kscreen-doctor".to_string(),
        format!("output.{name}.hdr.{action}"),
        format!("output.{name}.wcg.{action}"),
    ];
    if display.hdr_enabled {
        argv.push(format!("output.{name}.sdr-brightness.{}", display.sdr_nits));
    }
    argv
}

/// Mirrors the per-output note strings (`{name}.hdr.{action}[,sdr=N]`, with
/// a ` failed` suffix when kscreen-doctor exits non-zero).
pub fn apply_note(name: &str, display: &HdrDisplay, success: bool) -> String {
    let action = if display.hdr_enabled { "enable" } else { "disable" };
    let mut note = format!("{name}.hdr.{action}");
    if success && display.hdr_enabled {
        note.push_str(&format!(",sdr={}", display.sdr_nits));
    }
    if !success {
        note.push_str(" failed");
    }
    note
}

pub fn env_hints(display: &HdrDisplay) -> BTreeMap<String, String> {
    let display = clamp(display.clone());
    if !display.hdr_enabled { return BTreeMap::new(); }
    BTreeMap::from([
        ("KYTH_HDR".into(), "1".into()),
        ("KYTH_HDR_PEAK_NITS".into(), display.peak_nits.to_string()),
        ("KYTH_HDR_SDR_NITS".into(), display.sdr_nits.to_string()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clamps_and_round_trips_display_config() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("display-hdr.toml");
        let mut displays = BTreeMap::new();
        displays.insert("HDMI-1".into(), HdrDisplay { peak_nits: 10, hdr_enabled: true, sdr_nits: 999 });
        save(&path, &displays).unwrap();
        assert_eq!(load(&path)["HDMI-1"], HdrDisplay { peak_nits: 100, hdr_enabled: true, sdr_nits: 600 });
        assert_eq!(env_hints(&load(&path)["HDMI-1"]).get("KYTH_HDR"), Some(&"1".into()));
    }

    #[test]
    fn validates_output_names_like_python_regex() {
        assert!(is_output_name_valid("HDMI-1"));
        assert!(is_output_name_valid("DP_1.2-a"));
        assert!(!is_output_name_valid(""));
        assert!(!is_output_name_valid("HDMI 1"));
        assert!(!is_output_name_valid("DP-1;rm"));
    }

    #[test]
    fn selects_targets_like_python_apply() {
        let connected = vec!["HDMI-1".to_string(), "DP-1".to_string()];
        let mut configured = BTreeMap::new();
        configured.insert("HDMI-1".into(), HdrDisplay { peak_nits: 800, hdr_enabled: true, sdr_nits: 250 });
        // Configured connected outputs win; unlisted outputs are skipped.
        let targets = select_targets(&connected, &configured, None);
        assert_eq!(targets.len(), 1);
        assert!(targets["HDMI-1"].hdr_enabled);
        // Force overrides hdr_enabled on the selection.
        let forced = select_targets(&connected, &configured, Some(false));
        assert_eq!(forced.len(), 1);
        assert!(!forced["HDMI-1"].hdr_enabled);
        // Empty config without force: nothing to apply.
        assert!(select_targets(&connected, &BTreeMap::new(), None).is_empty());
        // Empty config with force: defaults for every connected output.
        let fallback = select_targets(&connected, &BTreeMap::new(), Some(true));
        assert_eq!(fallback.len(), 2);
        assert!(fallback.values().all(|display| display.hdr_enabled && display.sdr_nits == 200));
        // Non-empty config missing every connected output + force: all connected.
        let mut other = BTreeMap::new();
        other.insert("eDP-1".into(), HdrDisplay::default());
        let refilled = select_targets(&connected, &other, Some(true));
        assert_eq!(refilled.len(), 2);
    }

    #[test]
    fn projects_kscreen_argv_and_notes() {
        let enabled = HdrDisplay { peak_nits: 800, hdr_enabled: true, sdr_nits: 250 };
        assert_eq!(kscreen_apply_argv("HDMI-1", &enabled), vec![
            "kscreen-doctor", "output.HDMI-1.hdr.enable", "output.HDMI-1.wcg.enable", "output.HDMI-1.sdr-brightness.250",
        ]);
        assert_eq!(apply_note("HDMI-1", &enabled, true), "HDMI-1.hdr.enable,sdr=250");
        assert_eq!(apply_note("HDMI-1", &enabled, false), "HDMI-1.hdr.enable failed");
        let disabled = HdrDisplay::default();
        assert_eq!(kscreen_apply_argv("DP-1", &disabled), vec![
            "kscreen-doctor", "output.DP-1.hdr.disable", "output.DP-1.wcg.disable",
        ]);
        assert_eq!(apply_note("DP-1", &disabled, true), "DP-1.hdr.disable");
    }

    #[test]
    fn finds_extension_block_peak_hint() {
        let mut edid = vec![0_u8; 256];
        edid[126] = 1;
        edid[128] = 0x06;
        edid[130] = 4;
        assert_eq!(parse_edid_peak_nits(&edid), Some(400));
        assert_eq!(parse_edid_peak_nits(&edid[..127]), None);
    }
}
