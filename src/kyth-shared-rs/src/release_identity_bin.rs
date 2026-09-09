use kyth_shared::release_identity::build_identity;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

fn argument(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find_map(|pair| (pair[0] == name).then(|| pair[1].clone()))
}

fn required(args: &[String], name: &str) -> Result<String, String> {
    argument(args, name)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing required argument {name}"))
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let source_tag = required(&args, "--source-tag")?;
    let source_sha = argument(&args, "--source-sha").unwrap_or_else(|| {
        let command = ["git", "rev-parse", "HEAD"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        kyth_shared::system::process::run_bounded(&command, Duration::from_secs(5))
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .unwrap_or_default()
    });
    let run_number = required(&args, "--run-number")?;
    let run_attempt = required(&args, "--run-attempt")?;
    let identity = build_identity(
        &source_tag,
        &source_sha,
        &run_number,
        &run_attempt,
        argument(&args, "--build-date").as_deref(),
    )?;
    let output = identity.github_output();
    if let Some(path) = argument(&args, "--github-output") {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(PathBuf::from(path))
            .map_err(|error| error.to_string())?;
        file.write_all(output.as_bytes())
            .map_err(|error| error.to_string())?;
    } else {
        print!("{output}");
    }
    Ok(())
}
