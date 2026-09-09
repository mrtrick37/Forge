//! Native replacement for the Python `kyth-setup-transfer` launcher.
//!
//! Supports `export <destination>`, `summary <archive>`, and `restore
//! <archive>` with the same messages and exit codes (`2` for usage errors,
//! `1` with an `ERROR:` line for operational failures). `setup_transfer.py`
//! stays as the Phase 3 fixture.

use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kyth_shared::setup_transfer::{
    archive_summary, export_setup, restore_setup, stream_command, SetupCtx,
};
use kyth_shared::system::issue_draft::local_timestamp;
use kyth_shared::system::process::run_bounded;
use kyth_shared::system::session_snapshot::current_host;

fn find_binary(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

fn expand_user(value: &str, home: &Path) -> PathBuf {
    if value == "~" {
        home.to_path_buf()
    } else if let Some(rest) = value.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(value)
    }
}

fn usage() -> ! {
    eprintln!(
        "Usage: kyth-setup-transfer {{export <destination>|summary <archive>|restore <archive>}}"
    );
    std::process::exit(2);
}

fn run() -> Result<i32, String> {
    let mut args = env::args().skip(1);
    let (command, operand) = match (args.next(), args.next(), args.next()) {
        (Some(command), Some(operand), None)
            if ["export", "summary", "restore"].contains(&command.as_str()) =>
        {
            (command, operand)
        }
        _ => usage(),
    };
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"));
    let ctx = SetupCtx {
        home: &home,
        run_text: &|argv, secs| {
            run_bounded(argv, Duration::from_secs(secs))
                .ok()
                .map(|output| {
                    (
                        output.status.code().unwrap_or(1),
                        String::from_utf8_lossy(&output.stdout).into_owned(),
                    )
                })
        },
        stamp: &local_timestamp,
        iso_now: &kyth_shared::setup_transfer::now_iso_seconds,
        hostname: &current_host,
        flatpak_present: find_binary("flatpak"),
    };
    let operand = expand_user(&operand, &home);
    match command.as_str() {
        "export" => {
            let archive = export_setup(&ctx, &operand)?;
            println!("Setup archive created: {}", archive.display());
            println!(
                "Passwords, browser sessions, SMB credentials, and cloud OAuth tokens were not included."
            );
        }
        "summary" => {
            println!("{}", archive_summary(&ctx, &operand)?);
        }
        _ => {
            let print_line = |line: &str| println!("{line}");
            let report = restore_setup(
                &ctx,
                &|args, on_line| stream_command(args, 600, on_line),
                &print_line,
                &operand,
            )?;
            println!(
                "Setup restored: {} settings paths, {} default app associations, {} apps installed or updated.",
                report.paths, report.defaults, report.apps_ok
            );
            if report.apps_failed > 0 {
                println!(
                    "{} app install(s) failed; retry them from Discover Apps.",
                    report.apps_failed
                );
            }
            if !report.cloud_names.is_empty() {
                println!(
                    "Reconnect cloud account(s) in Cloud Storage: {}",
                    report.cloud_names.join(", ")
                );
            }
            if report.dynamic_lock {
                println!("Trusted-device Dynamic Lock restored.");
            }
            println!(
                "Re-enter network-share passwords from Network Shares, then sign out and back in."
            );
        }
    }
    Ok(0)
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(code) => std::process::ExitCode::from(code as u8),
        Err(error) => {
            eprintln!("ERROR: {error}");
            std::process::ExitCode::from(1)
        }
    }
}
