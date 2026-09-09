//! Native first-login default-app status writer.

use std::time::Duration;

fn run(program: &str, args: &[&str], timeout: u64) -> Option<(bool, String)> {
    let argv = std::iter::once(program.to_string())
        .chain(args.iter().map(|arg| (*arg).to_string()))
        .collect::<Vec<_>>();
    let output =
        kyth_shared::system::process::run_bounded(&argv, Duration::from_secs(timeout)).ok()?;
    Some((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
    ))
}

fn command_exists(program: &str) -> bool {
    let directories = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    directories
        .into_iter()
        .any(|dir| dir.join(program).is_file())
}

fn mark_run(path: &std::path::Path) {
    let _ = kyth_shared::atomic_io::atomic_write_text(path, "", Some(0o644));
}

fn main() -> std::process::ExitCode {
    if kyth_shared::system::process::is_live_session() {
        return std::process::ExitCode::SUCCESS;
    }
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let force = args.iter().any(|arg| arg == "--force");
    let delay = std::env::var("KYTH_APP_STATUS_DELAY")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(20);
    let notify_ready = std::env::var("KYTH_APP_STATUS_NOTIFY_READY")
        .ok()
        .as_deref()
        == Some("1");
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/root".into());
    let state_dir = home.join(".local/share/kyth");
    let status_file = state_dir.join("first-run-apps.status");
    let marker = state_dir.join("firstboot-app-status-v1");
    let done = kyth_shared::system::firstboot::default_flatpaks_done("/var/lib/kyth");
    if !force && marker.is_file() && done {
        return std::process::ExitCode::SUCCESS;
    }
    if delay > 0 {
        std::thread::sleep(Duration::from_secs(delay.min(300)));
    }

    if !command_exists("flatpak") {
        let _ = kyth_shared::system::firstboot::write_app_status(
            &status_file,
            "needs_attention",
            "Flatpak is not available. Open Hub > This PC > Repair for details.",
            &format!("{}", unix_now()),
        );
        return std::process::ExitCode::from(2);
    }
    let apps = [
        "com.valvesoftware.Steam",
        "net.lutris.Lutris",
        "com.heroicgameslauncher.hgl",
        "com.usebottles.bottles",
        "com.github.mtkennerly.ludusavi",
    ];
    let missing = apps
        .iter()
        .filter(|app| !run("flatpak", &["info", app], 20).is_some_and(|(ok, _)| ok))
        .count();
    let updated = format!("{}", unix_now());
    if missing == 0 {
        let message = "Steam, launchers, Bottles, and save backup tools are installed.";
        let _ = kyth_shared::system::firstboot::write_app_status(
            &status_file,
            "ready",
            message,
            &updated,
        );
        if force || notify_ready {
            let _ = run(
                "notify-send",
                &["--app-name=KythOS", "KythOS apps are ready", message],
                5,
            );
        }
        mark_run(&marker);
        return std::process::ExitCode::SUCCESS;
    }

    let service = run(
        "systemctl",
        &["is-active", "kyth-default-flatpaks.service"],
        5,
    )
    .map(|(_, output)| output)
    .unwrap_or_default();
    let (state, message) = if matches!(service.trim(), "active" | "activating") {
        (
            "setting_up",
            "Game launchers and migration tools are installing in the background.",
        )
    } else if service.trim() == "failed" {
        (
            "failed",
            "Some default apps are missing. Open Hub > This PC > Repair and retry Game Apps.",
        )
    } else if run(
        "sudo",
        &["-n", "systemctl", "start", "kyth-default-flatpaks.service"],
        10,
    )
    .is_some_and(|(ok, _)| ok)
    {
        (
            "setting_up",
            "Game launchers and migration tools have started installing in the background.",
        )
    } else {
        ("needs_attention", "Some default apps are missing. Connect to the network, then open Hub > This PC > Repair and retry Game Apps.")
    };
    let _ =
        kyth_shared::system::firstboot::write_app_status(&status_file, state, message, &updated);
    std::process::ExitCode::SUCCESS
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
