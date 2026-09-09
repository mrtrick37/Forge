//! Read-only gaming master profile and safety decision.

use super::tuning_profile::{config_path, load_profile, Profile};
use std::path::{Path, PathBuf};

pub fn master_config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    config_path(
        path,
        "/etc/kyth/gaming-performance.toml",
        "gaming-performance.toml",
    )
}

pub fn load_master(path: impl AsRef<Path>) -> Profile {
    load_profile(path)
}

pub fn save_master(path: impl AsRef<Path>, profile: Profile) -> std::io::Result<()> {
    super::tuning_profile::save_profile(path, "Kyth master gaming performance", profile)
}

pub fn thermal_high(root: impl AsRef<Path>, threshold_c: i64) -> bool {
    let Ok(entries) = root.as_ref().read_dir() else {
        return false;
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("thermal_zone")
        })
        .any(|entry| {
            std::fs::read_to_string(entry.path().join("temp"))
                .ok()
                .and_then(|value| value.trim().parse::<i64>().ok())
                .is_some_and(|temp| temp > threshold_c * 1000)
        })
}

pub fn battery_low(root: impl AsRef<Path>, threshold_pct: i64) -> bool {
    let Ok(entries) = root.as_ref().read_dir() else {
        return false;
    };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("BAT"))
        .any(|entry| {
            let capacity = std::fs::read_to_string(entry.path().join("capacity"))
                .ok()
                .and_then(|value| value.trim().parse::<i64>().ok());
            let status = std::fs::read_to_string(entry.path().join("status"))
                .ok()
                .map(|value| value.trim().to_ascii_lowercase());
            capacity.is_some_and(|capacity| capacity < threshold_pct)
                && status.is_some_and(|status| status == "discharging" || status == "not charging")
        })
}

pub fn effective_gaming(profile: Profile, thermal: bool, battery: bool) -> (Profile, &'static str) {
    if profile != Profile::Gaming {
        return (Profile::Balanced, "balanced profile selected");
    }
    if thermal {
        return (Profile::Balanced, "thermal limit reached");
    }
    if battery {
        return (Profile::Balanced, "battery is low while discharging");
    }
    (Profile::Gaming, "gaming profile ready")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn honors_thermal_and_battery_safety_gates() {
        let directory = tempdir().unwrap();
        let thermal = directory.path().join("thermal_zone0");
        let battery = directory.path().join("BAT0");
        fs::create_dir(&thermal).unwrap();
        fs::create_dir(&battery).unwrap();
        fs::write(thermal.join("temp"), "90000\n").unwrap();
        fs::write(battery.join("capacity"), "20\n").unwrap();
        fs::write(battery.join("status"), "Discharging\n").unwrap();
        assert!(thermal_high(directory.path(), 85));
        assert!(battery_low(directory.path(), 30));
        assert_eq!(
            effective_gaming(Profile::Gaming, true, false).0,
            Profile::Balanced
        );
        assert_eq!(
            effective_gaming(Profile::Gaming, false, false).0,
            Profile::Gaming
        );
    }

    #[test]
    fn saves_and_loads_master_profile() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("gaming-performance.toml");
        save_master(&path, Profile::Gaming).unwrap();
        assert_eq!(load_master(&path), Profile::Gaming);
    }
}
