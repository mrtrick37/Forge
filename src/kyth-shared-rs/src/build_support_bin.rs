//! Native build-time support boundary for shared-package helpers.
//!
//! Package assembly may still be orchestrated by shell, but repository
//! rendering, container-wrapper generation, gaming metadata projection, and
//! COPR cleanup are owned here so build fragments do not execute Python
//! package code or duplicate policy.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use kyth_shared::atomic_io::atomic_write_text;
use kyth_shared::containers::render_distrobox_wrapper;
use kyth_shared::repos::{load_repo_specs, GAMING_COPRS};
use kyth_shared::system::gaming_versions::GamingVersions;

fn usage() {
    eprintln!(
        "usage: kyth-build-support <gaming-label|gaming-coprs|disable-gaming-coprs|repo-render|container-wrapper> [options]"
    );
}

fn option(args: &[String], name: &str) -> Result<String, String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
        .ok_or_else(|| format!("missing required option {name}"))
}

fn default_repo_config() -> PathBuf {
    [
        PathBuf::from("/ctx/config/repos.json"),
        PathBuf::from("build_files/config/repos.json"),
        PathBuf::from("config/repos.json"),
        PathBuf::from("/usr/share/kyth/config/repos.json"),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .unwrap_or_else(|| PathBuf::from("build_files/config/repos.json"))
}

fn render_repo(args: &[String]) -> Result<(), String> {
    let name = option(args, "--name")?;
    let output = PathBuf::from(option(args, "--output")?);
    let config = PathBuf::from(
        args.windows(2)
            .find(|pair| pair[0] == "--config")
            .map(|pair| pair[1].clone())
            .unwrap_or_else(|| default_repo_config().display().to_string()),
    );
    let specs = load_repo_specs(&config)?;
    let spec = specs
        .iter()
        .find(|spec| spec.name == name)
        .ok_or_else(|| format!("repository {name:?} is not defined in {}", config.display()))?;
    atomic_write_text(output, &spec.render_yum_repo(), Some(0o644))
        .map_err(|error| error.to_string())
}

fn render_container(args: &[String]) -> Result<(), String> {
    let tool = option(args, "--tool")?;
    let description = option(args, "--description")?;
    let output = PathBuf::from(option(args, "--output")?);
    let box_name = args
        .windows(2)
        .find(|pair| pair[0] == "--box")
        .map(|pair| pair[1].as_str())
        .unwrap_or("kyth-ai-dev");
    let wrapper = render_distrobox_wrapper(&tool, &description, box_name);
    atomic_write_text(output, &wrapper, Some(0o755)).map_err(|error| error.to_string())
}

fn disable_gaming_coprs() -> Result<(), String> {
    let mut failures = Vec::new();
    for copr in GAMING_COPRS {
        let argv = vec![
            "dnf5".to_string(),
            "copr".to_string(),
            "disable".to_string(),
            "-y".to_string(),
            copr.to_string(),
        ];
        match kyth_shared::system::process::run_bounded(&argv, Duration::from_secs(30)) {
            Ok(output) if output.status.success() => {}
            Ok(output) => failures.push(format!("{copr} (exit {})", output.status)),
            Err(error) => failures.push(format!("{copr} ({error})")),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        // Preserve the package fragment's best-effort behavior: the image
        // remains buildable when an optional COPR is already absent.
        eprintln!(
            "warning: could not disable optional COPRs: {}",
            failures.join(", ")
        );
        Ok(())
    }
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        usage();
        return ExitCode::from(2);
    };
    let rest: Vec<String> = args.collect();
    let result = match command.as_str() {
        "gaming-label" => {
            println!("{}", GamingVersions::load_runtime().label());
            Ok(())
        }
        "gaming-coprs" => {
            for copr in GAMING_COPRS {
                println!("{copr}");
            }
            Ok(())
        }
        "disable-gaming-coprs" => disable_gaming_coprs(),
        "repo-render" => render_repo(&rest),
        "container-wrapper" => render_container(&rest),
        _ => {
            usage();
            Err(format!("unknown command {command:?}"))
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kyth-build-support: {error}");
            ExitCode::from(1)
        }
    }
}
