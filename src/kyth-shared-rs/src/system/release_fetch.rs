//! Release-asset download, verification, and extraction.
//!
//! Ports `kyth_shared.system.updater` for the Proton-CachyOS and rclone
//! launchers. Network transfer and archive unpacking go through the same
//! CLI tools available on the target (`curl`, `tar`, `unzip`); all safety
//! properties are preserved: version gating before paths are built,
//! checksum-before-extract, member traversal/duplicate/count validation,
//! no links or devices, and a 16 GiB expansion cap. `system/updater.py`
//! stays as the Phase 3 fixture.

use super::desktop_shortcuts::matches_web_app_name;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const MAX_ARCHIVE_MEMBERS: usize = 100_000;
pub const MAX_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
pub const GITHUB_API: &str = "https://api.github.com";
pub const USER_AGENT: &str = "KythOS-Updater/1.0";

/// A normalized downloadable asset from a release document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub url: String,
}

/// Keep only well-formed assets, insulating callers from JSON details.
pub fn release_assets(release: &Value) -> Vec<ReleaseAsset> {
    release
        .get("assets")
        .and_then(Value::as_array)
        .map(|assets| {
            assets
                .iter()
                .filter_map(|item| {
                    let name = item.get("name").and_then(Value::as_str)?;
                    let url = item.get("browser_download_url").and_then(Value::as_str)?;
                    if name.is_empty() || url.is_empty() {
                        return None;
                    }
                    Some(ReleaseAsset { name: name.to_string(), url: url.to_string() })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// First asset whose name matches the predicate.
pub fn find_release_asset<'a>(
    assets: &'a [ReleaseAsset],
    predicate: impl Fn(&str) -> bool,
) -> Option<&'a ReleaseAsset> {
    assets.iter().find(|asset| predicate(&asset.name))
}

/// Validate an externally supplied version before using it in paths or
/// URLs, mirroring `re.fullmatch`.
pub fn validate_version(version: &str, pattern: &str, component: &str) -> Result<String, String> {
    let full = format!("^(?:{pattern})$");
    let matched = Regex::new(&full).ok().is_some_and(|matcher| matcher.is_match(version));
    if matched {
        Ok(version.to_string())
    } else {
        Err(format!("Unexpected {component} version format: {version}"))
    }
}

/// Remove older version directories, retaining the newest `keep` by
/// mtime. Removal errors propagate like `shutil.rmtree` and the removed
/// paths are returned newest-first.
pub fn prune_installations(
    install_dir: &Path,
    pattern: &str,
    keep: usize,
) -> Result<Vec<PathBuf>, String> {
    let mut entries: Vec<(i64, PathBuf)> = std::fs::read_dir(install_dir)
        .map(|listing| {
            listing
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| {
                    path.is_dir()
                        && path.file_name().is_some_and(|name| {
                            matches_web_app_name(&name.to_string_lossy(), pattern)
                        })
                })
                .filter_map(|path| {
                    std::fs::metadata(&path).ok().map(|meta| {
                        #[cfg(unix)]
                        let mtime = std::os::unix::fs::MetadataExt::mtime(&meta);
                        #[cfg(not(unix))]
                        let mtime = 0;
                        (mtime, path)
                    })
                })
                .collect()
        })
        .map_err(|error| format!("Cannot scan installations: {error}"))?;
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let removed: Vec<PathBuf> =
        entries.into_iter().skip(keep).map(|(_, path)| path).collect();
    for path in &removed {
        std::fs::remove_dir_all(path)
            .map_err(|error| format!("Cannot remove {}: {error}", path.display()))?;
    }
    Ok(removed)
}

/// Default GitHub headers, incorporating auth tokens when available.
pub fn github_headers(secret_token: Option<&str>, env_token: Option<&str>) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::from([
        ("User-Agent".to_string(), USER_AGENT.to_string()),
        ("Accept".to_string(), "application/vnd.github.v3+json".to_string()),
    ]);
    let token = secret_token
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .or_else(|| env_token.map(str::trim).filter(|token| !token.is_empty()));
    if let Some(token) = token {
        headers.insert("Authorization".to_string(), format!("token {token}"));
    }
    headers
}

/// Best-effort read of a secret file, mirroring the token-file branch.
pub fn read_secret_file(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    std::fs::read_to_string(path).ok().map(|text| text.trim().to_string()).filter(|text| !text.is_empty())
}

fn header_args(headers: &BTreeMap<String, String>) -> Vec<String> {
    let mut args = Vec::new();
    for (name, value) in headers {
        args.push("-H".to_string());
        args.push(format!("{name}: {value}"));
    }
    args
}

/// `curl` argv for fetching a JSON document with a time bound.
pub fn curl_fetch_argv(url: &str, headers: &BTreeMap<String, String>, timeout_secs: u64) -> Vec<String> {
    let mut argv = vec![
        "curl".to_string(),
        "-fsSL".to_string(),
        "--max-time".to_string(),
        timeout_secs.to_string(),
    ];
    argv.extend(header_args(headers));
    argv.push(url.to_string());
    argv
}

/// `curl` argv for downloading a file with a time bound.
pub fn curl_download_argv(
    url: &str,
    dest: &Path,
    headers: &BTreeMap<String, String>,
    timeout_secs: u64,
) -> Vec<String> {
    let mut argv = vec![
        "curl".to_string(),
        "-fsSL".to_string(),
        "--max-time".to_string(),
        timeout_secs.to_string(),
        "-o".to_string(),
        dest.to_string_lossy().into_owned(),
    ];
    argv.extend(header_args(headers));
    argv.push(url.to_string());
    argv
}

/// Fetch and parse the latest-release document for a GitHub repo.
pub fn fetch_github_latest_release(
    run: &dyn for<'x> Fn(&'x [String], u64) -> Option<(i32, String)>,
    repo: &str,
    headers: &BTreeMap<String, String>,
) -> Result<Value, String> {
    let url = format!("{GITHUB_API}/repos/{repo}/releases/latest");
    match (run)(&curl_fetch_argv(&url, headers, 30), 35) {
        Some((0, stdout)) => serde_json::from_str(&stdout)
            .map_err(|error| format!("Failed to parse release info: {error}")),
        Some((code, stderr)) => Err(format!("Failed to fetch release info: curl exited {code}: {}", stderr.trim())),
        None => Err("Failed to fetch release info: request failed".to_string()),
    }
}

/// Download a URL to `dest`.
pub fn download_file(
    run: &dyn for<'x> Fn(&'x [String], u64) -> Option<(i32, String)>,
    url: &str,
    dest: &Path,
    headers: &BTreeMap<String, String>,
    timeout_secs: u64,
) -> Result<(), String> {
    match (run)(&curl_download_argv(url, dest, headers, timeout_secs), timeout_secs + 5) {
        Some((0, _)) => Ok(()),
        Some((code, stderr)) => Err(format!("Failed to download {url}: curl exited {code}: {}", stderr.trim())),
        None => Err(format!("Failed to download {url}: request failed")),
    }
}

/// Scratch directory removed on drop, mirroring `TemporaryDirectory`.
pub struct TempWorkdir {
    path: PathBuf,
}

impl TempWorkdir {
    pub fn create(prefix: &str) -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("{prefix}-{}", std::process::id()));
        if path.exists() {
            std::fs::remove_dir_all(&path).ok();
        }
        std::fs::create_dir_all(&path)
            .map_err(|error| format!("Cannot create temporary directory: {error}"))?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempWorkdir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn digest_bytes(algorithm: &str, data: &[u8]) -> Option<(Vec<u8>, usize)> {
    use sha2::Digest;
    let (bytes, length): (Vec<u8>, usize) = match algorithm.to_ascii_lowercase().as_str() {
        "sha224" => (sha2::Sha224::digest(data).to_vec(), 56),
        "sha256" => (sha2::Sha256::digest(data).to_vec(), 64),
        "sha384" => (sha2::Sha384::digest(data).to_vec(), 96),
        "sha512" => (sha2::Sha512::digest(data).to_vec(), 128),
        _ => return None,
    };
    Some((bytes, length))
}

/// Require exactly one valid checksum entry for the target file, with the
/// same error contract as the Python verifier.
pub fn verify_checksum_file(
    checksum_path: &Path,
    target_path: &Path,
    algorithm: &str,
) -> Result<(), String> {
    let target_meta = match std::fs::symlink_metadata(target_path) {
        Ok(meta) => meta,
        Err(_) => {
            return Err(format!("Checksum target is not a regular file: {}", target_path.display()));
        }
    };
    if !target_meta.is_file() {
        return Err(format!("Checksum target is not a regular file: {}", target_path.display()));
    }
    let Some((_, expected_length)) = digest_bytes(algorithm, &[]) else {
        return Err(format!("Unsupported checksum algorithm: {algorithm}"));
    };
    let target_name = target_path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let content = std::fs::read_to_string(checksum_path)
        .map_err(|error| format!("Cannot read checksum file: {error}"))?;
    let mut matches = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let line_number = index + 1;
        let stripped = line.trim();
        if stripped.is_empty() || stripped.starts_with('#') {
            continue;
        }
        let mut parts = stripped.splitn(2, char::is_whitespace);
        let (expected_hash, filename) = match (parts.next(), parts.next()) {
            (Some(hash), Some(name)) => (hash, name.trim_start()),
            _ => return Err(format!("Malformed checksum entry on line {line_number}")),
        };
        let filename = filename.strip_prefix('*').unwrap_or(filename);
        let filename = filename.strip_prefix("./").unwrap_or(filename);
        if filename.is_empty() || filename.contains('/') {
            return Err(format!("Unsafe checksum filename on line {line_number}"));
        }
        if expected_hash.len() != expected_length
            || !expected_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(format!("Malformed {algorithm} digest on line {line_number}"));
        }
        if filename == target_name {
            matches.push(expected_hash.to_ascii_lowercase());
        }
    }
    if matches.len() != 1 {
        return Err(format!(
            "Expected exactly one checksum for {target_name}, found {}",
            matches.len()
        ));
    }
    let data = std::fs::read(target_path)
        .map_err(|error| format!("Cannot read checksum target: {error}"))?;
    let (actual, _) = digest_bytes(algorithm, &data).expect("algorithm checked above");
    let actual_hex: String = actual.iter().map(|byte| format!("{byte:02x}")).collect();
    if actual_hex != matches[0] {
        return Err(format!(
            "Checksum mismatch for {target_name}: expected {}, got {actual_hex}",
            matches[0]
        ));
    }
    Ok(())
}

/// Archive flavor selected from the file name, mirroring the Python
/// suffix dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Tar,
    TarGz,
    TarXz,
    Zip,
}

pub fn archive_kind(archive: &Path) -> ArchiveKind {
    let name = archive.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    if name.ends_with(".tar.xz") || archive.extension().is_some_and(|ext| ext == "xz") {
        ArchiveKind::TarXz
    } else if name.ends_with(".tar.gz") || archive.extension().is_some_and(|ext| ext == "gz") {
        ArchiveKind::TarGz
    } else if archive.extension().is_some_and(|ext| ext == "zip") {
        ArchiveKind::Zip
    } else {
        ArchiveKind::Tar
    }
}

/// Split an archive member into portable path parts, rejecting NUL bytes,
/// absolute paths, and parent traversal like `_archive_output_path`.
pub fn member_parts(name: &str) -> Result<Vec<String>, String> {
    if name.contains('\0') {
        return Err("Archive member contains a NUL byte".to_string());
    }
    if name.starts_with('/') {
        return Err(format!("Directory traversal attempt detected: {name}"));
    }
    let mut parts = Vec::new();
    for part in name.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(format!("Directory traversal attempt detected: {name}"));
        }
        parts.push(part.to_string());
    }
    Ok(parts)
}

/// Validate a member listing: traversal, duplicates, and the member cap.
/// Returns the normalized part lists for extraction bookkeeping.
pub fn validate_members(members: &[String]) -> Result<Vec<Vec<String>>, String> {
    if members.len() > MAX_ARCHIVE_MEMBERS {
        return Err("Archive contains too many members".to_string());
    }
    let mut seen = std::collections::HashSet::new();
    let mut normalized = Vec::with_capacity(members.len());
    for name in members {
        let parts = member_parts(name)?;
        let key = parts.join("/");
        if !seen.insert(key) {
            return Err(format!("Duplicate archive member: {name}"));
        }
        normalized.push(parts);
    }
    Ok(normalized)
}

fn list_members(run: &dyn for<'x> Fn(&'x [String], u64) -> Option<(i32, String)>, archive: &Path) -> Result<Vec<String>, String> {
    let arg = archive.to_string_lossy().into_owned();
    let (argv, timeout) = match archive_kind(archive) {
        ArchiveKind::Zip => (vec!["unzip".to_string(), "-Z1".to_string(), arg], 60),
        _ => (vec!["tar".to_string(), "-tf".to_string(), arg], 120),
    };
    match (run)(&argv, timeout) {
        Some((0, stdout)) => Ok(stdout.lines().map(str::to_string).collect()),
        Some((code, stderr)) => Err(format!("Cannot list archive: tool exited {code}: {}", stderr.trim())),
        None => Err("Cannot list archive: listing failed".to_string()),
    }
}

fn walk_entries(root: &Path) -> Vec<(PathBuf, bool, bool, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut children: Vec<PathBuf> =
            entries.filter_map(|entry| entry.ok().map(|entry| entry.path())).collect();
        children.sort();
        for child in children {
            let Ok(meta) = std::fs::symlink_metadata(&child) else { continue };
            let kind = meta.file_type();
            if kind.is_dir() && !kind.is_symlink() {
                stack.push(child);
            } else {
                out.push((child, kind.is_symlink(), !kind.is_file() && !kind.is_dir(), meta.len()));
            }
        }
    }
    out
}

/// Best-effort removal of previously validated member paths.
fn remove_validated(dest: &Path, members: &[Vec<String>]) {
    let mut paths: Vec<PathBuf> = members
        .iter()
        .map(|parts| parts.iter().fold(dest.to_path_buf(), |base, part| base.join(part)))
        .collect();
    paths.sort();
    paths.dedup();
    paths.reverse();
    for path in paths {
        if path == dest {
            continue;
        }
        if path.is_dir() && !path.is_symlink() {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Extract regular files and directories only: links, devices, and
/// over-cap expansions are refused after unpacking into `dest`, with
/// best-effort cleanup so nothing unexpected persists.
pub fn extract_archive(run: &dyn for<'x> Fn(&'x [String], u64) -> Option<(i32, String)>, archive: &Path, dest_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest_dir)
        .map_err(|error| format!("Cannot prepare archive destination: {error}"))?;
    let dest_meta = std::fs::symlink_metadata(dest_dir)
        .map_err(|error| format!("Cannot prepare archive destination: {error}"))?;
    if !dest_meta.is_dir() {
        return Err(format!("Archive destination is not a real directory: {}", dest_dir.display()));
    }
    let members = list_members(run, archive)?;
    let normalized = validate_members(&members)?;
    let arg = archive.to_string_lossy().into_owned();
    let dest_arg = dest_dir.to_string_lossy().into_owned();
    let argv = match archive_kind(archive) {
        ArchiveKind::Zip => vec!["unzip".to_string(), "-oq".to_string(), arg, "-d".to_string(), dest_arg],
        _ => vec!["tar".to_string(), "-xf".to_string(), arg, "-C".to_string(), dest_arg, "--no-same-owner".to_string()],
    };
    match (run)(&argv, 600) {
        Some((0, _)) => {}
        Some((code, stderr)) => return Err(format!("Extraction failed: tool exited {code}: {}", stderr.trim())),
        None => return Err("Extraction failed: unpacking failed".to_string()),
    }
    let mut total: u64 = 0;
    for (path, is_link, is_special, size) in walk_entries(dest_dir) {
        total += size;
        if is_link || is_special {
            let rel = path.strip_prefix(dest_dir).map(|rel| rel.to_string_lossy().into_owned()).unwrap_or_default();
            remove_validated(dest_dir, &normalized);
            return Err(format!("Unsupported archive member type: {rel}"));
        }
    }
    if total > MAX_ARCHIVE_BYTES {
        remove_validated(dest_dir, &normalized);
        return Err("Archive expands beyond the permitted size".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;
    use std::time::Duration;

    fn runner() -> impl for<'x> Fn(&'x [String], u64) -> Option<(i32, String)> {
        |argv: &[String], secs: u64| {
            super::super::process::run_bounded(argv, Duration::from_secs(secs))
                .ok()
                .map(|output| {
                    (
                        output.status.code().unwrap_or(1),
                        String::from_utf8_lossy(&output.stdout).into_owned(),
                    )
                })
        }
    }

    #[test]
    fn filters_assets_and_validates_versions() {
        let release: Value = serde_json::from_str(
            "{\"assets\": [{\"name\": \"tool.tar.xz\", \"browser_download_url\": \"https://x/y\"}, {\"name\": \"\"}, {}]}",
        )
        .unwrap();
        let assets = release_assets(&release);
        assert_eq!(assets.len(), 1);
        assert_eq!(
            find_release_asset(&assets, |name| name.ends_with(".xz")).map(|asset| asset.name.as_str()),
            Some("tool.tar.xz")
        );
        assert_eq!(validate_version("v1.2.3", r"v[0-9]+\.[0-9]+\.[0-9]+", "tool").unwrap(), "v1.2.3");
        assert!(validate_version("1.2.3", r"v[0-9]+\.[0-9]+\.[0-9]+", "tool").unwrap_err().contains("Unexpected tool"));
    }

    #[test]
    fn prunes_oldest_installations_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        for name in ["proton-cachyos-a", "proton-cachyos-b", "proton-cachyos-c", "other"] {
            let path = dir.path().join(name);
            std::fs::create_dir_all(&path).unwrap();
            paths.push(path);
        }
        set_mtime(&paths[0], 100);
        set_mtime(&paths[1], 300);
        set_mtime(&paths[2], 200);
        set_mtime(&paths[3], 400);
        let removed = prune_installations(dir.path(), "proton-cachyos-*", 2).unwrap();
        assert_eq!(removed, vec![paths[0].clone()]);
        assert!(paths[1].is_dir() && paths[2].is_dir() && paths[3].is_dir());
    }

    fn set_mtime(path: &Path, secs: i64) {
        let times = [
            libc::timespec { tv_sec: secs, tv_nsec: 0 },
            libc::timespec { tv_sec: secs, tv_nsec: 0 },
        ];
        let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        unsafe {
            assert_eq!(libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), 0), 0);
        }
    }

    #[test]
    fn builds_github_headers_with_token_precedence() {
        let headers = github_headers(Some("  file-token  "), Some("env-token"));
        assert_eq!(headers["Authorization"], "token file-token");
        let headers = github_headers(None, Some("env-token"));
        assert_eq!(headers["Authorization"], "token env-token");
        let headers = github_headers(None, None);
        assert!(!headers.contains_key("Authorization"));
        assert_eq!(headers["User-Agent"], USER_AGENT);
    }

    #[test]
    fn verifies_checksums_with_exact_errors() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("tool.tar.xz");
        std::fs::write(&target, "payload").unwrap();
        let good = sha256_hex(b"payload");
        std::fs::write(dir.path().join("sums"), format!("{good}  tool.tar.xz\n")).unwrap();
        assert!(verify_checksum_file(&dir.path().join("sums"), &target, "sha256").is_ok());
        std::fs::write(dir.path().join("bad"), format!("{}  tool.tar.xz\n", "0".repeat(64))).unwrap();
        assert!(verify_checksum_file(&dir.path().join("bad"), &target, "sha256").unwrap_err().contains("Checksum mismatch"));
        std::fs::write(dir.path().join("none"), "# nothing\n").unwrap();
        assert!(verify_checksum_file(&dir.path().join("none"), &target, "sha256").unwrap_err().contains("exactly one"));
        assert!(verify_checksum_file(&dir.path().join("sums"), &target, "md5").unwrap_err().contains("Unsupported checksum algorithm"));
        std::os::unix::fs::symlink(&target, dir.path().join("link")).unwrap();
        assert!(verify_checksum_file(&dir.path().join("sums"), &dir.path().join("link"), "sha256").unwrap_err().contains("not a regular file"));
    }

    fn sha256_hex(data: &[u8]) -> String {
        use sha2::Digest;
        sha2::Sha256::digest(data).iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn rejects_traversal_duplicates_and_nul_members() {
        assert!(validate_members(&["../evil".to_string()]).unwrap_err().to_string().contains("traversal"));
        assert!(validate_members(&["/abs".to_string()]).unwrap_err().to_string().contains("traversal"));
        assert!(validate_members(&["a", "a"].iter().map(|name| name.to_string()).collect::<Vec<_>>())
            .unwrap_err()
            .to_string()
            .contains("Duplicate"));
        assert!(validate_members(&["a\0b".to_string()]).unwrap_err().to_string().contains("NUL"));
        assert_eq!(
            validate_members(&["./x", "y/"].iter().map(|name| name.to_string()).collect::<Vec<_>>()).unwrap().len(),
            2
        );
    }

    #[test]
    fn round_trips_tar_and_zip_through_real_tools() {
        let run = runner();
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("pkg")).unwrap();
        std::fs::write(src.join("pkg/tool"), "bits").unwrap();
        let tarball = dir.path().join("pkg.tar.gz");
        assert!(run(&["tar".to_string(), "-czf".to_string(), tarball.to_string_lossy().into_owned(), "-C".to_string(), src.to_string_lossy().into_owned(), "pkg".to_string()], 30).is_some_and(|(code, _)| code == 0));
        let out = dir.path().join("tar-out");
        extract_archive(&run, &tarball, &out).unwrap();
        assert_eq!(std::fs::read_to_string(out.join("pkg/tool")).unwrap(), "bits");
        let zipfile = dir.path().join("pkg.zip");
        assert!(std::process::Command::new("zip")
            .args(["-qr", &zipfile.to_string_lossy(), "pkg"])
            .current_dir(&src)
            .output()
            .is_ok_and(|output| output.status.success()));
        let out = dir.path().join("zip-out");
        std::fs::create_dir_all(&out).unwrap();
        extract_archive(&run, &zipfile, &out).unwrap();
        assert_eq!(std::fs::read_to_string(out.join("pkg/tool")).unwrap(), "bits");
    }

    #[test]
    fn refuses_symlink_members_after_unpack() {
        let run = runner();
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("real"), "data").unwrap();
        std::os::unix::fs::symlink("real", src.join("link")).unwrap();
        let tarball = dir.path().join("evil.tar.gz");
        assert!(run(&["tar".to_string(), "-czf".to_string(), tarball.to_string_lossy().into_owned(), "-C".to_string(), src.to_string_lossy().into_owned(), ".".to_string()], 30).is_some());
        let error = extract_archive(&run, &tarball, &dir.path().join("out")).unwrap_err();
        assert!(error.contains("Unsupported archive member type"));
    }
}
