//! Native MIME-handler launcher for Windows installers and RPM packages.
//!
//! The user interface lives in the existing Kyth Tauri/React Hub.  This
//! narrow executable exists so the desktop-file entry point remains native
//! while Dolphin and other XDG callers can pass a file path directly.

use std::env;
use std::process::{Command, ExitCode};

const HUB_SHELL: &str = "/usr/bin/kyth-hub-shell";

fn handler_path(args: impl IntoIterator<Item = String>) -> Option<String> {
    let mut path = None;
    for arg in args.into_iter().skip(1) {
        // Retain the legacy flag as a harmless compatibility spelling. The
        // Hub always presents a dialog, so no separate mode is needed.
        if arg != "--dialog" && path.is_none() {
            path = Some(arg);
        }
    }
    path
}

fn main() -> ExitCode {
    let Some(path) = handler_path(env::args()) else {
        return ExitCode::SUCCESS;
    };
    match Command::new(HUB_SHELL)
        .arg("--exe-handler")
        .arg(path)
        .status()
    {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => ExitCode::from(status.code().unwrap_or(1).clamp(1, 255) as u8),
        Err(error) => {
            eprintln!("kyth-exe-handler: could not start Kyth Hub: {error}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::handler_path;

    #[test]
    fn accepts_legacy_dialog_flag_without_treating_it_as_a_path() {
        assert_eq!(
            handler_path(["kyth-exe-handler", "--dialog", "/tmp/setup.exe"].map(String::from)),
            Some("/tmp/setup.exe".into())
        );
    }

    #[test]
    fn no_file_is_a_successful_no_op() {
        assert_eq!(handler_path(["kyth-exe-handler"].map(String::from)), None);
    }
}
