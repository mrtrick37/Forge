//! Pure performance helpers shared by native UI and tuning adapters.

/// Read the first vendor/model pair from `/proc/cpuinfo`-shaped text.
pub fn cpu_topology(text: &str) -> (String, String) {
    let mut vendor = "Unknown".to_string();
    let mut model = "Generic CPU".to_string();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("vendor_id") {
            vendor = value
                .split_once(':')
                .map_or(vendor.clone(), |(_, value)| value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("model name") {
            model = value
                .split_once(':')
                .map_or(model.clone(), |(_, value)| value.trim().to_string());
        }
    }
    (vendor, model)
}

pub fn has_3d_vcache(text: &str) -> bool {
    text.to_ascii_lowercase().contains("3d")
}

pub fn epp_value(text: Option<&str>) -> String {
    text.map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("n/a")
        .to_string()
}

pub fn gamescope_command(target: &[String], available: bool) -> Vec<String> {
    if !available {
        return target.to_vec();
    }
    let mut command = vec![
        "gamescope".into(),
        "-f".into(),
        "-e".into(),
        "--rt".into(),
        "--".into(),
    ];
    command.extend_from_slice(target);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cpuinfo_and_vcache() {
        assert_eq!(
            cpu_topology("vendor_id : AuthenticAMD\nmodel name : Ryzen 7 7800X3D\n"),
            ("AuthenticAMD".into(), "Ryzen 7 7800X3D".into())
        );
        assert!(has_3d_vcache("AMD Ryzen 7 7800X3D"));
        assert_eq!(cpu_topology(""), ("Unknown".into(), "Generic CPU".into()));
    }

    #[test]
    fn wraps_gamescope_only_when_available() {
        let target = vec!["game".into(), "--fullscreen".into()];
        assert_eq!(gamescope_command(&target, false), target);
        assert_eq!(gamescope_command(&target, true)[0], "gamescope");
    }
}
