//! Declarative gaming-slice configuration and systemd drop-in rendering.
//!
//! This ports the offline policy layer from cgroup_slice.py. Writing the
//! system drop-in and activating the slice remain explicit caller actions.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_CONFIG_PATH: &str = "/etc/kyth/gaming-slice.toml";
pub const DEFAULT_SLICE_PATH: &str = "/etc/systemd/system/gaming.slice.d/50-kyth.conf";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GamingCgroupConfig {
    pub cpu_weight: i64,
    pub memory_max: String,
    pub io_weight: i64,
    pub allowed_cpus: String,
}

impl Default for GamingCgroupConfig {
    fn default() -> Self {
        Self {
            cpu_weight: 300,
            memory_max: "80%".into(),
            io_weight: 200,
            allowed_cpus: String::new(),
        }
    }
}

impl GamingCgroupConfig {
    pub fn normalized(mut self) -> Self {
        self.cpu_weight = self.cpu_weight.clamp(1, 1000);
        self.io_weight = self.io_weight.clamp(1, 1000);
        self
    }

    pub fn load(path: impl AsRef<Path>) -> Self {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let Ok(value) = raw.parse::<toml::Value>() else {
            return Self::default();
        };
        Self {
            cpu_weight: value
                .get("cpu_weight")
                .and_then(toml::Value::as_integer)
                .unwrap_or(300),
            memory_max: value
                .get("memory_max")
                .and_then(toml::Value::as_str)
                .unwrap_or("80%")
                .into(),
            io_weight: value
                .get("io_weight")
                .and_then(toml::Value::as_integer)
                .unwrap_or(200),
            allowed_cpus: value
                .get("allowed_cpus")
                .and_then(toml::Value::as_str)
                .unwrap_or("")
                .into(),
        }
        .normalized()
    }

    pub fn to_toml(&self) -> String {
        let config = self.clone().normalized();
        format!(
            "# Kyth cgroup gaming slice — declarative, offline\n\
             cpu_weight = {}\n\
             memory_max = {:?}\n\
             io_weight = {}\n\
             allowed_cpus = {:?}\n",
            config.cpu_weight, config.memory_max, config.io_weight, config.allowed_cpus
        )
    }

    pub fn render_drop_in(&self) -> String {
        let config = self.clone().normalized();
        let mut lines = vec![
            "[Slice]".to_string(),
            format!("CPUWeight={}", config.cpu_weight),
            format!("MemoryMax={}", config.memory_max),
            format!("IOWeight={}", config.io_weight),
        ];
        if !config.allowed_cpus.is_empty() {
            lines.push(format!("AllowedCPUs={}", config.allowed_cpus));
        }
        lines.push("CPUAccounting=yes".into());
        lines.push("MemoryAccounting=yes".into());
        format!("{}\n", lines.join("\n"))
    }
}

pub fn config_path(path: Option<impl AsRef<Path>>) -> PathBuf {
    path.map_or_else(
        || PathBuf::from(DEFAULT_CONFIG_PATH),
        |path| path.as_ref().to_path_buf(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn defaults_and_clamps_match_python_policy() {
        let config = GamingCgroupConfig {
            cpu_weight: 5000,
            memory_max: "75%".into(),
            io_weight: 0,
            allowed_cpus: "0-7".into(),
        }
        .normalized();
        assert_eq!(config.cpu_weight, 1000);
        assert_eq!(config.io_weight, 1);
        assert_eq!(config.memory_max, "75%");
    }

    #[test]
    fn loads_and_renders_slice_drop_in() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("gaming-slice.toml");
        std::fs::write(
            &path,
            "cpu_weight = 400\nmemory_max = \"80%\"\nio_weight = 250\nallowed_cpus = \"2-5\"\n",
        )
        .unwrap();
        let config = GamingCgroupConfig::load(&path);
        let drop_in = config.render_drop_in();
        assert!(drop_in.contains("CPUWeight=400"));
        assert!(drop_in.contains("AllowedCPUs=2-5"));
        assert!(config.to_toml().contains("memory_max = \"80%\""));
    }

    #[test]
    fn omits_empty_cpu_affinity() {
        let drop_in = GamingCgroupConfig::default().render_drop_in();
        assert!(!drop_in.contains("AllowedCPUs="));
        assert!(drop_in.ends_with("MemoryAccounting=yes\n"));
    }
}
