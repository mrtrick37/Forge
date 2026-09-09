//! Offline KWin latency profile and drop-in rendering.
//!
//! This ports the profile normalization and generated-content portion of
//! kwin_latency.py. Installing the files and reloading KWin remain explicit
//! caller actions.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KwinLatencyConfig {
    pub profile: String,
    pub tearing: bool,
}

impl Default for KwinLatencyConfig {
    fn default() -> Self {
        Self {
            profile: "balanced".into(),
            tearing: false,
        }
    }
}

impl KwinLatencyConfig {
    pub fn normalized(profile: impl AsRef<str>, tearing: bool) -> Self {
        let profile = profile.as_ref().to_ascii_lowercase();
        let profile = if matches!(profile.as_str(), "balanced" | "gaming") {
            profile
        } else {
            "balanced".into()
        };
        Self { profile, tearing }
    }

    pub fn load(path: impl AsRef<Path>) -> Self {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let Ok(value) = raw.parse::<toml::Value>() else {
            return Self::default();
        };
        let profile = value
            .get("profile")
            .and_then(toml::Value::as_str)
            .unwrap_or("balanced");
        let default_tearing = profile.eq_ignore_ascii_case("gaming");
        Self::normalized(
            profile,
            value
                .get("tearing")
                .and_then(toml::Value::as_bool)
                .unwrap_or(default_tearing),
        )
    }

    pub fn to_toml(&self) -> String {
        format!(
            "# Kyth KWin latency — offline\nprofile = {:?}\ntearing = {}\n",
            self.profile, self.tearing
        )
    }

    pub fn render_dropin(&self) -> Option<String> {
        if self.profile != "gaming" {
            return None;
        }
        Some(format!(
            "# Kyth KWin latency — generated\n\
             [Compositing]\n\
             MaxFPS=1000\n\
             RefreshRate=0\n\
             AllowTearing={}\n",
            if self.tearing { "true" } else { "false" }
        ))
    }

    pub fn render_environment(&self) -> Option<&'static str> {
        (self.profile == "gaming")
            .then_some("# Kyth KWin — generated\nKWIN_DRM_PREFER_COLOR_DEPTH=24\n")
    }
}

pub fn status(dropin_exists: bool) -> &'static str {
    if dropin_exists {
        "gaming"
    } else {
        "balanced"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn normalizes_profile_and_uses_gaming_default_tearing() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("kwin-latency.toml");
        std::fs::write(&path, "profile = \"gaming\"\n").unwrap();
        let config = KwinLatencyConfig::load(&path);
        assert_eq!(config, KwinLatencyConfig::normalized("gaming", true));
        assert!(config
            .render_dropin()
            .unwrap()
            .contains("AllowTearing=true"));
    }

    #[test]
    fn balanced_has_no_generated_dropins() {
        let config = KwinLatencyConfig::default();
        assert!(config.render_dropin().is_none());
        assert!(config.render_environment().is_none());
        assert_eq!(status(false), "balanced");
    }
}
