//! GitHub issue drafts: prefilled issue URL plus a local Markdown draft.
//!
//! Mirrors `kyth_shared.diagnostics.create_github_issue_draft`: readable
//! body files, placeholder bodies, truncation for the browser URL, and
//! `quote_plus` URL encoding. Timestamps use the local timezone via libc,
//! matching `datetime.now().strftime`. Only the `*_bin.rs` entry point
//! touches the filesystem and browser.

use std::path::{Path, PathBuf};

pub const MAX_BODY: usize = 5500;
pub const TRUNCATION_NOTE: &str =
    "\n\n[Report body truncated for the browser URL. A full local draft was saved by kyth-report-issue.]";
pub const BODY_PLACEHOLDER: &str =
    "Describe what happened, what you expected, and what you were doing just before it happened.";
pub const DEFAULT_REPO_URL: &str = "https://github.com/kyth-os/kyth";

pub fn draft_dir() -> PathBuf {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(state).join("kyth");
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/root"));
    home.join(".local/state/kyth")
}

/// Local timestamp (`%Y%m%d-%H%M%S`), mirroring `datetime.now()`.
pub fn local_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as libc::time_t)
        .unwrap_or(0);
    let mut broken = unsafe { std::mem::zeroed::<libc::tm>() };
    unsafe { libc::localtime_r(&now, &mut broken) };
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        broken.tm_year + 1900,
        broken.tm_mon + 1,
        broken.tm_mday,
        broken.tm_hour,
        broken.tm_min,
        broken.tm_sec
    )
}

pub fn draft_path(dir: &Path, timestamp: &str) -> PathBuf {
    dir.join(format!("github-issue-{timestamp}.md"))
}

pub fn render_draft(title: &str, body: &str) -> String {
    format!("# {title}\n\n{body}\n")
}

pub fn url_body(body: &str) -> String {
    if body.chars().count() > MAX_BODY {
        let truncated: String = body.chars().take(MAX_BODY).collect();
        format!("{truncated}{TRUNCATION_NOTE}")
    } else {
        body.to_string()
    }
}

/// Mirrors `urllib.parse.quote_plus` (space → `+`, uppercase `%XX`).
pub fn quote_plus(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => encoded.push(byte as char),
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

pub fn issue_url(repo_url: &str, title: &str, body: &str, label: &str) -> String {
    let mut url = format!("{}/issues/new?title={}&body={}", repo_url.trim_end_matches('/'), quote_plus(title), quote_plus(&url_body(body)));
    if !label.is_empty() {
        url.push_str(&format!("&labels={}", quote_plus(label)));
    }
    url
}

/// Resolves the issue body: a readable body file wins, then the inline
/// body, then the placeholder. Mirrors the `os.access(R_OK)` gate —
/// unreadable paths raise `Body file is not readable`, as upstream.
pub fn resolve_body(body: &str, body_file: Option<&str>) -> Result<String, String> {
    if let Some(path) = body_file {
        let readable = std::fs::metadata(path)
            .map(|metadata| !metadata.permissions().readonly())
            .unwrap_or(false);
        if !readable {
            return Err(format!("Body file is not readable: {path}"));
        }
        let text = std::fs::read_to_string(path).map_err(|_| format!("Body file is not readable: {path}"))?;
        return Ok(text);
    }
    if body.is_empty() {
        Ok(BODY_PLACEHOLDER.to_string())
    } else {
        Ok(body.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn encodes_urls_like_quote_plus() {
        assert_eq!(quote_plus("a b+c~d"), "a+b%2Bc~d");
        assert_eq!(quote_plus("x/y?z"), "x%2Fy%3Fz");
        let url = issue_url("https://github.com/kyth-os/kyth/", "T wine", "b", "bug");
        assert_eq!(url, "https://github.com/kyth-os/kyth/issues/new?title=T+wine&body=b&labels=bug");
        let unlabeled = issue_url(DEFAULT_REPO_URL, "T", "b", "");
        assert!(!unlabeled.contains("labels="));
    }

    #[test]
    fn truncates_long_bodies_for_the_url_only() {
        let body = "x".repeat(MAX_BODY + 1);
        let encoded = url_body(&body);
        assert!(encoded.ends_with(TRUNCATION_NOTE));
        assert_eq!(render_draft("T", &body), format!("# T\n\n{body}\n"));
    }

    #[test]
    fn resolves_bodies_with_placeholder_fallback() {
        assert_eq!(resolve_body("", None).unwrap(), BODY_PLACEHOLDER);
        assert_eq!(resolve_body("hi", None).unwrap(), "hi");
        assert!(resolve_body("", Some("/nonexistent-body-file")).is_err());
        let dir = tempdir().unwrap();
        let file = dir.path().join("body.md");
        std::fs::write(&file, "from file").unwrap();
        assert_eq!(resolve_body("", Some(file.to_str().unwrap())).unwrap(), "from file");
    }

    #[test]
    fn stamps_local_timestamps() {
        let stamp = local_timestamp();
        assert_eq!(stamp.len(), 15);
        assert!(stamp.chars().all(|c| c.is_ascii_digit() || c == '-'));
    }
}
