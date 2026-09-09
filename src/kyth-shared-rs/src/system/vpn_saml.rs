//! Pure command projection for the VPN SAML sleep-survival helper.

pub const SLEEP_SURVIVE: bool = true;

pub const VPN_PROTOCOLS: &[&str] = &["gp", "anyconnect", "pulse", "nc", "f5", "fortinet", "array"];
pub const VPN_OS_OPTIONS: &[&str] = &["win", "linux", "mac"];
const MAX_SAML_FORM_BYTES: usize = 2 * 1024 * 1024;
const MAX_SAML_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_VPN_SECRET_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenconnectCommand {
    pub argv: Vec<String>,
    pub stdin: Option<String>,
}

pub fn validate_profile(
    gateway: &str,
    protocol: &str,
    os_emulation: &str,
    username: &str,
) -> Result<(), String> {
    if gateway.is_empty()
        || gateway.len() > 2048
        || !gateway.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'.' | b'_'
                        | b':'
                        | b'/'
                        | b'?'
                        | b'&'
                        | b'='
                        | b'%'
                        | b'+'
                        | b'@'
                        | b'['
                        | b']'
                        | b'-'
                )
        })
    {
        return Err("VPN gateway contains unsupported characters".into());
    }
    if !VPN_PROTOCOLS.contains(&protocol) {
        return Err("unsupported VPN protocol".into());
    }
    if !VPN_OS_OPTIONS.contains(&os_emulation) {
        return Err("unsupported VPN OS emulation".into());
    }
    if username.len() > 256 || username.chars().any(char::is_control) {
        return Err("VPN username contains control characters".into());
    }
    Ok(())
}

fn validate_secret(field: &str, value: &str) -> Result<(), String> {
    let maximum = if field == "password" {
        MAX_VPN_SECRET_BYTES
    } else {
        MAX_SAML_RESPONSE_BYTES
    };
    if value.len() > maximum || value.chars().any(char::is_control) {
        return Err(format!("VPN {field} contains invalid characters"));
    }
    Ok(())
}

pub fn build_initial_command(
    gateway: &str,
    protocol: &str,
    os_emulation: &str,
    username: &str,
    password: &str,
) -> Result<OpenconnectCommand, String> {
    validate_profile(gateway, protocol, os_emulation, username)?;
    validate_secret("password", password)?;
    let mut argv = vec![
        "sudo".into(),
        "-E".into(),
        "-A".into(),
        "/usr/bin/openconnect".into(),
        "--protocol".into(),
        protocol.into(),
        "--os".into(),
        os_emulation.into(),
        "--script".into(),
        "/usr/libexec/kyth-vpnc-script".into(),
    ];
    if !password.is_empty() {
        argv.push("--passwd-on-stdin".into());
    }
    if !username.is_empty() {
        argv.extend(["--user".into(), username.into()]);
    }
    argv.push(gateway.into());
    Ok(OpenconnectCommand {
        argv,
        stdin: (!password.is_empty()).then(|| format!("{password}\n")),
    })
}

pub fn build_reconnect_command(
    gateway: &str,
    protocol: &str,
    os_emulation: &str,
    interface: &str,
    cookie: &str,
    configured_username: &str,
) -> Result<OpenconnectCommand, String> {
    validate_profile(gateway, protocol, os_emulation, configured_username)?;
    validate_secret("cookie", cookie)?;
    if !matches!(interface, "portal" | "gateway") {
        return Err("invalid VPN authentication interface".into());
    }
    let (field, value, saml_username) = parse_gp_saml_cookie(cookie);
    let password_mode = protocol == "gp" && !field.is_empty() && !value.is_empty();
    let username = if saml_username.is_empty() {
        configured_username
    } else {
        &saml_username
    };
    let mut argv = vec![
        "sudo".into(),
        "-E".into(),
        "-A".into(),
        "/usr/bin/openconnect".into(),
        "--protocol".into(),
        protocol.into(),
        "--os".into(),
        os_emulation.into(),
        "--script".into(),
        "/usr/libexec/kyth-vpnc-script".into(),
    ];
    if password_mode {
        argv.extend([
            "--usergroup".into(),
            format!("{interface}:{field}"),
            "--passwd-on-stdin".into(),
        ]);
    } else {
        argv.push("--cookie-on-stdin".into());
    }
    if !username.is_empty() {
        argv.extend(["--user".into(), username.into()]);
    }
    argv.push(gateway.into());
    Ok(OpenconnectCommand {
        argv,
        stdin: Some(format!(
            "{}\n",
            if password_mode {
                value.as_str()
            } else {
                cookie
            }
        )),
    })
}

pub fn saml_url_from_log_line(line: &str) -> Option<String> {
    let start = line.find("SAML REDIRECT")?;
    let rest = &line[start..];
    let marker = rest.find("via https://")?;
    let url = rest[marker + 4..]
        .split_whitespace()
        .next()?
        .trim_end_matches([')', ',', ';']);
    validate_saml_redirect_url(url)
        .ok()
        .map(|_| url.to_string())
}

pub fn gp_interface_from_log_line(line: &str) -> Option<&'static str> {
    if line.contains("/global-protect/prelogin.esp") {
        Some("portal")
    } else if line.contains("/ssl-vpn/prelogin.esp") {
        Some("gateway")
    } else {
        None
    }
}

pub fn line_is_connected(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    [
        "connected as",
        "established dtls",
        "established cstp",
        "esp session established",
        "esp tunnel connected",
        "configured as",
    ]
    .iter()
    .any(|marker| line.contains(marker))
}

/// Validate a browser destination discovered in openconnect output. The
/// redirect is untrusted child-process output, so only a conventional HTTPS
/// URL with a public-style host and no credentials or fragment is accepted.
pub fn validate_saml_redirect_url(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 8192 || value.chars().any(char::is_control) {
        return Err("SAML redirect URL is invalid or too large".into());
    }
    if value.contains('#') || value.contains('@') || !value.starts_with("https://") {
        return Err("SAML redirect URL must be HTTPS without credentials or fragments".into());
    }
    let (_, port) = origin(value, false)?;
    if port != 443 {
        return Err("SAML redirect URL must use HTTPS port 443".into());
    }
    Ok(())
}

pub fn redact_log_line(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    for marker in [
        "portal-userauthcookie=",
        "portal-prelogonuserauthcookie=",
        "prelogin-cookie=",
        "preloginuserauthcookie=",
        "cas=",
    ] {
        if let Some(index) = lower.find(marker) {
            let end = index + marker.len();
            return format!("{}<redacted>", &line[..end]);
        }
    }
    line.chars().take(400).collect()
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                out.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        out.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

pub fn parse_gp_saml_cookie(cookie: &str) -> (String, String, String) {
    let raw = cookie.trim();
    if raw.is_empty() {
        return (String::new(), String::new(), String::new());
    }
    let names = [
        "preloginuserauthcookie",
        "portal-userauthcookie",
        "cas",
        "prelogin-cookie",
    ];
    let mut username = String::new();
    let mut values = Vec::new();
    for part in raw.split('&') {
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        let key = percent_decode(key).trim().to_ascii_lowercase();
        let value = percent_decode(value);
        if key == "saml-username" {
            username = value.clone();
        }
        values.push((key, value));
    }
    if let Some((key, value)) = values
        .iter()
        .find(|(key, value)| names.contains(&key.as_str()) && !value.is_empty())
    {
        return (key.clone(), value.clone(), username);
    }
    if let Some((key, value)) = values
        .into_iter()
        .last()
        .filter(|(key, value)| names.contains(&key.as_str()) && !value.is_empty())
    {
        return (key, value, username);
    }
    ("prelogin-cookie".into(), raw.into(), username)
}

fn origin(value: &str, bare_host: bool) -> Result<(String, u16), String> {
    let candidate = if bare_host && !value.contains("://") {
        format!("https://{value}")
    } else {
        value.to_string()
    };
    let rest = candidate
        .strip_prefix("https://")
        .ok_or_else(|| "SAML URL must use HTTPS".to_string())?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return Err("SAML URL has invalid authority".into());
    }
    if authority.starts_with('[') {
        let end = authority
            .find(']')
            .ok_or_else(|| "SAML URL has invalid IPv6 host".to_string())?;
        let host = authority[1..end].to_ascii_lowercase();
        let port = authority[end + 1..]
            .strip_prefix(':')
            .map_or(Ok(443), |raw| {
                raw.parse()
                    .map_err(|_| "SAML URL has invalid port".to_string())
            })?;
        return Ok((host, port));
    }
    let (host, port) = authority
        .rsplit_once(':')
        .map_or((authority, "443"), |(host, port)| (host, port));
    if host.is_empty()
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err("SAML URL has invalid hostname".into());
    }
    Ok((
        host.trim_end_matches('.').to_ascii_lowercase(),
        port.parse()
            .map_err(|_| "SAML URL has invalid port".to_string())?,
    ))
}

pub fn validate_saml_acs_url(action_url: &str, expected_gateway: &str) -> Result<(), String> {
    if action_url.contains('#') {
        return Err("SAML ACS destination must not contain a fragment".into());
    }
    let path = action_url
        .strip_prefix("https://")
        .and_then(|rest| {
            rest.split_once('/')
                .map(|(_, path)| path.split(['?', '#']).next().unwrap_or(""))
        })
        .unwrap_or("");
    if path.trim_end_matches('/') != "SAML20/SP/ACS" {
        return Err("SAML ACS destination has an unexpected path".into());
    }
    if origin(action_url, false)? != origin(expected_gateway, true)? {
        return Err("SAML ACS destination does not match the VPN gateway".into());
    }
    Ok(())
}

fn xml_tag(text: &str, tag: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = lower.find(&open)? + open.len();
    let end = lower[start..].find(&close)? + start;
    let value = text[start..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn form_encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

pub fn parse_saml_acs_response(headers: &str, body: &str) -> Option<String> {
    let names = [
        "prelogin-cookie",
        "portal-userauthcookie",
        "cas",
        "preloginuserauthcookie",
    ];
    for name in names {
        for line in headers.lines() {
            if let Some((key, value)) = line.split_once(':') {
                if key.trim().eq_ignore_ascii_case(name) && !value.trim().is_empty() {
                    let username = headers
                        .lines()
                        .find_map(|line| {
                            line.split_once(':').and_then(|(key, value)| {
                                key.trim()
                                    .eq_ignore_ascii_case("saml-username")
                                    .then(|| value.trim())
                            })
                        })
                        .unwrap_or("");
                    return Some(format!(
                        "{name}={}&saml-username={}",
                        form_encode(value.trim()),
                        form_encode(username)
                    ));
                }
            }
        }
        if let Some(value) = xml_tag(body, name) {
            let username = xml_tag(body, "saml-username").unwrap_or_default();
            return Some(format!(
                "{name}={}&saml-username={}",
                form_encode(&value),
                form_encode(&username)
            ));
        }
    }
    None
}

pub fn replay_saml_command(
    action_url: &str,
    body: &str,
    expected_gateway: &str,
) -> Result<(Vec<String>, Vec<u8>), String> {
    validate_saml_acs_url(action_url, expected_gateway)?;
    if body.len() > MAX_SAML_FORM_BYTES
        || !body.split('&').any(|part| {
            part.split_once('=')
                .is_some_and(|(key, _)| key == "SAMLResponse")
        })
    {
        return Err("SAML ACS form is invalid or too large".into());
    }
    Ok((
        vec![
            "curl".into(),
            "--silent".into(),
            "--show-error".into(),
            "--fail-with-body".into(),
            "--max-time".into(),
            "30".into(),
            "--connect-timeout".into(),
            "10".into(),
            "--max-redirs".into(),
            "0".into(),
            "--proto".into(),
            "=https".into(),
            "--request".into(),
            "POST".into(),
            "--header".into(),
            "Content-Type: application/x-www-form-urlencoded".into(),
            "--header".into(),
            "User-Agent: PAN GlobalProtect".into(),
            "--data-binary".into(),
            "@-".into(),
            action_url.into(),
        ],
        body.as_bytes().to_vec(),
    ))
}

/// Return the TERM-then-KILL cascade used by the bounded VPN worker.
pub fn kill_cascade(pid: u32) -> Vec<Vec<String>> {
    ["TERM", "KILL"]
        .into_iter()
        .map(|signal| vec!["kill".into(), format!("-{signal}"), pid.to_string()])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascade_is_ordered_and_sleep_survival_is_enabled() {
        assert!(SLEEP_SURVIVE);
        assert_eq!(
            kill_cascade(42),
            vec![vec!["kill", "-TERM", "42"], vec!["kill", "-KILL", "42"],]
        );
    }

    #[test]
    fn openconnect_command_keeps_password_out_of_argv() {
        let command =
            build_initial_command("https://vpn.example/gp", "gp", "win", "pat", "secret").unwrap();
        assert!(command.argv.iter().all(|arg| arg != "secret"));
        assert_eq!(command.stdin.as_deref(), Some("secret\n"));
        assert!(command.argv.contains(&"--passwd-on-stdin".into()));
    }

    #[test]
    fn saml_cookie_and_acs_response_are_parsed() {
        assert_eq!(
            parse_gp_saml_cookie("portal-userauthcookie=abc&saml-username=pat"),
            ("portal-userauthcookie".into(), "abc".into(), "pat".into())
        );
        assert_eq!(
            parse_saml_acs_response("prelogin-cookie: abc\nsaml-username: pat", ""),
            Some("prelogin-cookie=abc&saml-username=pat".into())
        );
        assert!(validate_saml_acs_url("https://vpn.example/SAML20/SP/ACS", "vpn.example").is_ok());
        assert!(
            validate_saml_acs_url("https://evil.example/SAML20/SP/ACS", "vpn.example").is_err()
        );
    }

    #[test]
    fn saml_redirects_are_limited_to_safe_https_destinations() {
        assert!(validate_saml_redirect_url("https://idp.example/login?request=abc").is_ok());
        assert_eq!(
            saml_url_from_log_line("SAML REDIRECT via https://idp.example/login?request=abc"),
            Some("https://idp.example/login?request=abc".into())
        );
        assert!(validate_saml_redirect_url("http://idp.example/login").is_err());
        assert!(validate_saml_redirect_url("https://user:password@idp.example/login").is_err());
        assert!(validate_saml_redirect_url("https://idp.example/login#fragment").is_err());
        assert!(
            saml_url_from_log_line("SAML REDIRECT via https://idp.example:bad/login").is_none()
        );
    }

    #[test]
    fn replay_uses_stdin_for_the_saml_form() {
        let (argv, input) = replay_saml_command(
            "https://vpn.example/SAML20/SP/ACS",
            "SAMLResponse=token",
            "vpn.example",
        )
        .unwrap();
        assert!(argv.contains(&"@-".into()));
        assert_eq!(input, b"SAMLResponse=token");
    }
}
