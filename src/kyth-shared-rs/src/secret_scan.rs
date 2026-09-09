//! High-confidence committed-secret pattern matching.
//!
//! The build script still owns Git enumeration and reporting policy. This
//! module only scans supplied text, which keeps the security rules reusable
//! and easy to test without invoking a shell or reading the worktree.

use regex::{Regex, RegexBuilder};
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretFinding {
    pub kind: &'static str,
}

fn patterns() -> &'static [(&'static str, Regex)] {
    static PATTERNS: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            (
                "private key block",
                Regex::new(r"-----BEGIN [A-Z ]*PRIVATE KEY-----").unwrap(),
            ),
            (
                "age key",
                Regex::new(r"AGE-SECRET-KEY-1[0-9A-Z]{58}").unwrap(),
            ),
            (
                "cosign private key",
                Regex::new(r"-----BEGIN ENCRYPTED COSIGN PRIVATE KEY-----").unwrap(),
            ),
            (
                "GitHub token",
                Regex::new(r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9_]{36,}\b").unwrap(),
            ),
            (
                "GitHub fine-grained token",
                Regex::new(r"\bgithub_pat_[A-Za-z0-9_]{80,}\b").unwrap(),
            ),
            (
                "AWS access key",
                Regex::new(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b").unwrap(),
            ),
            (
                "Slack token",
                Regex::new(r"\bxox[baprs]-[A-Za-z0-9-]{20,}\b").unwrap(),
            ),
            (
                "generic high-entropy secret",
                RegexBuilder::new(r"\b[A-Za-z0-9+/]{40,}={0,2}\b.*\b(?:secret|token|key)\b")
                    .case_insensitive(true)
                    .build()
                    .unwrap(),
            ),
        ]
    })
}

pub fn is_binary_suffix(path: impl AsRef<Path>) -> bool {
    matches!(
        path.as_ref()
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("cer" | "png" | "jpg" | "jpeg" | "webp" | "ico")
    )
}

pub fn scan_text(text: &str) -> Vec<SecretFinding> {
    patterns()
        .iter()
        .filter_map(|(kind, pattern)| pattern.is_match(text).then_some(SecretFinding { kind }))
        .collect()
}

pub fn scan_file_text(path: impl AsRef<Path>, text: &str) -> Vec<SecretFinding> {
    if is_binary_suffix(path) {
        Vec::new()
    } else {
        scan_text(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catches_high_confidence_secret_formats() {
        let findings = scan_text("token=ghp_abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ");
        assert!(findings
            .iter()
            .any(|finding| finding.kind == "GitHub token"));
        assert!(scan_text("-----BEGIN PRIVATE KEY-----")
            .iter()
            .any(|finding| finding.kind == "private key block"));
    }

    #[test]
    fn binary_suffixes_are_excluded_by_file_helper() {
        assert!(is_binary_suffix("logo.PNG"));
        assert!(scan_file_text(
            "logo.png",
            "ghp_abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ"
        )
        .is_empty());
        assert!(!is_binary_suffix("README.md"));
    }

    #[test]
    fn ordinary_text_has_no_findings() {
        assert!(scan_text("This is a normal build note with no credentials.").is_empty());
    }
}
