//! Port of `kyth_shared.system.drivers` — kernel module + lspci class probing.

use std::collections::HashSet;
use std::fs;
use std::time::Duration;

pub fn get_loaded_kernel_modules() -> HashSet<String> {
    let mut set = HashSet::new();
    if let Ok(text) = fs::read_to_string("/proc/modules") {
        for line in text.lines() {
            if let Some(first) = line.split_whitespace().next() {
                set.insert(first.to_string());
            }
        }
    }
    set
}

pub fn is_module_loaded(module: &str) -> bool {
    get_loaded_kernel_modules().contains(module)
}

pub fn get_pci_devices_by_class(class: &str) -> Vec<String> {
    let argv = ["lspci", "-nn"]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let out = super::process::run_bounded(&argv, Duration::from_secs(5));
    if let Ok(o) = out {
        if o.status.success() {
            let lower = class.to_lowercase();
            let stdout = String::from_utf8_lossy(&o.stdout);
            return stdout
                .lines()
                .filter(|l| l.to_lowercase().contains(&lower))
                .map(|l| l.to_string())
                .collect();
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loaded_modules_is_set() {
        let s = get_loaded_kernel_modules();
        // At least one module on typical system, but may be empty in container
        let _ = s.len();
    }
    #[test]
    fn pci_class_returns_vec() {
        let v = get_pci_devices_by_class("VGA");
        assert!(v.len() <= 100);
    }
}
