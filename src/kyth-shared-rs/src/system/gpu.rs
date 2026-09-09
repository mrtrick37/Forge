//! Port of `kyth_shared.system.gpu`'s `lspci_gpu_lines()` — one
//! `lspci -nn` call, filtered to display-controller lines. Same plain
//! substring match as the Python original (`"vga"`/`"3d"`/`"display"`
//! anywhere in the lowercased line, not word-boundary — a line like
//! "Non-VGA unclassified device" does match on `"vga"` in both this and
//! the Python original; ported as-is, not "fixed", since changing that
//! behavior isn't this port's job).

use std::time::Duration;

pub fn lspci_gpu_lines() -> Vec<String> {
    let argv = ["lspci", "-nn"]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let Ok(output) = super::process::run_bounded(&argv, Duration::from_secs(5)) else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("vga") || lower.contains("3d") || lower.contains("display")
        })
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // No lspci-availability assertion here — whether it's installed is a
    // property of the machine running the tests, not this function.
    // Correctness of the filter itself is exercised at the bridge-command
    // layer (see src-tauri's tests) where a fake `lspci` on PATH is cheap
    // to set up; this crate has no subprocess-injection seam of its own.
    #[test]
    fn does_not_panic_when_lspci_is_missing() {
        let _ = lspci_gpu_lines();
    }
}
