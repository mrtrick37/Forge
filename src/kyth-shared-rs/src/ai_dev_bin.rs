//! Native replacement for the kyth-ai-dev Python console entry point.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use kyth_shared::system::ai_dev::{
    self, box_exists, create_command_for_host, gpu_description, gpu_kind, inside_command,
    ollama_pull_command, ollama_start_command, provision_command, remount_paths, run_owned,
    validate_model, Config, COMMAND_TIMEOUT, DEFAULT_MODEL, PROVISION_TIMEOUT,
};
use kyth_shared::system::process::redact_sensitive_text;

fn config() -> Config {
    let environment = env::vars().collect::<BTreeMap<_, _>>();
    Config::from_environment(&environment)
}

fn print_output(output: &std::process::Output) {
    let stdout = redact_sensitive_text(&String::from_utf8_lossy(&output.stdout));
    let stderr = redact_sensitive_text(&String::from_utf8_lossy(&output.stderr));
    if !stdout.is_empty() {
        print!("{stdout}");
    }
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }
}

fn run_and_print(argv: &[String], timeout: std::time::Duration) -> io::Result<bool> {
    let output = run_owned(argv, timeout)?;
    print_output(&output);
    Ok(output.status.success())
}

fn require_distrobox() -> io::Result<()> {
    if !ai_dev::host_command_exists("distrobox") {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "distrobox is not installed.",
        ));
    }
    Ok(())
}

fn setup(app: &Config) -> io::Result<bool> {
    require_distrobox()?;
    fs::create_dir_all(&app.model_dir)?;
    if !box_exists(app)? {
        println!("Creating {} from {}...", app.box_name, app.image);
        let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        if !run_and_print(
            &create_command_for_host(app, &home, gpu_kind()),
            COMMAND_TIMEOUT,
        )? {
            return Ok(false);
        }
    } else {
        println!("{} already exists.", app.box_name);
    }
    println!("Installing developer & AI tools in {}...", app.box_name);
    if !run_and_print(&provision_command(app), PROVISION_TIMEOUT)? {
        return Ok(false);
    }
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    remount_paths(&ai_dev::host_git_paths(&home));
    println!("\nDeveloper & AI environment ({}) is ready.", app.box_name);
    println!("GPU Acceleration: {}", gpu_description(gpu_kind()));
    println!(
        "Enter environment manually with: distrobox enter {}",
        app.box_name
    );
    Ok(true)
}

fn status(app: &Config) -> io::Result<bool> {
    require_distrobox()?;
    println!("KythOS AI developer environment");
    println!(
        "Box: {}\nImage: {}\nModels: {}",
        app.box_name,
        app.image,
        app.model_dir.display()
    );
    if !box_exists(app)? {
        println!("Box: missing");
        println!("Run: ujust ai-dev-setup or kyth-ai-dev setup");
        return Ok(true);
    }
    println!("Box: present");
    let labels = [
        ("code", "VS Code"),
        ("node", "Node.js"),
        ("headroom", "Headroom CLI"),
        ("rtk", "RTK CLI"),
        ("hx", "Helix Editor"),
        ("zellij", "Zellij Multiplexer"),
        ("gh", "GitHub CLI"),
        ("flatpak-builder", "Flatpak Builder"),
        ("rclone", "RClone Cloud Sync"),
        ("duperemove", "Btrfs Deduplicator"),
        ("trivy", "Trivy Scanner"),
        ("zizmor", "Zizmor Workflow Scanner"),
        ("claude", "Claude Code"),
        ("codex", "Codex CLI"),
        ("ollama", "Ollama"),
    ];
    for (command, label) in labels {
        let check = inside_command(
            app,
            &[
                "bash".into(),
                "-lc".into(),
                format!("command -v {command} >/dev/null 2>&1"),
            ],
        );
        let installed = run_and_print(&check, COMMAND_TIMEOUT).unwrap_or(false);
        let suffix = if command == "node" && installed {
            let version = run_owned(
                &inside_command(app, &["node".into(), "--version".into()]),
                COMMAND_TIMEOUT,
            )
            .ok()
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "unknown".into());
            format!(" ({version})")
        } else {
            String::new()
        };
        println!(
            "{label}: {}{suffix}",
            if installed {
                "installed"
            } else {
                "not installed"
            }
        );
    }
    println!("GPU Acceleration: {}", gpu_description(gpu_kind()));
    Ok(true)
}

fn start(app: &Config) -> io::Result<bool> {
    require_distrobox()?;
    if !box_exists(app)? {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} does not exist. Run: ujust ai-dev-setup", app.box_name),
        ));
    }
    let check = inside_command(
        app,
        &[
            "bash".into(),
            "-lc".into(),
            "command -v ollama >/dev/null 2>&1".into(),
        ],
    );
    if !run_and_print(&check, COMMAND_TIMEOUT)? {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "ollama is not installed.",
        ));
    }
    let ok = run_and_print(&ollama_start_command(app), COMMAND_TIMEOUT)?;
    if ok {
        println!(
            "Started Ollama inside {} (Models in {}).",
            app.box_name,
            app.model_dir.display()
        );
    }
    Ok(ok)
}

fn pull_model(app: &Config, model: &str) -> io::Result<bool> {
    validate_model(model)?;
    require_distrobox()?;
    if !box_exists(app)? {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} does not exist. Run: ujust ai-dev-setup", app.box_name),
        ));
    }
    let running = run_and_print(
        &inside_command(app, &["pgrep".into(), "-x".into(), "ollama".into()]),
        COMMAND_TIMEOUT,
    )?;
    if !running {
        println!("Starting background Ollama server before pulling model...");
        if !start(app)? {
            return Ok(false);
        }
        thread::sleep(Duration::from_secs(2));
    }
    println!(
        "Pulling AI model '{model}' into {} ({}).",
        app.box_name,
        app.model_dir.display()
    );
    run_and_print(&ollama_pull_command(app, model), PROVISION_TIMEOUT)
}

fn usage() {
    eprintln!("usage: kyth-ai-dev <setup|status|enter|remove|start|stop|pull-model [model]>");
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let action = args.next().unwrap_or_default();
    let app = config();
    let result = match action.as_str() {
        "setup" => setup(&app),
        "status" => status(&app),
        "enter" => require_distrobox().and_then(|_| {
            let status = std::process::Command::new("distrobox")
                .args(["enter", &app.box_name])
                .status()?;
            Ok(status.success())
        }),
        "start" => start(&app),
        "stop" => (|| -> io::Result<bool> {
            require_distrobox()?;
            if box_exists(&app)? {
                run_and_print(
                    &inside_command(
                        &app,
                        &[
                            "bash".into(),
                            "-lc".into(),
                            "pkill -x ollama 2>/dev/null || true".into(),
                        ],
                    ),
                    COMMAND_TIMEOUT,
                )?;
            }
            println!("Stopped Ollama in {}.", app.box_name);
            Ok(true)
        })(),
        "remove" => (|| -> io::Result<bool> {
            require_distrobox()?;
            let ok = run_and_print(
                &[
                    "distrobox".into(),
                    "rm".into(),
                    "--force".into(),
                    app.box_name.clone(),
                ],
                COMMAND_TIMEOUT,
            )?;
            if ok {
                println!(
                    "Removed {}.\nModels were left in: {}.",
                    app.box_name,
                    app.model_dir.display()
                );
            }
            Ok(ok)
        })(),
        "pull-model" => pull_model(&app, &args.next().unwrap_or_else(|| DEFAULT_MODEL.into())),
        _ => {
            usage();
            Ok(false)
        }
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            eprintln!("ERROR: {error}");
            ExitCode::from(1)
        }
    }
}
