//! Plasma drift reconciler: declarative `plasma.toml` via `kwriteconfig`.
//!
//! Mirrors `kyth_shared.plasma_drift`: dotted section names map to config
//! files, nested tables to `--group` paths, bare sections default to
//! `General`. One deliberate deviation: section/key iteration is sorted
//! (`BTreeMap`), not TOML-declared order — writes are independent
//! single-key invocations, so the applied state is identical. Only the
//! `*_bin.rs` entry point executes processes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const TTL_PATH: &str = "/run/kyth-plasma-ttl";
pub const TTL_SECS: u64 = 30;

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("kyth/plasma.toml");
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".config/kyth/plasma.toml")
}

/// Mirrors Python `str()` for TOML scalars (`True`/`False`, integral floats
/// keeping one decimal).
pub fn toml_scalar(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => {
            if f.fract() == 0.0 && f.is_finite() { format!("{f:.1}") } else { format!("{f}") }
        }
        toml::Value::Boolean(b) => {
            if *b { "True".to_string() } else { "False".to_string() }
        }
        toml::Value::Array(items) => {
            format!("[{}]", items.iter().map(toml_repr).collect::<Vec<_>>().join(", "))
        }
        other => other.to_string(),
    }
}

fn toml_repr(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => format!("'{s}'"),
        _ => toml_scalar(value),
    }
}

fn flatten_into(table: &toml::Table, prefix: &str, out: &mut BTreeMap<String, BTreeMap<String, String>>) {
    let mut scalars = BTreeMap::new();
    for (key, value) in table {
        let name = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
        if let Some(nested) = value.as_table() {
            let mut nested_scalars = BTreeMap::new();
            for (child_key, child_value) in nested {
                if let Some(deeper) = child_value.as_table() {
                    // Single-key recursion, exactly like the Python version.
                    let mut single = toml::Table::new();
                    single.insert(child_key.clone(), toml::Value::Table(deeper.clone()));
                    flatten_into(&single, &name, out);
                } else {
                    nested_scalars.insert(child_key.clone(), toml_scalar(child_value));
                }
            }
            if !nested_scalars.is_empty() {
                out.insert(name, nested_scalars);
            }
        } else {
            scalars.insert(key.clone(), toml_scalar(value));
        }
    }
    // Top-level scalars are invalid for kwriteconfig; ignored, as in Python.
    if !scalars.is_empty() && !prefix.is_empty() {
        out.insert(prefix.to_string(), scalars);
    }
}

/// Flattens nested tables to dotted section keys, mirroring
/// `_flatten_sections` (top-level scalars ignored).
pub fn flatten_sections(value: &toml::Value) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    if let Some(table) = value.as_table() {
        flatten_into(table, "", &mut out);
    }
    out
}

pub fn load(path: impl AsRef<Path>) -> BTreeMap<String, BTreeMap<String, String>> {
    let Ok(raw) = std::fs::read_to_string(path) else { return BTreeMap::new(); };
    let Ok(value) = raw.parse::<toml::Value>() else { return BTreeMap::new(); };
    flatten_sections(&value)
}

/// Splits `kwinrc.Compositing` → (`kwinrc`, [`Compositing`]); bare names
/// default to `General`. Empty sections are invalid (`None`).
pub fn parse_section(section: &str) -> Option<(String, Vec<String>)> {
    let parts: Vec<&str> = section.split('.').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }
    if parts.len() == 1 {
        return Some((parts[0].to_string(), vec!["General".to_string()]));
    }
    Some((parts[0].to_string(), parts[1..].iter().map(|s| s.to_string()).collect()))
}

pub fn kwriteconfig_argv(binary: &str, file: &str, groups: &[String], key: &str, value: &str) -> Vec<String> {
    let mut argv = vec![binary.to_string(), "--file".to_string(), file.to_string()];
    for group in groups {
        argv.extend(["--group".to_string(), group.clone()]);
    }
    argv.extend(["--key".to_string(), key.to_string(), value.to_string()]);
    argv
}

/// Applies sections via `runner`; returns applied `section:key=value` notes.
/// `runner` mirrors `run(args, capture_output=True, timeout=5)` success.
/// Invalid sections are skipped; the KWin reconfigure + TTL steps live in
/// the binary (they need process execution).
pub fn apply_sections(
    sections: &BTreeMap<String, BTreeMap<String, String>>,
    binary: &str,
    runner: &dyn Fn(&[String]) -> bool,
) -> Vec<String> {
    let mut applied = Vec::new();
    for (section, entries) in sections {
        let Some((file, groups)) = parse_section(section) else { continue };
        for (key, value) in entries {
            if runner(&kwriteconfig_argv(binary, &file, &groups, key, value)) {
                applied.push(format!("{section}:{key}={value}"));
            }
        }
    }
    applied
}

pub fn reconfigure_argv(qdbus: &str) -> Vec<String> {
    vec![qdbus.to_string(), "org.kde.KWin".to_string(), "/KWin".to_string(), "reconfigure".to_string()]
}

pub fn kwriteconfig_candidates() -> [&'static str; 3] {
    ["kwriteconfig6", "kwriteconfig5", "kwriteconfig"]
}

pub fn qdbus_candidates() -> [&'static str; 3] {
    ["qdbus6", "qdbus-qt6", "qdbus"]
}

pub fn run_timeout() -> Duration {
    Duration::from_secs(5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattens_nested_tables_like_python() {
        let value: toml::Value = "[kwinrc]\nkey = \"value\"\n[kwinrc.Compositing]\nAllowTearing = \"false\"\n[kwinrc.Containments.1.General]\nfoo = \"bar\"\n".parse().unwrap();
        let sections = flatten_sections(&value);
        assert_eq!(sections["kwinrc"].get("key"), Some(&"value".to_string()));
        assert_eq!(sections["kwinrc.Compositing"].get("AllowTearing"), Some(&"false".to_string()));
        assert_eq!(sections["kwinrc.Containments.1.General"].get("foo"), Some(&"bar".to_string()));
    }

    #[test]
    fn ignores_top_level_scalars() {
        let value: toml::Value = "stray = 1\n[kwinrc]\nkey = \"value\"\n".parse().unwrap();
        let sections = flatten_sections(&value);
        assert_eq!(sections.len(), 1);
        assert!(sections.contains_key("kwinrc"));
    }

    #[test]
    fn parses_sections_with_general_default() {
        assert_eq!(parse_section("kwinrc"), Some(("kwinrc".to_string(), vec!["General".to_string()])));
        assert_eq!(
            parse_section("kwinrc.Containments.1.General"),
            Some(("kwinrc".to_string(), vec!["Containments".to_string(), "1".to_string(), "General".to_string()]))
        );
        assert_eq!(parse_section("..."), None);
    }

    #[test]
    fn renders_python_scalar_spellings() {
        assert_eq!(toml_scalar(&toml::Value::Boolean(true)), "True");
        assert_eq!(toml_scalar(&toml::Value::Integer(4)), "4");
        assert_eq!(toml_scalar(&toml::Value::Float(4.0)), "4.0");
    }

    #[test]
    fn applies_sections_and_skips_invalid_ones() {
        let mut sections = BTreeMap::new();
        sections.insert("kwinrc.Compositing".to_string(), BTreeMap::from([("AllowTearing".to_string(), "false".to_string())]));
        sections.insert("...".to_string(), BTreeMap::from([("x".to_string(), "y".to_string())]));
        let applied = apply_sections(&sections, "kwriteconfig6", &|_| true);
        assert_eq!(applied, vec!["kwinrc.Compositing:AllowTearing=false".to_string()]);
        let applied = apply_sections(&sections, "kwriteconfig6", &|_| false);
        assert!(applied.is_empty());
    }
}
