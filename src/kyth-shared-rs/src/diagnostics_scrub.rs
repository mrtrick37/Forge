//! Port of `kyth_shared.diagnostics_scrub`.
//!
//! This is intentionally a pure text transform. It does not collect
//! diagnostics or upload anything; it is the final safety boundary before a
//! report is handed to a browser or another public destination.

use regex::Regex;
use std::net::IpAddr;

fn replace_with<F>(regex: &Regex, text: String, replacement: F) -> String
where
    F: Fn(&regex::Captures<'_>) -> String,
{
    regex.replace_all(&text, replacement).into_owned()
}

pub fn scrub_logs(text: &str) -> String {
    let private_key = Regex::new(
        r"(?s)-----BEGIN [^-\r\n]*PRIVATE KEY-----.*?-----END [^-\r\n]*PRIVATE KEY-----",
    )
    .expect("private-key regex is valid");
    let auth_value = Regex::new(
        r"(?i)(\b(?:authorization|proxy-authorization)\s*[:=]\s*)(?:bearer|basic|token)\s+\S+",
    )
    .expect("auth regex is valid");
    let sensitive_header = Regex::new(
        r"(?im)(\b(?:authorization|proxy-authorization|cookie|set-cookie)\s*[:=]\s*)[^\r\n]+",
    )
    .expect("header regex is valid");
    let secret_value = Regex::new(
        r#"(?i)(["']?(?:access[_-]?token|refresh[_-]?token|id[_-]?token|bearer|password|passwd|passphrase|client[_-]?secret|api[_-]?key|private[_-]?key|auth[_-]?cookie|session[_-]?cookie|cookie|secret)["']?\s*[:=]\s*)("[^"\r\n]*"|'[^'\r\n]*'|[^\s,;&]+)"#,
    )
    .expect("secret-value regex is valid");
    let secret_flag =
        Regex::new(r"(?i)(--(?:cookie|password|passwd|token|client-secret|api-key)(?:=|\s+))\S+")
            .expect("secret-flag regex is valid");
    let secret_query = Regex::new(
        r"(?i)([?&](?:access_token|refresh_token|id_token|token|password|passwd|secret|cookie|api_key|client_secret)=)[^&#\s]+",
    )
    .expect("secret-query regex is valid");
    let url_credentials =
        Regex::new(r"(?i)(\b[a-z][a-z0-9+.-]*://)[^/@\s:]+:[^/@\s]+@").expect("URL regex is valid");

    let mut text = private_key
        .replace_all(text, "[private key redacted]")
        .into_owned();
    text = auth_value.replace_all(&text, "$1[redacted]").into_owned();
    text = sensitive_header
        .replace_all(&text, "$1[redacted]")
        .into_owned();
    text = replace_with(&secret_value, text, |captures| {
        let value = captures.get(2).map_or("", |match_| match_.as_str());
        let replacement = if value.starts_with('"') {
            "\"[redacted]\""
        } else if value.starts_with('\'') {
            "'[redacted]'"
        } else {
            "[redacted]"
        };
        format!("{}{}", &captures[1], replacement)
    });
    text = secret_flag.replace_all(&text, "$1[redacted]").into_owned();
    text = secret_query.replace_all(&text, "$1[redacted]").into_owned();
    text = url_credentials
        .replace_all(&text, "$1[credentials-redacted]@")
        .into_owned();

    for (pattern, replacement) in [
        (r"(?i)hostname[:=]\s*\S+", "hostname=redacted"),
        (r"(?i)serial[:=]\s*\S+", "serial=redacted"),
        (r"(?i)Serial\s*[:=]\s*\S+", "Serial: [scrubbed]"),
        (r"(?i)SSID[:=]\s*\S+", "SSID=redacted"),
        (r"(?i)SSID\s*[:=]\s*\S+", "SSID: [scrubbed]"),
    ] {
        text = Regex::new(pattern)
            .expect("identity regex is valid")
            .replace_all(&text, replacement)
            .into_owned();
    }

    text = Regex::new(r"[0-9a-fA-F]{2}(:[0-9a-fA-F]{2}){5}")
        .expect("MAC regex is valid")
        .replace_all(&text, "xx:xx:xx:xx:xx:xx")
        .into_owned();
    text = Regex::new(r"\b\d{1,3}(?:\.\d{1,3}){3}\b")
        .expect("IPv4 regex is valid")
        .replace_all(&text, "xxx.xxx.xxx.xxx")
        .into_owned();

    // Rust's regex crate deliberately has no look-around. The broad match
    // still mirrors the Python candidate for normal IPv6 literals, and the
    // parser prevents ordinary colon-separated text from being redacted.
    let ipv6_candidate =
        Regex::new(r"(?:[0-9A-Fa-f]*:){2,}[0-9A-Fa-f]*").expect("IPv6 regex is valid");
    text = replace_with(&ipv6_candidate, text, |captures| {
        let candidate = captures.get(0).map_or("", |match_| match_.as_str());
        if candidate
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_ipv6())
        {
            "xxxx:xxxx:xxxx:xxxx::xxxx".to_string()
        } else {
            candidate.to_string()
        }
    });
    text = Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}")
        .expect("email regex is valid")
        .replace_all(&text, "redacted@example.com")
        .into_owned();
    text = Regex::new(
        r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b",
    )
    .expect("UUID regex is valid")
    .replace_all(&text, "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx")
    .into_owned();
    text = Regex::new(r"/(var/home|home)/[^/\s:]+")
        .expect("home-path regex is valid")
        .replace_all(&text, "/$1/redacted")
        .into_owned();

    // The Python implementation uses the current hostname and USER as final
    // fallbacks. Keep those privacy guarantees without spawning a command.
    if let Ok(value) = std::env::var("HOSTNAME") {
        if value.len() > 2 {
            let escaped = regex::escape(&value);
            if let Ok(pattern) = Regex::new(&format!(r"\b{escaped}\b")) {
                text = pattern.replace_all(&text, "[hostname]").into_owned();
            }
        }
    }
    for key in ["USER", "USERNAME"] {
        if let Ok(value) = std::env::var(key) {
            if value.len() > 1 {
                let escaped = regex::escape(&value);
                if let Ok(pattern) = Regex::new(&format!(r"\b{escaped}\b")) {
                    text = pattern.replace_all(&text, "redacted").into_owned();
                }
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::scrub_logs;

    #[test]
    fn redacts_credentials_and_structured_secrets() {
        let report = concat!(
            "Authorization: Bearer bearer-secret\n",
            "Cookie: session=browser-secret; theme=dark\n",
            "oauth={\"access_token\":\"access-secret\",\"refresh_token\":\"refresh-secret\"}\n",
            "password=hunter2 api_key=api-secret\n",
            "command --cookie cli-secret --token=flag-secret\n",
        );
        let scrubbed = scrub_logs(&report);
        for secret in [
            "bearer-secret",
            "browser-secret",
            "access-secret",
            "refresh-secret",
            "hunter2",
            "api-secret",
            "cli-secret",
            "flag-secret",
        ] {
            assert!(!scrubbed.contains(secret), "secret leaked: {secret}");
        }
    }

    #[test]
    fn redacts_keys_urls_network_ids_and_paths() {
        let key_begin = "-----BEGIN ".to_owned() + "PRIVATE KEY-----";
        let key_end = "-----END ".to_owned() + "PRIVATE KEY-----";
        let report = format!(
            "{key_begin}\nprivate-material\n{key_end}\n{}{}{}",
            "https://alice:password@example.test/path?access_token=query-secret&safe=yes\n",
            "IPv6 2001:db8:85a3::8a2e:370:7334 IPv4 192.0.2.10\n",
            "home=/var/home/alice/project\n",
        );
        let scrubbed = scrub_logs(&report);
        for secret in [
            "private-material",
            "alice:password",
            "query-secret",
            "2001:db8:85a3::8a2e:370:7334",
            "192.0.2.10",
            "/var/home/alice",
        ] {
            assert!(!scrubbed.contains(secret), "secret leaked: {secret}");
        }
    }

    #[test]
    fn preserves_non_sensitive_text() {
        let scrubbed = scrub_logs("plain status: ok\nvalue=42");
        assert_eq!(scrubbed, "plain status: ok\nvalue=42");
    }
}
