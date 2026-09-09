//! Rust-owned VM acceptance guest lifecycle.
//!
//! The source-tree Python module is retained as a parity fixture. The
//! packaged run entry point uses this module so firmware gating, validation,
//! bounded execution, destructive operations, state persistence, and failure
//! reporting have one Rust owner.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::atomic_io::atomic_write_text;
use crate::system::process::{redact_sensitive_text, run_bounded};

const FW_CFG_ROOT: &str = "/sys/firmware/qemu_fw_cfg/by_name/opt/com.kyth";
const STATE_DIR: &str = "/var/lib/kyth/vm-acceptance";
const STATE_FILE: &str = "/var/lib/kyth/vm-acceptance/state";
const INITIAL_DIGEST_FILE: &str = "/var/lib/kyth/vm-acceptance/initial-digest";
const SERIAL_DEVICE: &str = "/dev/ttyS0";
const LOG_FILE: &str = "/var/log/kyth-vm-acceptance.log";
const TARGET_BY_ID: &str = "/dev/disk/by-id/virtio-KYTH_ACCEPT";
const INSTALLER_ENV_FILE: &str = "/etc/kyth-installer.env";
const HUB_BINARY: &str = "/usr/bin/kyth-hub-shell";
const HUB_ROUTE_MANIFEST: &str = "/usr/share/kyth/hubRoutes.json";
const HUB_ACCEPTANCE_TIMEOUT: Duration = Duration::from_secs(45);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
const UPDATE_REF_MAX: usize = 512;

pub fn valid_update_ref(value: &str) -> bool {
    value.is_empty()
        || (value.len() <= UPDATE_REF_MAX
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b'.' | b'_' | b'/' | b'@' | b':' | b'+' | b'-')
            }))
}

pub fn read_trimmed(path: impl AsRef<Path>) -> String {
    fs::read(path)
        .unwrap_or_default()
        .into_iter()
        .filter(|byte| *byte != 0)
        .map(char::from)
        .collect::<String>()
        .trim()
        .to_owned()
}

pub fn enabled() -> bool {
    read_trimmed(format!("{FW_CFG_ROOT}/acceptance/raw")) == "1"
}

pub fn update_ref() -> String {
    read_trimmed(format!("{FW_CFG_ROOT}/update-ref/raw"))
}

pub fn booted_digest_from_json(output: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(output).ok()?;
    let image = value.get("status")?.get("booted")?.get("image")?;
    image
        .get("imageDigest")
        .and_then(Value::as_str)
        .or_else(|| {
            image
                .get("image")
                .and_then(|nested| nested.get("imageDigest"))
                .and_then(Value::as_str)
        })
        .filter(|digest| !digest.is_empty())
        .map(str::to_owned)
}

pub fn deployment_count_from_json(output: &str) -> usize {
    let Ok(value) = serde_json::from_str::<Value>(output) else {
        return 0;
    };
    match value {
        Value::Array(items) => items.len(),
        Value::Object(object) => object
            .get("deployments")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        _ => 0,
    }
}

pub fn acceptance_state_from_text(value: Option<&str>) -> &str {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some("fresh") | None => "fresh",
        Some("update-staged") => "update-staged",
        Some("rollback-staged") => "rollback-staged",
        Some(_) => "unknown",
    }
}

pub fn acceptance_event(phase: &str, detail: &str) -> String {
    format!("KYTH_ACCEPTANCE:{phase}:{}", detail.replace('\n', " "))
}

fn emit(phase: &str, detail: impl AsRef<str>) {
    let line = redact_sensitive_text(&acceptance_event(phase, detail.as_ref()));
    println!("{line}");
    if let Ok(mut log) = OpenOptions::new().create(true).append(true).open(LOG_FILE) {
        let _ = writeln!(log, "{line}");
    }
    if let Ok(mut serial) = OpenOptions::new().write(true).open(SERIAL_DEVICE) {
        let _ = writeln!(serial, "{line}");
    }
}

fn power(action: &str) {
    let _ = run_bounded(
        &["systemctl".into(), action.into(), "--no-block".into()],
        Duration::from_secs(30),
    );
}

fn log_output(output: &Output) {
    let text = redact_sensitive_text(&format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ));
    if !text.is_empty() {
        if let Ok(mut log) = OpenOptions::new().create(true).append(true).open(LOG_FILE) {
            let _ = writeln!(log, "{text}");
        }
    }
}

fn run_logged(argv: &[String], timeout: Duration, failure: &str) -> io::Result<Output> {
    let output = run_bounded(argv, timeout)
        .map_err(|error| io::Error::new(error.kind(), format!("{failure}: {error}")))?;
    log_output(&output);
    output
        .status
        .success()
        .then_some(output)
        .ok_or_else(|| io::Error::other(failure))
}

fn wait_for_desktop(mode: &str) -> bool {
    for _ in 0..90 {
        let argv = if mode == "live" {
            vec!["pgrep".into(), "-x".into(), "plasmashell".into()]
        } else {
            vec![
                "systemctl".into(),
                "is-active".into(),
                "--quiet".into(),
                "display-manager.service".into(),
            ]
        };
        if run_bounded(&argv, Duration::from_secs(5)).is_ok_and(|output| output.status.success()) {
            return true;
        }
        thread::sleep(Duration::from_secs(2));
    }
    false
}

fn booted_digest() -> String {
    run_bounded(
        &[
            "bootc".into(),
            "status".into(),
            "--format".into(),
            "json".into(),
        ],
        Duration::from_secs(30),
    )
    .ok()
    .filter(|output| output.status.success())
    .and_then(|output| booted_digest_from_json(&String::from_utf8_lossy(&output.stdout)))
    .unwrap_or_default()
}

fn deployment_count() -> usize {
    run_bounded(
        &[
            "ostree".into(),
            "admin".into(),
            "status".into(),
            "--json".into(),
        ],
        Duration::from_secs(30),
    )
    .ok()
    .filter(|output| output.status.success())
    .map(|output| deployment_count_from_json(&String::from_utf8_lossy(&output.stdout)))
    .unwrap_or(0)
}

fn smoke_check(phase: &str) -> io::Result<()> {
    let output = run_logged(
        &["/usr/bin/kyth-smoke-check".into(), "--verbose".into()],
        Duration::from_secs(300),
        &format!("{phase} smoke check failed"),
    )?;
    if let Ok(data) = fs::read(LOG_FILE) {
        if let Ok(mut serial) = OpenOptions::new().write(true).open(SERIAL_DEVICE) {
            let _ = serial.write_all(&data);
        }
    }
    if output.status.code().unwrap_or(1) >= 2 {
        return Err(io::Error::other(format!(
            "{phase} smoke check reported failed invariants"
        )));
    }
    emit(
        &format!("{phase}_SMOKE_OK"),
        format!("warnings-allowed={}", output.status.code().unwrap_or(0)),
    );
    Ok(())
}

fn installer_target_ref() -> String {
    fs::read_to_string(INSTALLER_ENV_FILE)
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                line.strip_prefix("KYTH_TARGET_IMAGE=")
                    .map(|value| value.trim().trim_matches(['\'', '"']).to_owned())
            })
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "ghcr.io/kyth-os/kyth:testing".into())
}

#[cfg(unix)]
fn is_block_device(path: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    fs::metadata(path).is_ok_and(|metadata| metadata.file_type().is_block_device())
}

fn install_from_live_iso() -> io::Result<()> {
    if !wait_for_desktop("live") {
        return Err(io::Error::other("live Plasma desktop did not become ready"));
    }
    emit("LIVE_READY", "plasmashell-active");
    smoke_check("LIVE")?;
    let target = Path::new(TARGET_BY_ID).canonicalize().map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "dedicated acceptance disk not found",
        )
    })?;
    if !is_block_device(&target) {
        return Err(io::Error::other(
            "acceptance disk symlink did not resolve to a block device",
        ));
    }
    if !Path::new("/usr/share/kyth/image").is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "bundled OCI image is missing from live media",
        ));
    }
    let reference = update_ref();
    if !valid_update_ref(&reference) {
        return Err(io::Error::other(
            "update image reference contains unsupported characters",
        ));
    }
    let target_ref = installer_target_ref();
    emit("INSTALL_STARTED", target.display().to_string());
    run_logged(
        &[
            "bootc".into(),
            "install".into(),
            "to-disk".into(),
            "--source-imgref".into(),
            "oci:/usr/share/kyth/image:latest".into(),
            "--target-imgref".into(),
            target_ref.clone(),
            "--filesystem".into(),
            "btrfs".into(),
            "--wipe".into(),
            "--skip-fetch-check".into(),
            target.display().to_string(),
        ],
        Duration::from_secs(1800),
        "bootc install to-disk failed",
    )?;
    emit("INSTALL_COMPLETE", target_ref);
    power("reboot");
    Ok(())
}

fn state_value() -> String {
    acceptance_state_from_text(Some(&read_trimmed(STATE_FILE))).to_owned()
}

fn initial_digest() -> io::Result<String> {
    let value = read_trimmed(INITIAL_DIGEST_FILE);
    if value.is_empty() {
        Err(io::Error::other("initial deployment digest is missing"))
    } else {
        Ok(value)
    }
}

fn write_state(value: &str) -> io::Result<()> {
    atomic_write_text(STATE_FILE, &format!("{value}\n"), Some(0o600))
}

fn active_graphical_session() -> Option<(String, Vec<(String, String)>)> {
    let seat = run_bounded(
        &[
            "loginctl".into(),
            "show-seat".into(),
            "seat0".into(),
            "-p".into(),
            "ActiveSession".into(),
            "--value".into(),
        ],
        Duration::from_secs(5),
    )
    .ok()?;
    let session = String::from_utf8_lossy(&seat.stdout).trim().to_owned();
    if session.is_empty() {
        return None;
    }
    let owner = run_bounded(
        &[
            "loginctl".into(),
            "show-session".into(),
            session,
            "-p".into(),
            "Name".into(),
            "--value".into(),
        ],
        Duration::from_secs(5),
    )
    .ok()?;
    let username = String::from_utf8_lossy(&owner.stdout).trim().to_owned();
    if username.is_empty() {
        return None;
    }
    let uid = run_bounded(
        &["id".into(), "-u".into(), username.clone()],
        Duration::from_secs(5),
    )
    .ok()?;
    let uid = String::from_utf8_lossy(&uid.stdout)
        .trim()
        .parse::<u32>()
        .ok()?;
    let runtime = PathBuf::from(format!("/run/user/{uid}"));
    let wayland = fs::read_dir(&runtime).ok()?.flatten().find_map(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        name.starts_with("wayland-").then_some(name)
    });
    let x11 = fs::read_dir("/tmp/.X11-unix")
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .find_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with('X').then_some(name)
        });
    if wayland.is_none() && x11.is_none() {
        return None;
    }
    let home = fs::read_to_string("/etc/passwd")
        .ok()?
        .lines()
        .find_map(|line| {
            let fields: Vec<_> = line.split(':').collect();
            (fields.first() == Some(&username.as_str()) && fields.len() > 5)
                .then_some(fields[5].to_owned())
        })?;
    let mut environment = vec![
        ("HOME".into(), home.clone()),
        ("LOGNAME".into(), username.clone()),
        ("USER".into(), username.clone()),
        ("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into()),
        ("XDG_RUNTIME_DIR".into(), runtime.display().to_string()),
        ("XDG_CURRENT_DESKTOP".into(), "KDE".into()),
        (
            "DBUS_SESSION_BUS_ADDRESS".into(),
            format!("unix:path={}/bus", runtime.display()),
        ),
    ];
    if let Some(value) = wayland {
        environment.push(("WAYLAND_DISPLAY".into(), value));
    }
    if let Some(value) = x11 {
        environment.push((
            "DISPLAY".into(),
            format!(":{}", value.trim_start_matches('X')),
        ));
    }
    let xauthority = Path::new(&home).join(".Xauthority");
    if xauthority.is_file() {
        environment.push(("XAUTHORITY".into(), xauthority.display().to_string()));
    }
    Some((username, environment))
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                vec![byte as char]
            } else {
                format!("%{byte:02X}").chars().collect()
            }
        })
        .collect()
}

fn hub_pages() -> io::Result<Vec<(String, String)>> {
    let manifest: Value = serde_json::from_str(&fs::read_to_string(HUB_ROUTE_MANIFEST)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let destinations = manifest
        .get("destinations")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("Hub route manifest has no destinations"))?;
    let mut pages = vec![("Welcome".to_owned(), "/".to_owned())];
    for destination in destinations {
        let key = destination
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other("Hub destination has no key"))?;
        let route = destination
            .get("route")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other("Hub destination has no route"))?;
        pages.push((key.to_owned(), route.to_owned()));
        if let Some(sections) = destination.get("sections").and_then(Value::as_array) {
            for section in sections {
                let section_key = section
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| io::Error::other("Hub section has no key"))?;
                pages.push((
                    section_key.to_owned(),
                    format!("{route}?section={}", percent_encode(section_key)),
                ));
            }
        }
    }
    let mut keys = std::collections::HashSet::new();
    if pages.iter().any(|(key, _)| !keys.insert(key)) {
        return Err(io::Error::other(
            "Hub route manifest contains duplicate page keys",
        ));
    }
    Ok(pages)
}

fn hub_start(
    username: &str,
    environment: &[(String, String)],
    page: &str,
    evidence: &Path,
    degraded: bool,
) -> io::Result<Child> {
    let output = File::create(evidence.with_extension("process.log"))?;
    let mut command = Command::new("runuser");
    command.args(["-u", username, "--", "env"]);
    command.arg(format!("KYTH_HUB_ACCEPTANCE_FILE={}", evidence.display()));
    if degraded {
        command.arg("KYTH_HUB_ACCEPTANCE_DEGRADED=1");
    }
    for (key, value) in environment {
        if !value.is_empty() {
            command.arg(format!("{key}={value}"));
        }
    }
    command.args([HUB_BINARY, "--page", page]);
    command
        .stdout(Stdio::from(output.try_clone()?))
        .stderr(Stdio::from(output));
    command.spawn()
}

fn hub_stop(child: &mut Child) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    let _ = child.kill();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(8) {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn hub_event(evidence: &Path, event: &str) -> Option<serde_json::Map<String, Value>> {
    let prefix = format!("KYTH_HUB_ACCEPTANCE:{event}:");
    fs::read_to_string(evidence)
        .ok()?
        .lines()
        .rev()
        .find_map(|line| {
            let payload = line.strip_prefix(&prefix)?;
            match serde_json::from_str::<Value>(payload).ok()? {
                Value::Object(object) => Some(object),
                _ => None,
            }
        })
}

fn wait_hub_event(evidence: &Path, event: &str) -> Option<serde_json::Map<String, Value>> {
    let deadline = Instant::now() + HUB_ACCEPTANCE_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(value) = hub_event(evidence, event) {
            return Some(value);
        }
        thread::sleep(Duration::from_millis(500));
    }
    None
}

fn hub_launch_check(
    username: &str,
    environment: &[(String, String)],
    page: &str,
    route: &str,
    evidence: &Path,
) -> bool {
    let _ = fs::remove_file(evidence);
    let Ok(mut child) = hub_start(username, environment, page, evidence, false) else {
        return false;
    };
    let result = wait_hub_event(evidence, "deep-link").is_some_and(|event| {
        event.get("page").and_then(Value::as_str) == Some(page)
            && event.get("route").and_then(Value::as_str) == Some(route)
            && event.get("source").and_then(Value::as_str) == Some("initial")
    });
    hub_stop(&mut child);
    result
}

fn ensure_graphical_session() -> Option<(String, Vec<(String, String)>)> {
    if let Some(session) = active_graphical_session() {
        return Some(session);
    }
    let username = "kyth-acceptance";
    let account_missing = run_bounded(
        &["id".into(), "-u".into(), username.into()],
        Duration::from_secs(5),
    )
    .map_or(true, |output| !output.status.success());
    if account_missing
        && !run_bounded(
            &[
                "useradd".into(),
                "--create-home".into(),
                "--shell".into(),
                "/bin/bash".into(),
                username.into(),
            ],
            Duration::from_secs(30),
        )
        .is_ok_and(|output| output.status.success())
    {
        return None;
    }
    atomic_write_text(
        "/etc/plasmalogin.conf.d/90-kyth-vm-acceptance.conf",
        "[Autologin]\nUser=kyth-acceptance\nSession=plasma.desktop\nRelogin=true\n",
        Some(0o644),
    )
    .ok()?;
    let restart = run_bounded(
        &[
            "systemctl".into(),
            "restart".into(),
            "display-manager.service".into(),
            "--no-block".into(),
        ],
        Duration::from_secs(30),
    )
    .ok()?;
    if !restart.status.success() {
        return None;
    }
    for _ in 0..60 {
        if let Some(session) = active_graphical_session() {
            return Some(session);
        }
        thread::sleep(Duration::from_secs(1));
    }
    None
}

fn run_hub_acceptance() -> io::Result<()> {
    let (username, environment) = ensure_graphical_session().ok_or_else(|| {
        io::Error::other("active graphical session for installed Hub acceptance was not found")
    })?;
    let evidence = PathBuf::from(format!("/tmp/kyth-hub-acceptance-{}.log", unsafe {
        libc::geteuid()
    }));
    emit("HUB_BINARY_OK", HUB_BINARY);
    let pages = hub_pages()?;
    for (page, route) in &pages {
        if !hub_launch_check(&username, &environment, page, route, &evidence) {
            return Err(io::Error::other(format!(
                "Hub --page deep link failed for {page:?}"
            )));
        }
    }
    emit(
        "HUB_DEEP_LINKS_OK",
        pages
            .iter()
            .map(|(page, route)| format!("{page}={route}"))
            .collect::<Vec<_>>()
            .join("; "),
    );
    let _ = fs::remove_file(&evidence);
    let mut first = hub_start(&username, &environment, "Welcome", &evidence, false)?;
    if wait_hub_event(&evidence, "deep-link").is_none() {
        hub_stop(&mut first);
        return Err(io::Error::other("Hub first launch did not resolve Welcome"));
    }
    let _ = fs::remove_file(&evidence);
    let mut second = hub_start(&username, &environment, "Updates", &evidence, false)?;
    let forwarded = wait_hub_event(&evidence, "deep-link").is_some_and(|event| {
        event.get("page").and_then(Value::as_str) == Some("Updates")
            && event.get("source").and_then(Value::as_str) == Some("single-instance")
    });
    hub_stop(&mut second);
    hub_stop(&mut first);
    if !forwarded {
        return Err(io::Error::other(
            "Hub second launch did not forward the Updates page",
        ));
    }
    emit(
        "HUB_SECOND_LAUNCH_OK",
        "Updates forwarded to the existing Hub process",
    );
    let _ = fs::remove_file(&evidence);
    let mut degraded = hub_start(&username, &environment, "Welcome", &evidence, true)?;
    let dashboard = wait_hub_event(&evidence, "dashboard");
    hub_stop(&mut degraded);
    if dashboard
        .as_ref()
        .and_then(|event| event.get("state"))
        .and_then(Value::as_str)
        != Some("degraded")
        || dashboard
            .as_ref()
            .and_then(|event| event.get("label"))
            .and_then(Value::as_str)
            != Some("Status unavailable")
    {
        return Err(io::Error::other(
            "Hub dashboard did not report its unavailable-data state honestly",
        ));
    }
    emit("HUB_DASHBOARD_DEGRADED_OK", "Status unavailable");
    let _ = fs::remove_file(&evidence);
    let mut updates = hub_start(&username, &environment, "Updates", &evidence, false)?;
    let update_probe = wait_hub_event(&evidence, "updates-probe");
    let privilege_probe = wait_hub_event(&evidence, "privileged-failure");
    hub_stop(&mut updates);
    let update_state = update_probe
        .as_ref()
        .and_then(|event| event.get("state"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if !matches!(update_state, "ok" | "degraded")
        || privilege_probe
            .as_ref()
            .and_then(|event| event.get("state"))
            .and_then(Value::as_str)
            != Some("expected")
    {
        return Err(io::Error::other(
            "Hub Updates page did not report its probes honestly",
        ));
    }
    emit("HUB_UPDATES_OK", update_state);
    emit("HUB_PRIVILEGED_FAILURE_OK", "allowlist rejection surfaced");
    Ok(())
}

fn installed_lifecycle() -> io::Result<()> {
    fs::create_dir_all(STATE_DIR)?;
    let reference = update_ref();
    if !valid_update_ref(&reference) {
        return Err(io::Error::other(
            "update image reference contains unsupported characters",
        ));
    }
    if !wait_for_desktop("installed") {
        return Err(io::Error::other(
            "installed display manager did not become ready",
        ));
    }
    match state_value().as_str() {
        "fresh" => {
            emit("INSTALLED_READY", "display-manager-active");
            smoke_check("INSTALLED")?;
            if !Path::new(HUB_BINARY).is_file() {
                return Err(io::Error::other(
                    "installed Rust/Tauri Hub binary is missing",
                ));
            }
            if !Path::new(HUB_ROUTE_MANIFEST).is_file() {
                return Err(io::Error::other("Hub route manifest is missing"));
            }
            run_hub_acceptance()?;
            let initial = booted_digest();
            if initial.is_empty() {
                return Err(io::Error::other("could not read initial booted digest"));
            }
            atomic_write_text(INITIAL_DIGEST_FILE, &format!("{initial}\n"), Some(0o600))?;
            if reference.is_empty() {
                emit("COMPLETE", "install-only");
                power("poweroff");
            } else {
                write_state("update-staged")?;
                emit("UPDATE_STARTED", &reference);
                run_logged(
                    &["bootc".into(), "switch".into(), reference.clone()],
                    COMMAND_TIMEOUT,
                    "bootc switch failed",
                )?;
                emit("UPDATE_STAGED", &reference);
                power("reboot");
            }
        }
        "update-staged" => {
            let initial = initial_digest()?;
            let current = booted_digest();
            if current.is_empty() || current == initial {
                return Err(io::Error::other(
                    "updated deployment did not boot a different digest",
                ));
            }
            if deployment_count() < 2 {
                return Err(io::Error::other(
                    "updated system does not expose a rollback deployment",
                ));
            }
            emit("UPDATE_BOOTED", &current);
            smoke_check("UPDATE")?;
            write_state("rollback-staged")?;
            run_logged(
                &["bootc".into(), "rollback".into()],
                COMMAND_TIMEOUT,
                "bootc rollback failed",
            )?;
            emit("ROLLBACK_STAGED", initial);
            power("reboot");
        }
        "rollback-staged" => {
            let initial = initial_digest()?;
            let current = booted_digest();
            if current.is_empty() || current != initial {
                return Err(io::Error::other(
                    "rollback did not restore the initial deployment digest",
                ));
            }
            emit("ROLLBACK_BOOTED", &current);
            smoke_check("ROLLBACK")?;
            emit("COMPLETE", "update-and-rollback");
            let _ = fs::remove_file(STATE_FILE);
            power("poweroff");
        }
        state => {
            return Err(io::Error::other(format!(
                "unknown acceptance state: {state}"
            )))
        }
    }
    Ok(())
}

pub fn run() -> io::Result<ExitCode> {
    if !enabled() {
        return Ok(ExitCode::SUCCESS);
    }
    let result = if Path::new(INSTALLER_ENV_FILE).is_file() {
        install_from_live_iso()
    } else {
        installed_lifecycle()
    };
    match result {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(error) => {
            emit("FAILED", error.to_string());
            power("poweroff");
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_restricted_update_refs() {
        assert!(valid_update_ref(""));
        assert!(valid_update_ref("ghcr.io/kyth-os/kyth:testing@sha256:abc"));
        assert!(!valid_update_ref("ghcr.io/kyth;poweroff"));
        assert!(!valid_update_ref("image with spaces"));
        assert!(!valid_update_ref(&"a".repeat(UPDATE_REF_MAX + 1)));
    }

    #[test]
    fn decodes_bootc_and_ostree_shapes() {
        assert_eq!(
            booted_digest_from_json(
                "{\"status\":{\"booted\":{\"image\":{\"imageDigest\":\"sha256:abc\"}}}}"
            ),
            Some("sha256:abc".into())
        );
        assert_eq!(booted_digest_from_json("{\"status\":{\"booted\":{\"image\":{\"image\":{\"imageDigest\":\"sha256:nested\"}}}}}"), Some("sha256:nested".into()));
        assert_eq!(deployment_count_from_json("[{\"id\":1},{\"id\":2}]"), 2);
        assert_eq!(
            deployment_count_from_json("{\"deployments\":[{\"id\":1}] }"),
            1
        );
    }

    #[test]
    fn normalizes_state_and_events() {
        assert_eq!(acceptance_state_from_text(None), "fresh");
        assert_eq!(acceptance_state_from_text(Some("bad")), "unknown");
        assert_eq!(
            acceptance_event("FAILED", "line one\nline two"),
            "KYTH_ACCEPTANCE:FAILED:line one line two"
        );
    }
}
