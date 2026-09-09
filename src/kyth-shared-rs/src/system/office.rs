//! Offline office-suite association preference.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficeSuite {
    Libre,
    OnlyOffice,
}

impl OfficeSuite {
    fn as_str(self) -> &'static str {
        match self {
            Self::Libre => "libre",
            Self::OnlyOffice => "onlyoffice",
        }
    }
}

pub fn parse(value: Option<&str>) -> OfficeSuite {
    if value == Some("onlyoffice") {
        OfficeSuite::OnlyOffice
    } else {
        OfficeSuite::Libre
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    if let Some(path) = path {
        return path.as_ref().to_path_buf();
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join("kyth/office.toml");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
        .join(".config/kyth/office.toml")
}

pub fn load(path: impl AsRef<Path>) -> OfficeSuite {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return OfficeSuite::Libre;
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return OfficeSuite::Libre;
    };
    parse(value.get("suite").and_then(toml::Value::as_str))
}

pub fn save(path: impl AsRef<Path>, suite: OfficeSuite) -> std::io::Result<()> {
    crate::atomic_io::atomic_write_text(
        path,
        &format!("# Kyth Office assoc\nsuite = {:?}\n", suite.as_str()),
        Some(0o600),
    )
}

pub fn mime_for_suite(suite: OfficeSuite) -> BTreeMap<&'static str, &'static str> {
    if suite == OfficeSuite::OnlyOffice {
        BTreeMap::from([
            (
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "onlyoffice-desktopeditors.desktop",
            ),
            (
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                "onlyoffice-desktopeditors.desktop",
            ),
        ])
    } else {
        BTreeMap::from([(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "libreoffice-writer.desktop",
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn defaults_and_round_trips_suite() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("office.toml");
        assert_eq!(load(&path), OfficeSuite::Libre);
        save(&path, OfficeSuite::OnlyOffice).unwrap();
        assert_eq!(load(&path), OfficeSuite::OnlyOffice);
        assert_eq!(mime_for_suite(OfficeSuite::OnlyOffice).len(), 2);
    }
}
