//! Small URL projection helpers shared by native shells.
//!
//! This deliberately implements only RFC 3986's unreserved-set encoding.
//! Callers must still construct and allowlist the destination URL; this
//! helper only makes caller-supplied query values safe to interpolate.

/// Percent-encode UTF-8 bytes which are not RFC 3986 unreserved characters.
pub fn percent_encode(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                output.push(byte as char)
            }
            _ => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::percent_encode;

    #[test]
    fn encodes_query_delimiters_and_utf8_bytes() {
        assert_eq!(
            percent_encode("title & body/# ✓"),
            "title%20%26%20body%2F%23%20%E2%9C%93"
        );
        assert_eq!(percent_encode("safe-_.~chars"), "safe-_.~chars");
    }
}
