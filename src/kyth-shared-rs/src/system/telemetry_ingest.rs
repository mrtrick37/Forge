//! Pure MangoHud telemetry parsing for the future Rust collector.
//!
//! This module intentionally does not touch files, SQLite, MangoHud config,
//! or `/proc`. The current `kyth-telem` writer stays Python-owned while this
//! parser is validated and adopted by a Rust service in a later slice.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MangoHudCsv {
    pub metadata: HashMap<String, String>,
    pub rows: Vec<HashMap<String, String>>,
}

/// Parse one CSV record, including quoted commas and escaped quotes.
fn csv_record(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(field.trim().to_string());
                field.clear();
            }
            _ => field.push(ch),
        }
    }
    fields.push(field.trim().to_string());
    fields
}

/// Parse MangoHud's metadata rows and data rows without opening the file.
/// This mirrors `kyth-telem`'s four-line minimum and header discovery rules.
pub fn parse_mangohud_csv(text: &str) -> Option<MangoHudCsv> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < 4 {
        return None;
    }

    let keys = csv_record(lines[0]);
    let values = csv_record(lines[1]);
    let metadata = keys
        .into_iter()
        .zip(values)
        .filter(|(key, _)| !key.is_empty())
        .collect();

    let data_start =
        (2..lines.len().min(6)).find(|index| lines[*index].to_ascii_lowercase().contains("fps"))?;
    let headers = csv_record(lines[data_start]);
    if headers.is_empty() {
        return None;
    }

    let rows = lines[data_start + 1..]
        .iter()
        .map(|line| {
            csv_record(line)
                .into_iter()
                .enumerate()
                .filter_map(|(index, value)| headers.get(index).map(|key| (key.trim(), value)))
                .filter(|(key, _)| !key.is_empty())
                .map(|(key, value)| (key.to_string(), value))
                .collect::<HashMap<_, _>>()
        })
        .collect();

    Some(MangoHudCsv { metadata, rows })
}

pub fn safe_float(value: Option<&str>) -> f64 {
    value
        .and_then(|raw| raw.parse::<f64>().ok())
        .filter(|parsed| parsed.is_finite())
        .unwrap_or(0.0)
}

fn has_timestamp_suffix(stem: &str) -> Option<usize> {
    // _YYYY-MM-DD_HH:MM:SS and the hyphenated-time variant accepted by the
    // Python collector. Keep this structural rather than locale-dependent.
    let bytes = stem.as_bytes();
    if bytes.len() < 20 || bytes[bytes.len() - 20] != b'_' {
        return None;
    }
    let suffix = &bytes[bytes.len() - 19..];
    let separators = [b'-', b'-', b'_', b':', b':'];
    let separator_positions = [4, 7, 10, 13, 16];
    for (position, separator) in separator_positions.into_iter().zip(separators) {
        if suffix[position] != separator
            && !(position >= 13 && separator == b':' && suffix[position] == b'-')
        {
            return None;
        }
    }
    if [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18]
        .into_iter()
        .any(|position| !suffix[position].is_ascii_digit())
    {
        return None;
    }
    Some(bytes.len() - 20)
}

pub fn derive_game_name(stem: &str) -> (String, String) {
    let executable = has_timestamp_suffix(stem)
        .map(|index| &stem[..index])
        .unwrap_or(stem);
    let game_name = executable
        .replace(['-', '_'], " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    (game_name, executable.to_string())
}

pub fn detect_launcher(executable: &str, driver: Option<&str>) -> &'static str {
    let executable = executable.to_ascii_lowercase();
    let driver = driver.unwrap_or_default().to_ascii_lowercase();
    if executable.contains("steam") || driver.contains("proton") {
        "Steam"
    } else if executable.contains("heroic") {
        "Heroic"
    } else if executable.contains("lutris") {
        "Lutris"
    } else if executable.contains("wine") {
        "Wine"
    } else {
        "Native"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_metadata_quoted_fields_and_rows() {
        let csv = "gpu,driver\nAMD,\"Mesa, Vulkan\"\ntime,fps,frametime\n0,60,16.6\n1,58,17.2\n";
        let parsed = parse_mangohud_csv(csv).unwrap();
        assert_eq!(
            parsed.metadata.get("driver"),
            Some(&"Mesa, Vulkan".to_string())
        );
        assert_eq!(parsed.rows[1].get("fps"), Some(&"58".to_string()));
    }

    #[test]
    fn rejects_short_or_missing_fps_csv() {
        assert!(parse_mangohud_csv("a\nb\nc\n").is_none());
        assert!(parse_mangohud_csv("a\nb\ncpu\n1\n").is_none());
    }

    #[test]
    fn derives_game_name_like_python_collector() {
        assert_eq!(
            derive_game_name("my-game_2025-01-15_14:22:01"),
            ("My Game".into(), "my-game".into())
        );
        assert_eq!(
            derive_game_name("my_game_2025-01-15_14-22-01"),
            ("My Game".into(), "my_game".into())
        );
        assert_eq!(
            derive_game_name("native-game"),
            ("Native Game".into(), "native-game".into())
        );
    }

    #[test]
    fn launcher_detection_prefers_steam_and_proton() {
        assert_eq!(detect_launcher("game", Some("Proton")), "Steam");
        assert_eq!(detect_launcher("heroic-game", None), "Heroic");
        assert_eq!(detect_launcher("game", None), "Native");
    }

    #[test]
    fn safe_float_is_zero_for_missing_or_invalid_values() {
        assert_eq!(safe_float(Some("60.5")), 60.5);
        assert_eq!(safe_float(Some("nan")), 0.0);
        assert_eq!(safe_float(None), 0.0);
    }
}
