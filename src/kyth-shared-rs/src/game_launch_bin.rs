//! Native replacement for the Python `kyth-game-launch` launcher.
//!
//! Parses `--no-gamemode`/`-p`/`--`, marks the gaming hint, ensures the
//! scheduler arbiter, wraps with gamemoderun when present, and prefers a
//! `gaming.slice` systemd scope before falling back to a direct exec.
//! Exit `127` when nothing executes, `1` for status mode with arguments.
//! `sched_arbiter.py` stays as the Phase 3 fixture.

use std::env;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::time::Duration;

use kyth_shared::system::process::run_bounded;
use kyth_shared::system::scheduler_arbiter::{current_desired_state, generate_arbiter};

fn on_path(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

fn parse_args(args: &[String]) -> (bool, Vec<String>) {
    let mut use_gamemode = true;
    let mut profile = "gaming".to_string();
    let mut cmd_start = 0;
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--no-gamemode" {
            use_gamemode = false;
            index += 1;
        } else if (args[index] == "-p" || args[index] == "--profile") && index + 1 < args.len() {
            profile = args[index + 1].clone();
            index += 2;
        } else if args[index] == "--" {
            cmd_start = index + 1;
            break;
        } else if args[index].starts_with('-') {
            index += 1;
        } else {
            cmd_start = index;
            break;
        }
    }
    let _ = profile;
    let cmd = if cmd_start < args.len() {
        args[cmd_start..].to_vec()
    } else {
        Vec::new()
    };
    (use_gamemode, cmd)
}

fn mark_gaming_hint() {
    if std::fs::create_dir_all("/run/kyth").is_err() {
        return;
    }
    if std::fs::write("/run/kyth/gaming-hint", "1").is_err() {
        return;
    }
    if on_path("kyth-readahead-hint") {
        let _ = run_bounded(
            &["kyth-readahead-hint".into(), "apply".into()],
            Duration::from_secs(2),
        );
    }
}

fn exec_argv(argv: &[String]) -> std::io::Error {
    let (program, args) = argv.split_first().expect("exec argv is never empty");
    std::process::Command::new(program).args(args).exec()
}

fn launch(args: &[String]) -> i32 {
    let (use_gamemode, cmd) = parse_args(args);
    if cmd.is_empty() {
        let state = current_desired_state();
        if let Ok(rendered) = serde_json::to_string_pretty(&state.as_value()) {
            println!("{rendered}");
        }
        println!("Usage: kyth-game-launch [--no-gamemode] [-p gaming] -- <cmd>");
        return if args.is_empty() { 0 } else { 1 };
    }
    mark_gaming_hint();
    let _ = generate_arbiter();
    let mut run_cmd = cmd;
    if use_gamemode && on_path("gamemoderun") {
        run_cmd.insert(0, "gamemoderun".to_string());
    }
    if on_path("systemd-run") {
        let user_scope =
            unsafe { libc::geteuid() } != 0 && Path::new("/run/systemd/system").exists();
        let mut scoped = if user_scope {
            vec![
                "systemd-run".to_string(),
                "--user".to_string(),
                "--scope".to_string(),
                "--slice=gaming.slice".to_string(),
            ]
        } else {
            vec![
                "systemd-run".to_string(),
                "--scope".to_string(),
                "--slice=gaming.slice".to_string(),
            ]
        };
        scoped.push("--".to_string());
        scoped.extend(run_cmd.clone());
        let _ = exec_argv(&scoped);
    }
    let error = exec_argv(&run_cmd);
    eprintln!("exec failed: {error}");
    let _ = std::fs::remove_file("/run/kyth/gaming-hint");
    127
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    std::process::ExitCode::from(launch(&args) as u8)
}
