//! Native replacement for the Python `kyth-vscode-wallet` launcher.
//!
//! Enables KWallet integration for Chromium/Electron apps (VS Code and
//! Brave) under `$HOME` and prints the confirmation line. `session.py`
//! stays as the Phase 3 fixture.

use std::env;
use std::path::PathBuf;

use kyth_shared::system::session_config::enable_vscode_brave_wallet_prompts;
use kyth_shared::system::session_snapshot::current_user;

fn main() -> std::process::ExitCode {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    enable_vscode_brave_wallet_prompts(&home);
    let user = current_user();
    // `getpass.getuser() or "this user"`: only an unresolvable account hits
    // the fallback, where the Rust helper reports "unknown".
    let user = if user == "unknown" {
        "this user".to_string()
    } else {
        user
    };
    println!("VS Code and Brave KWallet integration enabled for {user}.");
    std::process::ExitCode::SUCCESS
}
