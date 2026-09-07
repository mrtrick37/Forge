//! Native replacement for the Python `kyth-report-issue` launcher.
//!
//! Writes a local Markdown issue draft and prints the prefilled GitHub
//! issue URL, opening it in the browser unless `--no-open`. Errors go to
//! stderr with exit `1`; CLI misuse exits `2` like argparse. `diagnostics.py`
//! stays as the Phase 3 fixture.

use std::env;

use kyth_shared::atomic_io::atomic_write_text;
use kyth_shared::system::issue_draft::{
    DEFAULT_REPO_URL, draft_dir, draft_path, issue_url, local_timestamp, render_draft, resolve_body,
};

const DESCRIPTION: &str = "Creates a prefilled GitHub issue URL for KythOS and writes a local draft.";

fn usage() -> String {
    "Usage: kyth-report-issue [--title TITLE] [--body BODY] [--body-file PATH] [--label LABEL] [--no-open]".to_string()
}

fn on_path(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

fn main() -> std::process::ExitCode {
    let mut title = "KythOS issue report".to_string();
    let mut body = String::new();
    let mut body_file: Option<String> = None;
    let mut label = env::var("KYTH_ISSUE_LABEL").unwrap_or_else(|_| "bug".to_string());
    let mut no_open = false;
    let repo_url =
        env::var("KYTH_ISSUE_REPO_URL").unwrap_or_else(|_| DEFAULT_REPO_URL.to_string());

    let mut args = env::args().skip(1).peekable();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) => (flag.to_string(), Some(value.to_string())),
            None => (arg.clone(), None),
        };
        let mut value = inline;
        let mut take_next = |args: &mut std::iter::Peekable<std::iter::Skip<std::env::Args>>| -> Result<String, std::process::ExitCode> {
            if let Some(inline) = value.take() {
                return Ok(inline);
            }
            match args.next() {
                Some(next) => Ok(next),
                None => {
                    eprintln!("{usage}", usage = usage());
                    eprintln!("error: argument {flag}: expected one argument");
                    Err(std::process::ExitCode::from(2))
                }
            }
        };
        match flag.as_str() {
            "-h" | "--help" => {
                println!("{DESCRIPTION}\n\n{}", usage());
                return std::process::ExitCode::SUCCESS;
            }
            "--title" => title = match take_next(&mut args) {
                Ok(value) => value,
                Err(code) => return code,
            },
            "--body" => body = match take_next(&mut args) {
                Ok(value) => value,
                Err(code) => return code,
            },
            "--body-file" => body_file = Some(match take_next(&mut args) {
                Ok(value) => value,
                Err(code) => return code,
            }),
            "--label" => label = match take_next(&mut args) {
                Ok(value) => value,
                Err(code) => return code,
            },
            "--no-open" => no_open = true,
            _ => {
                eprintln!("{}", usage());
                eprintln!("error: unrecognized arguments: {arg}");
                return std::process::ExitCode::from(2);
            }
        }
    }

    let resolved = match resolve_body(&body, body_file.as_deref()) {
        Ok(resolved) => resolved,
        Err(error) => {
            eprintln!("kyth-report-issue error: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let dir = draft_dir();
    if let Err(error) = std::fs::create_dir_all(&dir) {
        eprintln!("kyth-report-issue error: {error}");
        return std::process::ExitCode::FAILURE;
    }
    let path = draft_path(&dir, &local_timestamp());
    if let Err(error) = atomic_write_text(&path, &render_draft(&title, &resolved), None) {
        eprintln!("kyth-report-issue error: {error}");
        return std::process::ExitCode::FAILURE;
    }
    println!("Draft saved: {}", path.display());
    let url = issue_url(&repo_url, &title, &resolved, &label);
    println!("{url}");
    if !no_open && on_path("xdg-open") {
        let _ = std::process::Command::new("xdg-open")
            .arg(&url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    std::process::ExitCode::SUCCESS
}
