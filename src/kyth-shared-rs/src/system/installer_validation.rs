//! Shared installer identity/input validation contract.

use regex::Regex;

const USERNAME: &str = r"^[a-z_][a-z0-9_-]{0,30}$";
const HOSTNAME: &str = r"^[a-zA-Z0-9]([a-zA-Z0-9\-]{0,61}[a-zA-Z0-9])?$";
const LOCALE: &str = r"^[A-Za-z0-9_.@-]{1,64}$";
const KEYMAP: &str = r"^[A-Za-z0-9_.+@/-]{1,64}$";

fn matches(pattern: &str, value: &str) -> bool {
    Regex::new(pattern)
        .expect("installer validation regex is valid")
        .is_match(value)
}

pub fn valid_username(value: &str) -> bool {
    matches(USERNAME, value)
}
pub fn valid_hostname(value: &str) -> bool {
    matches(HOSTNAME, value)
}
pub fn valid_locale(value: &str) -> bool {
    matches(LOCALE, value)
}
pub fn valid_keymap(value: &str) -> bool {
    matches(KEYMAP, value)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationErrors {
    pub hostname: bool,
    pub username: bool,
    pub locale: bool,
    pub keymap: bool,
}

impl ValidationErrors {
    pub fn is_valid(&self) -> bool {
        self.hostname && self.username && self.locale && self.keymap
    }
}

pub fn validate(hostname: &str, username: &str, locale: &str, keymap: &str) -> ValidationErrors {
    ValidationErrors {
        hostname: valid_hostname(hostname),
        username: valid_username(username),
        locale: valid_locale(locale),
        keymap: valid_keymap(keymap),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_python_contract_boundaries() {
        assert!(valid_username("kyth_user-1"));
        assert!(!valid_username("Kyth"));
        assert!(valid_hostname("kyth-box"));
        assert!(!valid_hostname("-kyth"));
        assert!(valid_locale("en_US.UTF-8"));
        assert!(valid_keymap("us-intl"));
        assert!(!validate("-kyth", "user", "en_US.UTF-8", "us").is_valid());
    }
}
