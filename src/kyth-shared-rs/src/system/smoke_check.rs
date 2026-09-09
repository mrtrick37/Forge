//! Read-only smoke-check result model and filesystem checks.
//!
//! This ports the deterministic part of `kyth_shared.smoke_check`. It does
//! not invoke services or commands; callers supply command outcomes and keep
//! ownership of live probing and console/JSON policy.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Level {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultRow {
    pub level: Level,
    pub name: String,
    pub detail: String,
    pub section: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub results: Vec<ResultRow>,
}

impl Report {
    pub fn record(
        &mut self,
        level: Level,
        name: impl Into<String>,
        detail: impl Into<String>,
        section: impl Into<String>,
    ) {
        self.results.push(ResultRow {
            level,
            name: name.into(),
            detail: detail.into(),
            section: section.into(),
        });
    }
    pub fn passed(
        &mut self,
        name: impl Into<String>,
        detail: impl Into<String>,
        section: impl Into<String>,
    ) {
        self.record(Level::Pass, name, detail, section);
    }
    pub fn warned(
        &mut self,
        name: impl Into<String>,
        detail: impl Into<String>,
        section: impl Into<String>,
    ) {
        self.record(Level::Warn, name, detail, section);
    }
    pub fn failed(
        &mut self,
        name: impl Into<String>,
        detail: impl Into<String>,
        section: impl Into<String>,
    ) {
        self.record(Level::Fail, name, detail, section);
    }
    pub fn warnings(&self) -> usize {
        self.results
            .iter()
            .filter(|row| row.level == Level::Warn)
            .count()
    }
    pub fn failures(&self) -> usize {
        self.results
            .iter()
            .filter(|row| row.level == Level::Fail)
            .count()
    }
    pub fn exit_code(&self) -> i32 {
        self.exit_code_with_strict(true)
    }
    /// Python's smoke check treats warnings as advisory unless `--strict` is requested.
    pub fn exit_code_with_strict(&self, strict: bool) -> i32 {
        if self.failures() > 0 {
            2
        } else if self.warnings() > 0 && strict {
            1
        } else {
            0
        }
    }
}

pub fn path_check(
    path: impl AsRef<Path>,
    label: impl Into<String>,
    executable: bool,
    absent: bool,
    section: impl Into<String>,
) -> ResultRow {
    let path = path.as_ref();
    let exists = path.exists();
    let executable_ok = !executable || is_executable(path);
    let (level, detail) = if absent {
        if exists {
            (Level::Fail, format!("{} should not exist", path.display()))
        } else {
            (Level::Pass, format!("{} absent", path.display()))
        }
    } else if exists && executable_ok {
        (Level::Pass, path.display().to_string())
    } else {
        let suffix = if executable {
            " missing or not executable"
        } else {
            " missing"
        };
        (Level::Fail, format!("{}{suffix}", path.display()))
    };
    ResultRow {
        level,
        name: label.into(),
        detail,
        section: section.into(),
    }
}

pub fn contains_check(
    path: impl AsRef<Path>,
    needle: &str,
    label: impl Into<String>,
    detail: impl Into<String>,
    negate: bool,
    section: impl Into<String>,
) -> ResultRow {
    let path = path.as_ref();
    let found = std::fs::read_to_string(path)
        .map(|text| text.contains(needle))
        .unwrap_or(false);
    let good = if negate { !found } else { found };
    let detail = if good {
        detail.into()
    } else if negate {
        format!("{} contains an unwanted entry", path.display())
    } else {
        format!("{} does not contain expected entry", path.display())
    };
    ResultRow {
        level: if good { Level::Pass } else { Level::Fail },
        name: label.into(),
        detail,
        section: section.into(),
    }
}

pub fn command_available(
    command: &str,
    path: impl AsRef<Path>,
    label: impl Into<String>,
    optional: bool,
    section: impl Into<String>,
) -> ResultRow {
    let command_path = path.as_ref().join(command);
    let available = command_path.is_file() && is_executable(&command_path);
    let (level, detail) = if available {
        (Level::Pass, command_path.display().to_string())
    } else if optional {
        (
            Level::Warn,
            "not installed or unavailable on this image".into(),
        )
    } else {
        (Level::Fail, "missing command".into())
    };
    ResultRow {
        level,
        name: label.into(),
        detail,
        section: section.into(),
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn checks_paths_and_expected_content_without_invoking_commands() {
        let directory = tempdir().unwrap();
        let config = directory.path().join("config");
        fs::write(&config, "Theme=kyth\n").unwrap();
        assert_eq!(
            path_check(&config, "Config", false, false, "Desktop").level,
            Level::Pass
        );
        assert_eq!(
            contains_check(&config, "Theme=kyth", "Theme", "selected", false, "Desktop").level,
            Level::Pass
        );
        assert_eq!(
            contains_check(&config, "Fedora", "Branding", "selected", false, "Desktop").level,
            Level::Fail
        );
    }

    #[test]
    fn aggregates_levels_and_optional_commands() {
        let directory = tempdir().unwrap();
        let mut report = Report::default();
        let row = command_available("missing", directory.path(), "Optional", true, "Tools");
        report.record(row.level, row.name, row.detail, row.section);
        assert_eq!(report.warnings(), 1);
        assert_eq!(report.exit_code(), 1);
        assert_eq!(report.exit_code_with_strict(false), 0);
        assert_eq!(
            path_check(
                directory.path().join("missing"),
                "Required",
                false,
                false,
                "Tools"
            )
            .level,
            Level::Fail
        );
    }
}
