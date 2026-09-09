//! Native replacement for the Python `kyth-cloud-mount` launcher.
//!
//! Ensures `~/Cloud/<name>` exists per `cloud.toml` drive and starts the
//! matching `rclone@<name>.service` user unit best-effort (systemd owns the
//! real mount). Always exits `0`. `cloud_preset.py` stays as the Phase 3
//! fixture.

use std::env;
use std::path::PathBuf;

use kyth_shared::system::network_services::{cloud_path, load_cloud};

fn main() -> std::process::ExitCode {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let drives = load_cloud(cloud_path(None::<PathBuf>));
    for (name, remote) in &drives {
        if remote.is_empty() {
            continue;
        }
        let mount = home.join("Cloud").join(name);
        let _ = std::fs::create_dir_all(&mount);
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "start", &format!("rclone@{name}.service")])
            .output();
        println!(
            "kyth-cloud-mount: {name} \u{2192} {remote} \u{2192} {}",
            mount.display()
        );
    }
    std::process::ExitCode::SUCCESS
}
