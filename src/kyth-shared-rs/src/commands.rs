//! Safe command-boundary descriptions shared by native callers.
//!
//! This module deliberately does not execute processes. It ports the
//! validation and environment-policy portion of `kyth_shared.commands` so a
//! caller can build an explicit argv and pass it to its own bounded runner.

use std::collections::BTreeMap;

pub const DESKTOP_ENV_KEYS: &[&str] = &[
    "DBUS_SESSION_BUS_ADDRESS",
    "DESKTOP_STARTUP_ID",
    "DISPLAY",
    "HOME",
    "LANG",
    "LC_ALL",
    "PATH",
    "SUDO_ASKPASS",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "XDG_CURRENT_DESKTOP",
    "XDG_RUNTIME_DIR",
];

const UNSAFE_ENV_NAMES: &[&str] = &[
    "BASH_ENV",
    "ENV",
    "GCONV_PATH",
    "PERL5LIB",
    "PERL5OPT",
    "RUBYLIB",
    "RUBYOPT",
    "JAVA_TOOL_OPTIONS",
    "_JAVA_OPTIONS",
    "JDK_JAVA_OPTIONS",
    "NODE_OPTIONS",
];

const UNSAFE_ENV_PREFIXES: &[&str] = &["LD_", "PYTHON", "PERL", "RUBY", "NODE_", "GEM_", "JAVA_"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentPolicy {
    Inherit,
    Sanitized,
    Desktop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub argv: Vec<String>,
    pub name: String,
    pub timeout_seconds: Option<u64>,
    pub environment: EnvironmentPolicy,
    pub sensitive_options: Vec<String>,
}

impl CommandSpec {
    pub fn new(argv: Vec<String>) -> Result<Self, String> {
        normalize_command(&argv)?;
        Ok(Self {
            argv,
            name: "command".into(),
            timeout_seconds: Some(30),
            environment: EnvironmentPolicy::Sanitized,
            sensitive_options: Vec::new(),
        })
    }

    pub fn display_argv(&self) -> Vec<String> {
        let mut displayed = self.argv.clone();
        for index in 0..displayed.len().saturating_sub(1) {
            if self
                .sensitive_options
                .iter()
                .any(|option| option == &displayed[index])
            {
                displayed[index + 1] = "<redacted>".into();
            }
        }
        displayed
    }
}

pub fn normalize_command(argv: &[String]) -> Result<Vec<String>, String> {
    if argv.is_empty() {
        return Err("command must contain at least one argument".into());
    }
    if argv.iter().any(|part| part.is_empty()) {
        return Err("command arguments must not be empty".into());
    }
    Ok(argv.to_vec())
}

pub fn ujust_command(recipe: &str) -> Result<Vec<String>, String> {
    if recipe.is_empty()
        || recipe.chars().any(|character| {
            !(character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '_'
                || character == '-')
        })
        || !recipe.as_bytes()[0].is_ascii_lowercase() && !recipe.as_bytes()[0].is_ascii_digit()
    {
        return Err(format!("Refusing unsafe ujust recipe: {recipe:?}"));
    }
    Ok(vec!["ujust".into(), recipe.into()])
}

pub fn sanitized_environment(environment: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    environment
        .iter()
        .filter(|(key, _)| {
            !UNSAFE_ENV_NAMES
                .iter()
                .any(|unsafe_name| key.as_str() == *unsafe_name)
                && !UNSAFE_ENV_PREFIXES
                    .iter()
                    .any(|prefix| key.starts_with(prefix))
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

pub fn desktop_environment(environment: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    environment
        .iter()
        .filter(|(key, _)| {
            DESKTOP_ENV_KEYS
                .iter()
                .any(|allowed| key.as_str() == *allowed)
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

pub fn environment_for(
    policy: EnvironmentPolicy,
    environment: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    match policy {
        EnvironmentPolicy::Inherit => environment.clone(),
        EnvironmentPolicy::Sanitized => sanitized_environment(environment),
        EnvironmentPolicy::Desktop => desktop_environment(environment),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("PATH".into(), "/usr/bin".into()),
            ("HOME".into(), "/home/test".into()),
            ("LD_PRELOAD".into(), "/tmp/inject.so".into()),
            ("PYTHONPATH".into(), "/tmp/python".into()),
            (
                "DBUS_SESSION_BUS_ADDRESS".into(),
                "unix:path=/run/user".into(),
            ),
            ("TERM".into(), "xterm".into()),
        ])
    }

    #[test]
    fn validates_argv_and_ujust_recipe_without_shell_expansion() {
        assert_eq!(
            ujust_command("gaming-mode").unwrap(),
            vec!["ujust", "gaming-mode"]
        );
        assert!(ujust_command("gaming mode").is_err());
        assert!(ujust_command("../unsafe").is_err());
        assert!(normalize_command(&[]).is_err());
        assert!(normalize_command(&["".into()]).is_err());
    }

    #[test]
    fn applies_environment_policies() {
        let source = env();
        let sanitized = sanitized_environment(&source);
        assert!(sanitized.contains_key("PATH"));
        assert!(!sanitized.contains_key("LD_PRELOAD"));
        assert!(!sanitized.contains_key("PYTHONPATH"));
        assert_eq!(desktop_environment(&source).len(), 3);
        assert!(environment_for(EnvironmentPolicy::Inherit, &source).contains_key("TERM"));
    }

    #[test]
    fn redacts_values_following_sensitive_options() {
        let mut spec = CommandSpec::new(vec![
            "tool".into(),
            "--token".into(),
            "secret".into(),
            "--mode".into(),
            "safe".into(),
        ])
        .unwrap();
        spec.sensitive_options = vec!["--token".into()];
        assert_eq!(
            spec.display_argv(),
            vec!["tool", "--token", "<redacted>", "--mode", "safe"]
        );
    }
}
