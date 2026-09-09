//! Port of `kyth_shared.system.bootc_query` — bootc status queries.

use regex::Regex;
use serde_json::Value;
use std::time::Duration;

pub fn nested_get<'a>(data: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cur = data;
    for k in path {
        cur = cur.get(*k)?;
    }
    Some(cur)
}

pub fn walk_strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Object(m) => {
            for val in m.values() {
                walk_strings(val, out);
            }
        }
        Value::Array(arr) => {
            for val in arr {
                walk_strings(val, out);
            }
        }
        _ => {}
    }
}

fn run_with_timeout(cmd: &[String], timeout: Duration) -> Option<(i32, String)> {
    if cmd.is_empty() {
        return None;
    }
    let output = super::process::run_bounded(cmd, timeout).ok()?;
    Some((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
    ))
}

fn status_commands(json_mode: bool) -> Vec<Vec<String>> {
    let guard_op = if json_mode { "status-json" } else { "status" };
    let guard = vec![
        "/usr/bin/kyth-bootc-guard".to_string(),
        guard_op.to_string(),
    ];
    let bootc = if json_mode {
        vec![
            "bootc".to_string(),
            "status".to_string(),
            "--json".to_string(),
        ]
    } else {
        vec!["bootc".to_string(), "status".to_string()]
    };
    if effective_uid() == 0 {
        vec![guard, bootc]
    } else {
        vec![
            vec![
                "sudo".to_string(),
                "-n".to_string(),
                guard[0].clone(),
                guard[1].clone(),
            ],
            bootc,
        ]
    }
}

fn effective_uid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status.lines().find_map(|line| {
                let value = line.strip_prefix("Uid:")?.split_whitespace().next()?;
                value.parse().ok()
            })
        })
        .unwrap_or(1)
}

pub fn holds_sysroot_lock(cmdline: &str) -> bool {
    let text = cmdline.trim();
    if text.contains("ostree admin finalize-staged") {
        return true;
    }
    Regex::new(r"(?:^|[\s/])bootc\s+(upgrade|switch|rollback|reset)(?:\s|$)")
        .map(|pattern| pattern.is_match(text))
        .unwrap_or(false)
}

pub fn active_operation() -> Option<String> {
    let output = super::process::run_bounded(
        &["ps", "-eo", "pid=,args="]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>(),
        Duration::from_secs(3),
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| holds_sysroot_lock(line))
        .map(str::to_string)
}

pub fn fetch_status_text() -> String {
    if active_operation().is_some() {
        return String::new();
    }
    for cmd in status_commands(false) {
        if let Some((0, stdout)) = run_with_timeout(&cmd, Duration::from_secs(10)) {
            let t = stdout.trim().to_string();
            if !t.is_empty() {
                return t;
            }
        }
    }
    String::new()
}

pub fn fetch_status_data() -> Option<Value> {
    if active_operation().is_some() {
        return None;
    }
    for cmd in status_commands(true) {
        if let Some((0, stdout)) = run_with_timeout(&cmd, Duration::from_secs(10)) {
            if let Some(value) = parse_status_data(&stdout) {
                return Some(value);
            }
        }
    }
    None
}

/// Decode the structured status response and reject valid JSON that is not an
/// object, matching the Python `parse_json_object` contract.
pub fn parse_status_data(raw: &str) -> Option<Value> {
    let value = serde_json::from_str::<Value>(raw).ok()?;
    value.is_object().then_some(value)
}

pub fn image_reference_from_status(data: &Value) -> Option<String> {
    image_reference_from_status_with_output(data, "")
}

/// Resolve an image reference from structured bootc data, then from the
/// human-readable status output used by older bootc versions.
pub fn image_reference_from_status_with_output(
    data: &Value,
    status_output: &str,
) -> Option<String> {
    // Try status.booted.image.reference etc.
    for path in [
        vec!["status", "booted", "image", "reference"],
        vec!["status", "booted", "image", "image", "reference"],
        vec!["status", "booted", "image", "image", "image"],
        vec!["status", "booted", "image", "image"],
        vec!["status", "booted", "image"],
        vec!["spec", "image", "image"],
        vec!["spec", "image", "reference"],
    ] {
        if let Some(v) = nested_get(data, &path.iter().map(|s| *s).collect::<Vec<_>>()) {
            if let Some(s) = v.as_str() {
                if !s.trim().is_empty() {
                    return Some(s.trim().to_string());
                }
            }
            if let Some(obj) = v.as_object() {
                if let Some(s) = obj.get("reference").and_then(|x| x.as_str()) {
                    if !s.trim().is_empty() {
                        return Some(s.trim().to_string());
                    }
                }
                if let Some(s) = obj.get("image").and_then(|x| x.as_str()) {
                    if !s.trim().is_empty() {
                        return Some(s.trim().to_string());
                    }
                }
            }
        }
    }
    // walk strings for ghcr.io
    let mut strs = Vec::new();
    walk_strings(data, &mut strs);
    for s in strs {
        if s.to_lowercase().contains("ghcr.io/kyth-os/kyth") {
            return Some(s.trim().to_string());
        }
    }
    if !status_output.is_empty() {
        let pattern =
            Regex::new(r"(ghcr\.io/kyth-os/kyth(?::[A-Za-z0-9._-]+)?(?:@sha256:[a-fA-F0-9]+)?)")
                .ok()?;
        if let Some(reference) = pattern
            .captures(status_output)
            .and_then(|captures| captures.get(1))
        {
            return Some(reference.as_str().to_string());
        }
    }
    None
}

/// Read the current image reference through the same bounded fallbacks as
/// the Python compatibility layer. This is observation only: no update or
/// deployment command is issued.
pub fn image_reference() -> Option<String> {
    let data = crate::system::probe::read_section("bootc-status-data")
        .or_else(fetch_status_data)
        .unwrap_or(Value::Object(serde_json::Map::new()));
    if let Some(reference) = image_reference_from_status_with_output(&data, "") {
        return Some(reference);
    }
    let status_output = fetch_status_text();
    if let Some(reference) = image_reference_from_status_with_output(&data, &status_output) {
        return Some(reference);
    }
    let command = vec!["rpm-ostree".to_string(), "status".to_string()];
    run_with_timeout(&command, Duration::from_secs(10))
        .filter(|(code, _)| *code == 0)
        .and_then(|(_, output)| {
            image_reference_from_status_with_output(&Value::Object(serde_json::Map::new()), &output)
        })
}

pub fn image_digest_from_status(data: &Value, section: &str) -> Option<String> {
    for path in [
        vec!["status", section, "image", "imageDigest"],
        vec!["status", section, "image", "digest"],
        vec!["status", section, "imageDigest"],
        vec!["status", section, "digest"],
    ] {
        if let Some(v) = nested_get(data, &path.iter().map(|s| *s).collect::<Vec<_>>()) {
            if let Some(s) = v.as_str() {
                if s.starts_with("sha256:") {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

pub fn image_digest(data: &Value, section: &str) -> Option<(String, String)> {
    let full = image_digest_from_status(data, section)?;
    Some((full[7..].chars().take(12).collect(), full[7..].to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn nested() {
        let v = json!({"a":{"b":2}});
        assert_eq!(nested_get(&v, &["a", "b"]).unwrap().as_i64(), Some(2));
        assert!(nested_get(&v, &["a", "c"]).is_none());
    }
    #[test]
    fn image_ref() {
        let v = json!({"status":{"booted":{"image":{"reference":"ghcr.io/kyth-os/kyth:latest"}}}});
        assert_eq!(
            image_reference_from_status(&v),
            Some("ghcr.io/kyth-os/kyth:latest".to_string())
        );
    }

    #[test]
    fn image_ref_falls_back_to_human_readable_status_output() {
        let output = "Image: ghcr.io/kyth-os/kyth:testing@sha256:abcdef1234\n";
        assert_eq!(
            image_reference_from_status_with_output(&Value::Null, output),
            Some("ghcr.io/kyth-os/kyth:testing@sha256:abcdef1234".into())
        );
        assert!(image_reference_from_status_with_output(
            &Value::Null,
            "Image: quay.io/example/other:latest"
        )
        .is_none());
    }

    #[test]
    fn status_json_fallback_rejects_non_object_documents() {
        assert!(parse_status_data("[]").is_none());
        assert!(parse_status_data("{bad").is_none());
        assert_eq!(
            parse_status_data(r#"{"status":{}}"#).unwrap()["status"],
            json!({})
        );
    }

    #[test]
    fn lock_detection_matches_bootc_operations() {
        assert!(holds_sysroot_lock("123 /usr/bin/bootc upgrade"));
        assert!(holds_sysroot_lock("/usr/bin/ostree admin finalize-staged"));
        assert!(!holds_sysroot_lock("/usr/bin/bootc status --json"));
    }

    #[test]
    fn digest_shortens_without_the_algorithm_prefix() {
        let v = serde_json::json!({"status":{"staged":{"image":{"imageDigest":"sha256:1234567890abcdef"}}}});
        assert_eq!(
            image_digest(&v, "staged"),
            Some(("1234567890ab".into(), "1234567890abcdef".into()))
        );
    }
}
