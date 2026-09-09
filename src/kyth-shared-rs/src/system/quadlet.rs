//! Offline Quadlet gaming-service preset model.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuadletService {
    pub image: String,
    pub auto: bool,
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join("kyth/quadlet.toml");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
        .join(".config/kyth/quadlet.toml")
}

pub fn load(path: impl AsRef<Path>) -> BTreeMap<String, QuadletService> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return BTreeMap::new();
    };
    value
        .get("services")
        .and_then(toml::Value::as_table)
        .map(|services| {
            services
                .iter()
                .filter_map(|(name, value)| {
                    let table = value.as_table()?;
                    Some((
                        name.clone(),
                        QuadletService {
                            image: table
                                .get("image")
                                .and_then(toml::Value::as_str)
                                .unwrap_or("")
                                .into(),
                            auto: table
                                .get("auto")
                                .and_then(toml::Value::as_bool)
                                .unwrap_or(false),
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn save(
    path: impl AsRef<Path>,
    services: &BTreeMap<String, QuadletService>,
) -> std::io::Result<()> {
    let mut text = String::from("# Kyth quadlet gaming services\n");
    for (name, service) in services {
        text.push_str(&format!(
            "[services.{name:?}]\nimage = {:?}\nauto = {}\n\n",
            service.image, service.auto
        ));
    }
    crate::atomic_io::atomic_write_text(path, &text, Some(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_and_saves_service_table() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("quadlet.toml");
        let mut services = BTreeMap::new();
        services.insert(
            "gamescope".into(),
            QuadletService {
                image: "registry/kyth-game:latest".into(),
                auto: true,
            },
        );
        save(&path, &services).unwrap();
        assert_eq!(load(&path), services);
    }
}
