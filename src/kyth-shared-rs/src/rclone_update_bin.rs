//! Native replacement for the Python `kyth-rclone-update` launcher.
//!
//! Installs or updates the rclone binary from official releases with
//! SHA256 verification. `RCLONE_VERSION` overrides the live tag lookup.
//! Exits `1` with the launcher error lines on failure.
//! `system/updater.py` stays as the Phase 3 fixture.

use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kyth_shared::system::process::run_bounded;
use kyth_shared::system::release_fetch::{
    TempWorkdir, download_file, extract_archive, fetch_github_latest_release,
    github_headers, read_secret_file, validate_version, verify_checksum_file,
};

const REPO: &str = "rclone/rclone";
const VERSION_PATTERN: &str = r"v[0-9]+\.[0-9]+\.[0-9]+";
const RCLONE_BIN: &str = "/usr/local/bin/rclone";

fn run(argv: &[String], timeout_secs: u64) -> Option<(i32, String)> {
    run_bounded(argv, Duration::from_secs(timeout_secs)).ok().map(|output| {
        (
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )
    })
}

fn fail(message: String) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

#[cfg(unix)]
fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) {}

fn installed_version() -> Option<String> {
    if !Path::new(RCLONE_BIN).is_file() {
        return None;
    }
    let (_, stdout) = run(&[RCLONE_BIN.to_string(), "--version".to_string()], 30)?;
    let mut parts = stdout.lines().next()?.split_whitespace();
    if parts.next()? != "rclone" {
        return None;
    }
    let tagged = parts.next()?;
    if !tagged.starts_with('v') {
        return None;
    }
    Some(tagged.trim_start_matches('v').to_string())
}

fn main() -> std::process::ExitCode {
    let mut rclone_ver = env::var("RCLONE_VERSION").unwrap_or_default();
    if rclone_ver.is_empty() {
        println!("Fetching latest rclone release metadata...");
        let secret = read_secret_file(Path::new("/run/secrets/github_token"));
        let env_token = env::var("GITHUB_TOKEN").ok();
        let headers = github_headers(secret.as_deref(), env_token.as_deref());
        match fetch_github_latest_release(&run, REPO, &headers) {
            Ok(release) => {
                rclone_ver = release.get("tag_name").and_then(serde_json::Value::as_str).unwrap_or("").to_string();
            }
            Err(error) => fail(format!("ERROR: Could not determine latest rclone release tag: {error}")),
        }
    }
    if rclone_ver.is_empty() {
        fail("ERROR: Could not determine latest rclone release tag".to_string());
    }
    if validate_version(&rclone_ver, VERSION_PATTERN, "rclone").is_err() {
        fail(format!("ERROR: Unexpected rclone version format: {rclone_ver}"));
    }
    let target_ver = rclone_ver.trim_start_matches('v').to_string();
    if let Some(installed) = installed_version() {
        if installed == target_ver {
            println!("rclone already current: v{installed}");
            return std::process::ExitCode::SUCCESS;
        }
    }
    let basename = format!("rclone-{rclone_ver}-linux-amd64");
    let zip_name = format!("{basename}.zip");
    let base_url = format!("https://downloads.rclone.org/{rclone_ver}");
    let work = match TempWorkdir::create("kyth-rclone") {
        Ok(work) => work,
        Err(error) => fail(format!("ERROR: Failed to download rclone assets: {error}")),
    };
    let headers = std::collections::BTreeMap::new();
    let zip_dest = work.path().join(&zip_name);
    let sums_dest = work.path().join("SHA256SUMS");
    let sums_name = "SHA256SUMS".to_string();
    for (file, dest) in [(&zip_name, &zip_dest), (&sums_name, &sums_dest)] {
        println!("rclone: downloading {file}...");
        if let Err(error) = download_file(&run, &format!("{base_url}/{file}"), dest, &headers, 120) {
            fail(format!("ERROR: Failed to download rclone assets: {error}"));
        }
    }
    println!("Verifying checksum...");
    if let Err(error) = verify_checksum_file(&sums_dest, &zip_dest, "sha256") {
        fail(format!("ERROR: Checksum verification failed: {error}"));
    }
    println!("Extracting archive...");
    if let Err(error) = extract_archive(&run, &zip_dest, work.path()) {
        fail(format!("ERROR: Extraction failed: {error}"));
    }
    let mut extracted = work.path().join(&basename).join("rclone");
    if !extracted.is_file() {
        extracted = work.path().join("rclone");
    }
    let target = PathBuf::from(RCLONE_BIN);
    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::copy(&extracted, &target) {
        Ok(_) => set_executable(&target),
        Err(error) => fail(format!("ERROR: Failed to install rclone binary to {}: {error}", target.display())),
    }
    match run(&[RCLONE_BIN.to_string(), "--version".to_string()], 30) {
        Some((_, stdout)) => {
            println!("rclone installed: {}", stdout.lines().next().unwrap_or(""));
        }
        None => println!("rclone installed, but version check failed: rclone could not be executed"),
    }
    std::process::ExitCode::SUCCESS
}
