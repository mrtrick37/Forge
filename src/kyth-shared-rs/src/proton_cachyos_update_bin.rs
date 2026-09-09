//! Native replacement for the Python `kyth-proton-cachyos-update`
//! launcher.
//!
//! Fetches the latest Proton-CachyOS release, verifies its checksum,
//! extracts it, and prunes old versions. Skipped on live ISOs. Exits `1`
//! with the launcher error lines on failure. `system/updater.py` stays
//! as the Phase 3 fixture.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use kyth_shared::system::process::run_bounded;
use kyth_shared::system::release_fetch::{
    download_file, extract_archive, fetch_github_latest_release, find_release_asset,
    github_headers, prune_installations, read_secret_file, release_assets, validate_version,
    verify_checksum_file, TempWorkdir,
};

const REPO: &str = "CachyOS/proton-cachyos";
const VERSION_PATTERN: &str = r"cachyos-[0-9]+\.[0-9]+-[0-9]{8}-slr";

fn run(argv: &[String], timeout_secs: u64) -> Option<(i32, String)> {
    run_bounded(argv, Duration::from_secs(timeout_secs))
        .ok()
        .map(|output| {
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

fn main() -> std::process::ExitCode {
    if let Ok(cmdline) = std::fs::read_to_string("/proc/cmdline") {
        if cmdline
            .split_whitespace()
            .any(|arg| arg == "kyth.live" || arg == "kyth.live=1")
        {
            println!("Proton-CachyOS update disabled in live ISO environment.");
            return std::process::ExitCode::SUCCESS;
        }
    }
    let install_dir = PathBuf::from("/var/lib/kyth/proton-cachyos");
    println!("Fetching latest Proton-CachyOS release metadata...");
    let secret = read_secret_file(std::path::Path::new("/run/secrets/github_token"));
    let env_token = env::var("GITHUB_TOKEN").ok();
    let headers = github_headers(secret.as_deref(), env_token.as_deref());
    let release = match fetch_github_latest_release(&run, REPO, &headers) {
        Ok(release) => release,
        Err(error) => fail(format!(
            "Failed to fetch Proton-CachyOS release info: {error}"
        )),
    };
    let ver = release
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if ver.is_empty() {
        fail("Failed to parse Proton-CachyOS version tag from release JSON".to_string());
    }
    if validate_version(ver, VERSION_PATTERN, "Proton-CachyOS").is_err() {
        fail(format!("Unexpected Proton-CachyOS version format: {ver}"));
    }
    let assets = release_assets(&release);
    let tarball = find_release_asset(&assets, |name| name.ends_with("x86_64.tar.xz"));
    let checksum = find_release_asset(&assets, |name| name.ends_with("x86_64.sha512sum"));
    let (Some(tarball), Some(checksum)) = (tarball, checksum) else {
        fail("Failed to locate Proton-CachyOS release assets".to_string());
    };
    let folder = tarball
        .name
        .strip_suffix(".tar.xz")
        .or_else(|| tarball.name.strip_suffix(".xz"))
        .unwrap_or(&tarball.name)
        .to_string();
    if install_dir.join(&folder).is_dir() {
        println!("Proton-CachyOS {ver} is already up to date.");
        return std::process::ExitCode::SUCCESS;
    }
    println!("Updating to Proton-CachyOS {ver}...");
    let work = match TempWorkdir::create("kyth-proton") {
        Ok(work) => work,
        Err(error) => fail(format!("Failed to download assets: {error}")),
    };
    let tarball_dest = work.path().join(&tarball.name);
    let sha512_dest = work.path().join(&checksum.name);
    let downloads = [
        (&tarball.url, &tarball_dest, &tarball.name),
        (&checksum.url, &sha512_dest, &checksum.name),
    ];
    for (url, dest, name) in downloads {
        println!("Downloading {name}...");
        if let Err(error) = download_file(&run, url, dest, &headers, 120) {
            fail(format!("Failed to download assets: {error}"));
        }
    }
    println!("Verifying checksum...");
    if let Err(error) = verify_checksum_file(&sha512_dest, &tarball_dest, "sha512") {
        fail(format!("Checksum verification failed: {error}"));
    }
    println!("Extracting to {}...", install_dir.display());
    if let Err(error) = extract_archive(&run, &tarball_dest, &install_dir) {
        fail(format!("Extraction failed: {error}"));
    }
    println!(
        "Proton-CachyOS {ver} installed to {}/",
        install_dir.display()
    );
    match prune_installations(&install_dir, "proton-cachyos-*", 2) {
        Ok(removed) => {
            for old in &removed {
                if let Some(name) = old.file_name().map(|name| name.to_string_lossy()) {
                    println!("Removing old version: {name}");
                }
            }
        }
        Err(error) => eprintln!("Failed to prune old versions: {error}"),
    }
    std::process::ExitCode::SUCCESS
}
