//! Native root-facing transport for the installer compatibility backend.
//!
//! The destructive installer phases are still implemented by the Python
//! backend while their Rust equivalents gain parity coverage.  This binary
//! owns the privileged Unix socket, validates the session boundary, and
//! exposes the backend only through a loopback connection.  The Python
//! process never binds the user-visible socket.

use std::ffi::CString;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use super::installer_job::{EventReplay, JobSnapshot, JobSupervisor, StartReceipt};
use super::installer_job_executor::{NativeInstallRequest, NativePhaseExecutor};
use super::installer_plan::{build_plan, InstallerPlanInput};
use super::installer_runtime::RuntimeCoordinator;
use super::installer_storage;

const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;
const BACKEND_SOCKET_SUFFIX: &str = ".backend";
const TRANSACTION_PATH: &str = "/run/kyth-installer/transaction.json";

type NativeSupervisor = JobSupervisor<NativePhaseExecutor>;

/// Shared native job ownership for the daemon's request threads.
///
/// A supervisor is bound to one validated install request, so the registry
/// stores the active supervisor rather than a reusable executor. The slot is
/// claimed while holding this mutex, closing the race between concurrent
/// `/api/start` requests.
struct NativeJobRegistry {
    active: Mutex<Option<NativeSupervisor>>,
}

impl Default for NativeJobRegistry {
    fn default() -> Self {
        Self {
            active: Mutex::new(None),
        }
    }
}

impl NativeJobRegistry {
    fn start(&self, request: NativeInstallRequest) -> Result<StartReceipt, String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "native installer job state is unavailable".to_string())?;
        if let Some(supervisor) = active.as_ref() {
            if supervisor.snapshot()?.worker_active {
                return Err("An installation is already running.".to_string());
            }
        }
        let supervisor = NativeSupervisor::new(NativePhaseExecutor::from_request(request)?);
        let receipt = supervisor.start()?;
        *active = Some(supervisor);
        Ok(receipt)
    }

    fn cancel(&self) -> Result<(), String> {
        let active = self
            .active
            .lock()
            .map_err(|_| "native installer job state is unavailable".to_string())?;
        active
            .as_ref()
            .ok_or_else(|| "No installation is running to cancel.".to_string())?
            .cancel()
    }

    fn snapshot(&self) -> Result<Option<JobSnapshot>, String> {
        let active = self
            .active
            .lock()
            .map_err(|_| "native installer job state is unavailable".to_string())?;
        active.as_ref().map(JobSupervisor::snapshot).transpose()
    }

    fn replay(&self, last_event_id: u64) -> Result<Option<EventReplay>, String> {
        let active = self
            .active
            .lock()
            .map_err(|_| "native installer job state is unavailable".to_string())?;
        active
            .as_ref()
            .map(|supervisor| supervisor.replay(last_event_id))
            .transpose()
    }

    fn wait_for_events(
        &self,
        last_event_id: u64,
        timeout: Duration,
    ) -> Result<Option<EventReplay>, String> {
        let active = self
            .active
            .lock()
            .map_err(|_| "native installer job state is unavailable".to_string())?;
        active
            .as_ref()
            .map(|supervisor| supervisor.wait_for_events(last_event_id, timeout))
            .transpose()
    }
}

struct Options {
    socket_path: PathBuf,
    session_token_file: PathBuf,
    socket_group: Option<String>,
    peer_uid: Option<u32>,
}

fn value(args: &[String], name: &str) -> Result<String, String> {
    let index = args
        .iter()
        .position(|arg| arg == name)
        .ok_or_else(|| format!("missing {name}"))?;
    args.get(index + 1)
        .cloned()
        .ok_or_else(|| format!("missing value for {name}"))
}

fn options(args: &[String]) -> Result<Options, String> {
    let socket_group = args
        .iter()
        .position(|arg| arg == "--socket-group")
        .map(|index| {
            args.get(index + 1)
                .cloned()
                .ok_or_else(|| "missing value for --socket-group".to_string())
        })
        .transpose()?;
    let peer_uid = args
        .iter()
        .position(|arg| arg == "--peer-uid")
        .map(|index| {
            args.get(index + 1)
                .ok_or_else(|| "missing value for --peer-uid".to_string())?
                .parse::<u32>()
                .map_err(|_| "invalid --peer-uid".to_string())
        })
        .transpose()?;
    Ok(Options {
        socket_path: PathBuf::from(value(args, "--socket-path")?),
        session_token_file: PathBuf::from(value(args, "--session-token-file")?),
        socket_group,
        peer_uid,
    })
}

fn read_session_token(path: &Path) -> Result<String, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| format!("could not open installer session token: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("could not stat installer session token: {error}"))?;
    if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & 0o077 != 0 {
        return Err(
            "installer session token must be a root-owned private regular file".to_string(),
        );
    }
    let mut token = String::new();
    (&file)
        .take(513)
        .read_to_string(&mut token)
        .map_err(|error| format!("could not read installer session token: {error}"))?;
    let token = token.trim();
    if !(32..=512).contains(&token.len())
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err("installer session token has an invalid format".to_string());
    }
    Ok(token.to_string())
}

fn group_id(name: &str) -> Result<u32, String> {
    let name = CString::new(name).map_err(|_| "socket group contains NUL".to_string())?;
    // SAFETY: getgrnam reads the process' system group database and returns a
    // pointer owned by libc; it is used only for the scalar gid value.
    let entry = unsafe { libc::getgrnam(name.as_ptr()) };
    if entry.is_null() {
        return Err(format!(
            "socket group does not exist: {}",
            name.to_string_lossy()
        ));
    }
    // SAFETY: entry was checked non-null above.
    Ok(unsafe { (*entry).gr_gid })
}

fn chown(path: &Path, gid: u32) -> Result<(), String> {
    let path = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| "socket path contains NUL".to_string())?;
    // SAFETY: path is a NUL-free CString and -1 preserves the current owner.
    if unsafe { libc::chown(path.as_ptr(), u32::MAX, gid) } != 0 {
        return Err(format!(
            "could not set socket group: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn listener(options: &Options) -> Result<UnixListener, String> {
    let parent = options
        .socket_path
        .parent()
        .ok_or_else(|| "socket path has no parent".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create installer socket directory: {error}"))?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o750))
        .map_err(|error| format!("could not secure installer socket directory: {error}"))?;
    let gid = options.socket_group.as_deref().map(group_id).transpose()?;
    if let Some(gid) = gid {
        chown(parent, gid)?;
    }

    if fs::symlink_metadata(&options.socket_path).is_ok() {
        let metadata =
            fs::symlink_metadata(&options.socket_path).map_err(|error| error.to_string())?;
        if !metadata.file_type().is_socket() {
            return Err("installer socket path is not a socket".to_string());
        }
        fs::remove_file(&options.socket_path)
            .map_err(|error| format!("could not replace installer socket: {error}"))?;
    }
    let socket = UnixListener::bind(&options.socket_path)
        .map_err(|error| format!("could not bind installer socket: {error}"))?;
    fs::set_permissions(
        &options.socket_path,
        fs::Permissions::from_mode(if gid.is_some() { 0o660 } else { 0o600 }),
    )
    .map_err(|error| format!("could not secure installer socket: {error}"))?;
    if let Some(gid) = gid {
        chown(&options.socket_path, gid)?;
    }
    Ok(socket)
}

fn peer_uid(stream: &UnixStream) -> Result<u32, String> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: credentials and length point to valid writable storage for the
    // socket option requested from this connected Unix stream.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut credentials as *mut _ as *mut _,
            &mut length,
        )
    };
    if result != 0 {
        return Err(format!(
            "could not inspect installer peer: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(credentials.uid)
}

fn route_allowed(method: &str, target: &str) -> bool {
    let path = target.split('?').next().unwrap_or(target);
    match method {
        "GET" => matches!(
            path,
            "/api/config"
                | "/api/disks"
                | "/api/partitions"
                | "/api/free-space"
                | "/api/timezones"
                | "/api/locales"
                | "/api/keymaps"
                | "/api/disk/pending"
                | "/api/disk/filesystems"
                | "/api/report"
                | "/api/runtime"
                | "/api/rescue/probe"
                | "/api/log"
                | "/api/stream"
        ),
        "POST" => matches!(
            path,
            "/api/start"
                | "/api/cancel"
                | "/api/reboot"
                | "/api/disk/new-table"
                | "/api/disk/create"
                | "/api/disk/delete"
                | "/api/disk/resize"
                | "/api/disk/format"
                | "/api/disk/set-mountpoint"
                | "/api/disk/pending/remove"
                | "/api/disk/commit"
                | "/api/disk/rollback"
                | "/api/rescue/logs-to-usb"
        ),
        _ => false,
    }
}

fn header_value<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then_some(value.trim())
    })
}

fn request_parts(request: &[u8]) -> Result<(&str, &str, &str), String> {
    let header_end = header_end(request)?;
    let headers = std::str::from_utf8(&request[..header_end])
        .map_err(|_| "installer request headers are not UTF-8".to_string())?;
    let mut line = headers
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let method = line.next().unwrap_or_default();
    let target = line.next().unwrap_or_default();
    let version = line.next().unwrap_or_default();
    if line.next().is_some()
        || method.is_empty()
        || target.is_empty()
        || !matches!(version, "HTTP/1.0" | "HTTP/1.1")
    {
        return Err("installer request line is invalid".to_string());
    }
    Ok((method, target, headers))
}

fn header_end(request: &[u8]) -> Result<usize, String> {
    request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
        .ok_or_else(|| "installer request has no complete headers".to_string())
}

fn read_request(stream: &mut UnixStream) -> Result<Vec<u8>, String> {
    let mut request = Vec::with_capacity(4096);
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let count = stream
            .read(&mut buffer)
            .map_err(|error| format!("could not read installer request: {error}"))?;
        if count == 0 {
            return Err("installer client closed before sending a request".to_string());
        }
        request.extend_from_slice(&buffer[..count]);
        if request.len() > MAX_REQUEST_BYTES {
            return Err("installer request is too large".to_string());
        }
        if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let header_text = std::str::from_utf8(&request[..header_end - 4])
        .map_err(|_| "installer request headers are not UTF-8".to_string())?;
    let content_length = header_value(header_text, "Content-Length")
        .unwrap_or("0")
        .parse::<usize>()
        .map_err(|_| "installer content length is invalid".to_string())?;
    if content_length > MAX_REQUEST_BYTES || header_end + content_length > MAX_REQUEST_BYTES {
        return Err("installer request body is too large".to_string());
    }
    while request.len() < header_end + content_length {
        let count = stream
            .read(&mut buffer)
            .map_err(|error| format!("could not read installer request body: {error}"))?;
        if count == 0 {
            return Err("installer request body is incomplete".to_string());
        }
        request.extend_from_slice(&buffer[..count]);
    }
    request.truncate(header_end + content_length);
    Ok(request)
}

fn forbidden(stream: &mut UnixStream) {
    let _ = stream
        .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
}

fn bad_start_request(stream: &mut UnixStream, message: &str) {
    let body = serde_json::json!({"started": false, "message": message}).to_string();
    let response = format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(), body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn json_response(stream: &mut UnixStream, status: &str, value: &serde_json::Value) {
    let body = value.to_string();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn add_display_sizes(value: &mut serde_json::Value) {
    let Some(items) = value.as_array_mut() else {
        return;
    };
    for item in items {
        let Some(object) = item.as_object_mut() else {
            continue;
        };
        let Some(size) = object.get("size_bytes").and_then(serde_json::Value::as_u64) else {
            continue;
        };
        object.insert(
            "size".to_string(),
            serde_json::json!(kyth_shared::transfer::human_bytes(size as f64)),
        );
    }
}

fn command_output(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} failed with exit code {}: {}",
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("{program} returned non-UTF-8 output"))
}

fn findmnt_sources(path: &str, recursive: bool) -> Result<Vec<String>, String> {
    let mut args = Vec::with_capacity(5);
    if recursive {
        args.push("-R");
    }
    args.extend(["-n", "-o", "SOURCE", path]);
    let output = Command::new("/usr/bin/findmnt")
        .args(args)
        .output()
        .map_err(|error| format!("could not run findmnt: {error}"))?;
    // findmnt uses exit code 1 for a path with no matching mount. That is a
    // normal result for optional live-media paths, unlike a probe failure.
    if !output.status.success() && output.status.code() != Some(1) {
        return Err(format!(
            "findmnt failed with exit code {}: {}",
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8(output.stdout)
        .map_err(|_| "findmnt returned non-UTF-8 output".to_string())?
        .lines()
        .map(str::trim)
        .filter(|source| source.starts_with("/dev/"))
        .map(str::to_string)
        .collect())
}

fn storage_lsblk_args(disk: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "--json".to_string(),
        "--bytes".to_string(),
        "--paths".to_string(),
        "--output".to_string(),
        "NAME,SIZE,TYPE,FSTYPE,PARTTYPE,LABEL,MOUNTPOINT,MOUNTPOINTS,START,RO,MODEL,TRAN,ROTA,RM,PTTYPE,PKNAME".to_string(),
    ];
    if let Some(disk) = disk {
        args.push(disk.to_string());
    }
    args
}

fn read_only_storage_route(
    method: &str,
    target: &str,
    runtime: &RuntimeCoordinator,
) -> Result<Option<serde_json::Value>, String> {
    if method != "GET" {
        return Ok(None);
    }
    let path = target.split('?').next().unwrap_or(target);
    match path {
        "/api/runtime" => Ok(Some(serde_json::to_value(runtime.snapshot()?).map_err(
            |error| format!("could not serialize installer runtime: {error}"),
        )?)),
        "/api/disks" => {
            let disk_snapshot = command_output(
                "/usr/bin/lsblk",
                &storage_lsblk_args(None)
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            )?;
            let ancestry_snapshot = command_output(
                "/usr/bin/lsblk",
                &[
                    "--json",
                    "--bytes",
                    "--paths",
                    "--output",
                    "NAME,PKNAME,TYPE",
                ],
            )?;
            let mut protected_sources = Vec::new();
            for mount in [
                "/",
                "/boot",
                "/boot/efi",
                "/sysroot",
                "/run/initramfs/live",
                "/run/initramfs/iso",
            ] {
                protected_sources.extend(findmnt_sources(mount, false)?);
            }
            protected_sources.extend(findmnt_sources("/run/initramfs", true)?);
            protected_sources.extend(findmnt_sources("/run/media", true)?);
            let current_source = findmnt_sources("/", false)?.into_iter().next();
            let records = installer_storage::runtime_disks_from_snapshots(
                &disk_snapshot,
                &ancestry_snapshot,
                &protected_sources,
                current_source.as_deref(),
            )?;
            let mut value = serde_json::to_value(records)
                .map_err(|error| format!("could not serialize disk inventory: {error}"))?;
            add_display_sizes(&mut value);
            Ok(Some(value))
        }
        "/api/partitions" => {
            let Some(disk) = query_value(target, "disk") else {
                return Ok(Some(serde_json::json!([])));
            };
            let disk = super::installer_plan::normalize_device_path(disk)
                .ok_or_else(|| "invalid disk query path".to_string())?;
            let args = storage_lsblk_args(Some(&disk));
            let snapshot = command_output(
                "/usr/bin/lsblk",
                &args.iter().map(String::as_str).collect::<Vec<_>>(),
            )?;
            let mut value =
                serde_json::to_value(installer_storage::parse_partitions(&snapshot)?)
                    .map_err(|error| format!("could not serialize partition inventory: {error}"))?;
            add_display_sizes(&mut value);
            Ok(Some(value))
        }
        "/api/free-space" => {
            let Some(disk) = query_value(target, "disk") else {
                return Ok(Some(serde_json::json!([])));
            };
            let disk = super::installer_plan::normalize_device_path(disk)
                .ok_or_else(|| "invalid disk query path".to_string())?;
            let args = storage_lsblk_args(Some(&disk));
            let snapshot = command_output(
                "/usr/bin/lsblk",
                &args.iter().map(String::as_str).collect::<Vec<_>>(),
            )?;
            let sector = command_output("/usr/bin/blockdev", &["--getss", &disk])?
                .trim()
                .parse::<u64>()
                .map_err(|_| "blockdev returned an invalid sector size".to_string())?;
            let mut value =
                serde_json::to_value(installer_storage::free_regions(&snapshot, &disk, sector)?)
                    .map_err(|error| {
                        format!("could not serialize free-space inventory: {error}")
                    })?;
            add_display_sizes(&mut value);
            Ok(Some(value))
        }
        _ => Ok(None),
    }
}

fn query_value<'a>(target: &'a str, name: &str) -> Option<&'a str> {
    target
        .split_once('?')?
        .1
        .split('&')
        .find_map(|part| part.strip_prefix(&format!("{name}=")))
}

fn rebuild_request(request: &[u8], body: &[u8]) -> Result<Vec<u8>, String> {
    let end = header_end(request)?;
    let headers = std::str::from_utf8(&request[..end - 4])
        .map_err(|_| "installer request headers are not UTF-8".to_string())?;
    let retained = headers
        .lines()
        .filter(|line| {
            line.split_once(':')
                .map(|(name, _)| !name.trim().eq_ignore_ascii_case("Content-Length"))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>()
        .join("\r\n");
    let mut rebuilt = format!("{retained}\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
    rebuilt.extend_from_slice(body);
    Ok(rebuilt)
}

/// Normalize only the pure storage portion of an authenticated start request.
///
/// The Python service still repeats live discovery, current-disk policy, user
/// acknowledgements, locale checks, and every destructive safety check. This
/// boundary owns the request shape and canonical device/region projection so
/// malformed paths and impossible guided-plan values never reach that worker.
fn normalize_start_request(request: &[u8]) -> Result<Vec<u8>, String> {
    let (method, target, _headers) = request_parts(request)?;
    let path = target.split('?').next().unwrap_or(target);
    if method != "POST" || path != "/api/start" {
        return Ok(request.to_vec());
    }

    let end = header_end(request)?;
    let mut value: serde_json::Value = serde_json::from_slice(&request[end..])
        .map_err(|error| format!("Invalid installer request JSON: {error}"))?;
    let input: InstallerPlanInput = serde_json::from_value(value.clone())
        .map_err(|error| format!("Invalid installer plan: {error}"))?;
    let plan = build_plan(input)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Installer start request must be a JSON object.".to_string())?;

    object.insert("disk".to_string(), serde_json::json!(plan.disk));
    object.insert("install_mode".to_string(), serde_json::json!(plan.mode));
    object.insert(
        "target_partition".to_string(),
        serde_json::json!(plan.target_partition.unwrap_or_default()),
    );
    object.insert(
        "resize_partition".to_string(),
        serde_json::json!(plan.resize_partition.unwrap_or_default()),
    );
    object.insert(
        "resize_gib".to_string(),
        serde_json::json!(plan.resize_bytes / (1024 * 1024 * 1024)),
    );
    object.insert(
        "free_region_start".to_string(),
        serde_json::json!(plan.free_region_start.unwrap_or_default()),
    );
    object.insert(
        "free_region_end".to_string(),
        serde_json::json!(plan.free_region_end.unwrap_or_default()),
    );
    let body = serde_json::to_vec(&value)
        .map_err(|error| format!("Could not serialize normalized installer request: {error}"))?;
    rebuild_request(request, &body)
}

/// Validate and construct the native executor at the daemon boundary.
///
/// This is deliberately separate from route handling so the eventual native
/// cutover can atomically install a request-specific supervisor only after all
/// typed plans have passed validation.
fn native_executor_from_start(request: &[u8]) -> Result<NativePhaseExecutor, String> {
    let (method, target, _headers) = request_parts(request)?;
    if method != "POST" || target.split('?').next().unwrap_or(target) != "/api/start" {
        return Err("native executor requires POST /api/start".to_string());
    }
    let end = header_end(request)?;
    let value: serde_json::Value = serde_json::from_slice(&request[end..])
        .map_err(|error| format!("Invalid installer request JSON: {error}"))?;
    NativePhaseExecutor::from_request(NativeInstallRequest::from_http(value)?)
}

fn native_request_from_start(request: &[u8]) -> Result<NativeInstallRequest, String> {
    let (method, target, _headers) = request_parts(request)?;
    if method != "POST" || target.split('?').next().unwrap_or(target) != "/api/start" {
        return Err("native request requires POST /api/start".to_string());
    }
    let end = header_end(request)?;
    let value: serde_json::Value = serde_json::from_slice(&request[end..])
        .map_err(|error| format!("Invalid installer request JSON: {error}"))?;
    NativeInstallRequest::from_http(value)
}

fn response_status(response: &[u8]) -> Option<u16> {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?;
    std::str::from_utf8(&response[..header_end])
        .ok()?
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

fn response_body(response: &[u8]) -> Option<&[u8]> {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?;
    Some(&response[header_end + 4..])
}

fn native_report(snapshot: &JobSnapshot) -> Result<serde_json::Value, String> {
    let mut value = serde_json::json!({
        "job_id": snapshot.job_id,
        "lifecycle": snapshot.runtime.lifecycle,
        "phase": snapshot.runtime.phase,
        "cancel_requested": snapshot.runtime.cancel_requested,
        "worker_active": snapshot.worker_active,
        "terminal_event_id": snapshot.terminal_event_id,
        "status": match snapshot.runtime.lifecycle {
            super::installer_runtime::Lifecycle::Done => "complete",
            super::installer_runtime::Lifecycle::Failed => "failed",
            super::installer_runtime::Lifecycle::Idle => "idle",
            _ => "installing",
        },
        "message": "Native installer job state",
    });
    if let Ok(contents) = fs::read_to_string(TRANSACTION_PATH) {
        if let Ok(transaction) = serde_json::from_str::<serde_json::Value>(&contents) {
            if transaction.is_object() {
                value = transaction;
            }
        }
    }
    Ok(value)
}

fn forward_buffered(mut backend: UnixStream, mut client: UnixStream) -> Result<Vec<u8>, String> {
    let mut response = Vec::new();
    backend
        .read_to_end(&mut response)
        .map_err(|error| format!("could not read installer response: {error}"))?;
    if response.len() > MAX_REQUEST_BYTES {
        return Err("installer response is too large".to_string());
    }
    client
        .write_all(&response)
        .map_err(|error| format!("could not forward installer response: {error}"))?;
    Ok(response)
}

fn forward_stream(
    mut backend: UnixStream,
    mut client: UnixStream,
    runtime: &RuntimeCoordinator,
) -> Result<(), String> {
    let mut buffer = [0_u8; 8192];
    let mut pending = String::new();
    loop {
        let count = backend
            .read(&mut buffer)
            .map_err(|error| format!("could not read installer event stream: {error}"))?;
        if count == 0 {
            return Ok(());
        }
        client
            .write_all(&buffer[..count])
            .map_err(|error| format!("could not forward installer event stream: {error}"))?;
        pending.push_str(&String::from_utf8_lossy(&buffer[..count]));
        while let Some(newline) = pending.find('\n') {
            let line = pending[..newline].trim_end_matches('\r').to_string();
            pending.drain(..newline + 1);
            if let Some(payload) = line.strip_prefix("data: ") {
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(payload.trim()) {
                    // The compatibility stream remains the wire format, but
                    // Rust is now the lifecycle authority that observes it.
                    let _ = runtime.event(&event);
                }
            }
        }
    }
}

fn native_stream(
    mut client: UnixStream,
    registry: &NativeJobRegistry,
    last_event_id: u64,
) -> Result<(), String> {
    client
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nX-Accel-Buffering: no\r\nConnection: close\r\n\r\n",
        )
        .map_err(|error| format!("could not write native stream headers: {error}"))?;
    let mut sent = last_event_id.saturating_add(1);
    loop {
        let replay = registry
            .replay(sent.saturating_sub(1))?
            .or_else(|| {
                Some(EventReplay {
                    events: Vec::new(),
                    next_event_id: sent,
                    reset_required: false,
                })
            })
            .expect("native stream replay fallback is always present");
        if replay.reset_required {
            sent = replay.events.first().map(|event| event.id).unwrap_or(sent);
        }
        for event in replay.events {
            let payload = serde_json::to_string(&event)
                .map_err(|error| format!("could not encode native installer event: {error}"))?;
            let frame = format!("id: {}\ndata: {payload}\n\n", event.id);
            client
                .write_all(frame.as_bytes())
                .and_then(|_| client.flush())
                .map_err(|error| format!("could not forward native installer event: {error}"))?;
            sent = event.id.saturating_add(1);
            if matches!(
                event.kind,
                super::installer_job::JobEventKind::Done
                    | super::installer_job::JobEventKind::Error { .. }
            ) {
                return Ok(());
            }
        }
        if let Some(replay) =
            registry.wait_for_events(sent.saturating_sub(1), Duration::from_secs(15))?
        {
            if replay.events.is_empty() && !replay.reset_required {
                client
                    .write_all(b":ka\n\n")
                    .and_then(|_| client.flush())
                    .map_err(|error| format!("could not write native stream keepalive: {error}"))?;
            }
        } else {
            client
                .write_all(b":ka\n\n")
                .and_then(|_| client.flush())
                .map_err(|error| format!("could not write native stream keepalive: {error}"))?;
        }
    }
}

fn handle(
    mut client: UnixStream,
    backend_path: &Path,
    token: &str,
    expected_uid: Option<u32>,
    runtime: Arc<RuntimeCoordinator>,
    native_registry: Arc<NativeJobRegistry>,
) -> Result<(), String> {
    if let Some(expected_uid) = expected_uid {
        if peer_uid(&client)? != expected_uid {
            forbidden(&mut client);
            return Ok(());
        }
    }
    let request = read_request(&mut client)?;
    let (method, target, headers) = request_parts(&request)?;
    if !route_allowed(method, target)
        || header_value(headers, "X-Kyth-Session-Token") != Some(token)
    {
        forbidden(&mut client);
        return Ok(());
    }
    if method == "GET" && target.split('?').next().unwrap_or(target) == "/api/runtime" {
        if let Some(snapshot) = native_registry.snapshot()? {
            let value = serde_json::to_value(snapshot.runtime).map_err(|error| {
                format!("could not serialize native installer runtime: {error}")
            })?;
            json_response(&mut client, "200 OK", &value);
            return Ok(());
        }
    }
    match read_only_storage_route(method, target, &runtime) {
        Ok(Some(value)) => {
            json_response(&mut client, "200 OK", &value);
            return Ok(());
        }
        Err(error)
            if method == "GET"
                && matches!(
                    target.split('?').next().unwrap_or(target),
                    "/api/disks" | "/api/partitions" | "/api/free-space"
                ) =>
        {
            json_response(
                &mut client,
                "503 Service Unavailable",
                &serde_json::json!({"error": error}),
            );
            return Ok(());
        }
        Ok(None) => {}
        Err(error) => return Err(error),
    }
    let request = match normalize_start_request(&request) {
        Ok(request) => request,
        Err(error) => {
            if method == "POST" && target.split('?').next().unwrap_or(target) == "/api/start" {
                bad_start_request(&mut client, &error);
                return Ok(());
            }
            return Err(error);
        }
    };
    let route = target.split('?').next().unwrap_or(target);
    if method == "GET" && route == "/api/report" {
        if let Some(snapshot) = native_registry.snapshot()? {
            let value = native_report(&snapshot)?;
            json_response(&mut client, "200 OK", &value);
            return Ok(());
        }
        let value = match fs::read_to_string(TRANSACTION_PATH) {
            Ok(contents) => serde_json::from_str(&contents)
                .map_err(|error| format!("could not decode installer transaction: {error}"))?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => serde_json::json!({}),
            Err(error) => return Err(format!("could not read installer transaction: {error}")),
        };
        json_response(&mut client, "200 OK", &value);
        return Ok(());
    }
    if method == "POST" && route == "/api/start" {
        let native_request = match native_request_from_start(&request) {
            Ok(request) => request,
            Err(error) => {
                bad_start_request(&mut client, &error);
                return Ok(());
            }
        };
        match native_registry.start(native_request) {
            Ok(receipt) => {
                json_response(
                    &mut client,
                    "200 OK",
                    &serde_json::json!({
                        "started": true,
                        "job_id": receipt.job_id,
                        "first_event_id": receipt.first_event_id,
                    }),
                );
            }
            Err(error) if error.contains("already running") => {
                json_response(
                    &mut client,
                    "409 Conflict",
                    &serde_json::json!({"started": false, "message": error}),
                );
            }
            Err(error) => {
                json_response(
                    &mut client,
                    "400 Bad Request",
                    &serde_json::json!({"started": false, "message": error}),
                );
            }
        }
        return Ok(());
    }
    if method == "POST" && route == "/api/cancel" {
        if native_registry.snapshot()?.is_none() {
            json_response(
                &mut client,
                "409 Conflict",
                &serde_json::json!({
                    "ok": false,
                    "message": "No installation is running to cancel."
                }),
            );
            return Ok(());
        }
        match native_registry.cancel() {
            Ok(()) => json_response(
                &mut client,
                "200 OK",
                &serde_json::json!({
                    "ok": true,
                    "message": "Cancellation requested."
                }),
            ),
            Err(error) => json_response(
                &mut client,
                "409 Conflict",
                &serde_json::json!({"ok": false, "message": error}),
            ),
        }
        return Ok(());
    }
    if method == "GET" && route == "/api/stream" {
        if native_registry.snapshot()?.is_some() {
            let last_event_id = header_value(headers, "Last-Event-ID")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            return native_stream(client, &native_registry, last_event_id);
        }
    }
    let mut backend = UnixStream::connect(backend_path).map_err(|error| {
        format!("could not connect to installer compatibility backend: {error}")
    })?;
    backend
        .write_all(&request)
        .map_err(|error| format!("could not forward installer request: {error}"))?;
    if method == "GET" && route == "/api/stream" {
        return forward_stream(backend, client, &runtime);
    }
    let response = match forward_buffered(backend, client) {
        Ok(response) => response,
        Err(error) => {
            if method == "POST" && route == "/api/start" {
                let _ = runtime.start_rejected();
            }
            if method == "POST" && route == "/api/cancel" {
                let _ = runtime.cancel_rejected();
            }
            return Err(error);
        }
    };
    let status = response_status(&response).unwrap_or(500);
    let body = response_body(&response).unwrap_or_default();
    if method == "POST" && route == "/api/start" {
        let started = serde_json::from_slice::<serde_json::Value>(body)
            .ok()
            .and_then(|value| value.get("started").and_then(serde_json::Value::as_bool))
            .unwrap_or(false);
        if status < 300 && started {
            runtime.start_accepted()?;
        } else {
            runtime.start_rejected()?;
        }
    }
    if method == "POST" && route == "/api/cancel" {
        let accepted = serde_json::from_slice::<serde_json::Value>(body)
            .ok()
            .and_then(|value| value.get("ok").and_then(serde_json::Value::as_bool))
            .unwrap_or(false);
        if !accepted {
            runtime.cancel_rejected()?;
        }
    }
    Ok(())
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_backend(token_path: &Path, backend_path: &Path) -> Result<ChildGuard, String> {
    let child = Command::new("/usr/bin/python3")
        .args(["-m", "kyth_installer.daemon", "--socket-path"])
        .arg(backend_path)
        .args(["--session-token-file"])
        .arg(token_path)
        .stdin(Stdio::null())
        .spawn()
        .map_err(|error| format!("could not start installer compatibility backend: {error}"))?;
    Ok(ChildGuard(child))
}

fn wait_for_backend(backend_path: &Path) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match UnixStream::connect(backend_path) {
            Ok(_) => return Ok(()),
            Err(_error) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                return Err(format!(
                    "installer compatibility backend did not start: {error}"
                ))
            }
        }
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    if unsafe { libc::geteuid() } != 0 {
        return Err("kyth-installerd must run as root".to_string());
    }
    let options = options(args)?;
    let token = read_session_token(&options.session_token_file)?;
    let backend_path = PathBuf::from(format!(
        "{}{}",
        options.socket_path.display(),
        BACKEND_SOCKET_SUFFIX
    ));
    let _backend = start_backend(&options.session_token_file, &backend_path)?;
    wait_for_backend(&backend_path)?;
    let listener = listener(&options)?;
    let runtime = Arc::new(RuntimeCoordinator::default());
    let native_registry = Arc::new(NativeJobRegistry::default());
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let token = token.clone();
                let expected_uid = options.peer_uid;
                let backend_path = backend_path.clone();
                let runtime = Arc::clone(&runtime);
                let native_registry = Arc::clone(&native_registry);
                thread::spawn(move || {
                    if let Err(error) = handle(
                        stream,
                        &backend_path,
                        &token,
                        expected_uid,
                        runtime,
                        native_registry,
                    ) {
                        eprintln!("installer request failed: {error}");
                    }
                });
            }
            Err(error) => return Err(format!("installer socket accept failed: {error}")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        native_executor_from_start, native_request_from_start, normalize_start_request, options,
        read_session_token, route_allowed,
    };
    use serde_json::Value;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    #[test]
    fn options_require_the_native_socket_boundary() {
        let parsed = options(&[
            "--socket-path".into(),
            "/run/kyth-installer/api.sock".into(),
            "--session-token-file".into(),
            "/run/kyth-installer/session-token".into(),
        ])
        .unwrap();
        assert_eq!(
            parsed.socket_path.to_str(),
            Some("/run/kyth-installer/api.sock")
        );
        assert!(parsed.socket_group.is_none());
    }

    #[test]
    fn route_allowlist_excludes_arbitrary_execution() {
        assert!(route_allowed("GET", "/api/disks"));
        assert!(route_allowed("POST", "/api/start"));
        assert!(!route_allowed("POST", "/api/exec"));
        assert!(!route_allowed("GET", "http://127.0.0.1:7777/api/disks"));
    }

    #[test]
    fn token_reader_rejects_loose_modes_and_bad_format() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("token");
        fs::write(&path, "short").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_session_token(&path).is_err());
        fs::write(&path, "A".repeat(43)).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        assert!(read_session_token(&path).is_err());
    }

    fn start_request(body: &str) -> Vec<u8> {
        format!(
            "POST /api/start HTTP/1.1\r\nHost: installer\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
        .into_bytes()
    }

    #[test]
    fn normalizes_start_request_before_forwarding() {
        let request = start_request(
            r#"{"disk":" sda ","install_mode":" Wipe ","resize_gib":0,"free_region_start":0,"free_region_end":0,"username":"alice","password":"secret"}"#,
        );
        let normalized = normalize_start_request(&request).expect("start request should normalize");
        let end = normalized
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap()
            + 4;
        let body: Value =
            serde_json::from_slice(&normalized[end..]).expect("normalized body is JSON");
        assert_eq!(body["disk"], "/dev/sda");
        assert_eq!(body["install_mode"], "wipe");
        assert_eq!(body["target_partition"], "");
        assert_eq!(body["resize_partition"], "");
        assert_eq!(body["resize_gib"], 0);
        assert!(String::from_utf8_lossy(&normalized)
            .contains(&format!("Content-Length: {}", normalized.len() - end)));
    }

    #[test]
    fn rejects_invalid_start_plan_before_forwarding() {
        let request = start_request(r#"{"disk":"../../etc/passwd","install_mode":"wipe"}"#);
        let error = normalize_start_request(&request).expect_err("unsafe disk must fail closed");
        assert!(error.contains("target disk"), "{error}");
    }

    #[test]
    fn leaves_other_routes_byte_for_byte_unchanged() {
        let request = b"GET /api/disks HTTP/1.1\r\nHost: installer\r\nContent-Length: 0\r\n\r\n";
        assert_eq!(normalize_start_request(request).unwrap(), request);
    }

    #[test]
    fn native_start_boundary_builds_typed_executor() {
        let request = start_request(
            r#"{"disk":"sda","install_mode":"wipe","source_imgref":"oci:/image","target_imgref":"kyth:latest","hostname":"kyth"}"#,
        );
        let executor = native_executor_from_start(&request).expect("native plan should validate");
        assert_eq!(executor.storage_plan().disk, "/dev/sda");
        assert_eq!(executor.execution_plan().bootc.target, "/dev/sda");
    }

    #[test]
    fn native_start_boundary_keeps_start_route_native() {
        let request = start_request(
            r#"{"disk":"sda","install_mode":"wipe","username":"alice","password_hash":"$6$hash"}"#,
        );
        let native = native_request_from_start(&request).expect("native request should decode");
        assert_eq!(native.storage.disk, "sda");
        assert_eq!(native.execution.account.unwrap().username, "alice");
    }

    #[test]
    fn native_job_registry_starts_empty_and_rejects_cancel_without_job() {
        let registry = super::NativeJobRegistry::default();
        assert!(registry.snapshot().unwrap().is_none());
        assert!(registry.replay(0).unwrap().is_none());
        assert_eq!(
            registry.cancel().unwrap_err(),
            "No installation is running to cancel."
        );
    }
}
