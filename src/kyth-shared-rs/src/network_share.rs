//! Native root-side executor for the fixed SMB share protocol.
//!
//! The Tauri shell and the root privileged socket validate the request shape,
//! but this binary is the final authority for the files and systemd units it
//! writes. Credentials are accepted only on stdin and are written to a
//! root-owned mode-0600 file; they never enter an argv vector or unit text.

use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

pub const CREDENTIALS_DIR: &str = "/etc/kyth/smb-credentials";
pub const UNIT_DIR: &str = "/etc/systemd/system";
const MAX_PAYLOAD: usize = 64 * 1024;
const SAFE_MOUNT_PREFIXES: &[&str] = &["/mnt/", "/media/", "/run/media/", "/home/"];

fn plain<'a>(
    value: Option<&'a Value>,
    field: &str,
    allow_empty: bool,
    maximum: usize,
) -> Result<&'a str, String> {
    let value = value
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{field} must be text"))?;
    if (!allow_empty && value.is_empty())
        || value.len() > maximum
        || value.chars().any(|character| character.is_control())
    {
        return Err(format!("invalid {field}"));
    }
    Ok(value)
}

fn normalized_mount_point(value: &str) -> String {
    let mut output = PathBuf::new();
    for component in Path::new(value).components() {
        match component {
            Component::RootDir => output.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                output.pop();
            }
            Component::Normal(part) => output.push(part),
            Component::Prefix(_) => {}
        }
    }
    output.to_string_lossy().into_owned()
}

pub fn validate_mount_point(value: Option<&Value>) -> Result<String, String> {
    let raw = plain(value, "mount_point", false, 4096)?;
    let path = normalized_mount_point(raw);
    if path.is_empty()
        || !path.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b' ' | b'-')
        })
        || !SAFE_MOUNT_PREFIXES
            .iter()
            .any(|prefix| path.starts_with(prefix))
    {
        return Err("mount_point is outside an approved prefix".into());
    }
    Ok(path)
}

fn validate_name(value: Option<&Value>) -> Result<String, String> {
    let name = plain(value, "name", false, 64)?;
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err("name contains unsupported characters".into());
    }
    Ok(name.to_string())
}

fn validate_server(value: Option<&Value>) -> Result<String, String> {
    let server = plain(value, "server", false, 253)?;
    if !server
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err("server is not a hostname or IP address".into());
    }
    Ok(server.to_string())
}

fn validate_share_path(value: Option<&Value>) -> Result<String, String> {
    let share_path = plain(value, "share_path", false, 4096)?.trim_start_matches('/');
    if share_path.is_empty()
        || share_path.starts_with('.')
        || share_path.contains("//")
        || share_path.contains('%')
        || share_path.split('/').any(|part| part == "..")
    {
        return Err("share_path is invalid".into());
    }
    Ok(share_path.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareRequest {
    pub name: String,
    pub server: Option<String>,
    pub share_path: Option<String>,
    pub mount_point: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub domain: Option<String>,
    pub auto_mount: bool,
    pub mount_now: bool,
    pub uid: u32,
    pub gid: u32,
}

pub fn validate_request(payload: &Value, adding: bool) -> Result<ShareRequest, String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "share payload must be an object".to_string())?;
    let name = validate_name(object.get("name"))?;
    let mount_point = validate_mount_point(object.get("mount_point"))?;
    let uid = if adding {
        object
            .get("uid")
            .and_then(Value::as_u64)
            .ok_or_else(|| "uid must be a non-negative integer".to_string())?
    } else {
        0
    };
    let gid = if adding {
        object
            .get("gid")
            .and_then(Value::as_u64)
            .ok_or_else(|| "gid must be a non-negative integer".to_string())?
    } else {
        0
    };
    if uid > u32::MAX as u64 || gid > u32::MAX as u64 {
        return Err("uid and gid are out of range".into());
    }
    if !adding {
        return Ok(ShareRequest {
            name,
            server: None,
            share_path: None,
            mount_point,
            username: None,
            password: None,
            domain: None,
            auto_mount: false,
            mount_now: false,
            uid: uid as u32,
            gid: gid as u32,
        });
    }
    let auto_mount = object
        .get("auto_mount")
        .and_then(Value::as_bool)
        .ok_or_else(|| "auto_mount must be boolean".to_string())?;
    let mount_now = object
        .get("mount_now")
        .and_then(Value::as_bool)
        .ok_or_else(|| "mount_now must be boolean".to_string())?;
    Ok(ShareRequest {
        name,
        server: Some(validate_server(object.get("server"))?),
        share_path: Some(validate_share_path(object.get("share_path"))?),
        mount_point,
        username: Some(plain(object.get("username"), "username", false, 256)?.to_string()),
        password: Some(plain(object.get("password"), "password", true, 4096)?.to_string()),
        domain: Some(plain(object.get("domain"), "domain", true, 256)?.to_string()),
        auto_mount,
        mount_now,
        uid: uid as u32,
        gid: gid as u32,
    })
}

fn safe_artifact_path(directory: &Path, name: &str) -> PathBuf {
    directory.join(name)
}

fn ensure_no_symlink_in_path(path: &Path) -> Result<(), String> {
    let mut current = path.to_path_buf();
    loop {
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "mount_point traverses a symlink: {}",
                    current.display()
                ))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        if !current.pop() {
            break;
        }
    }
    Ok(())
}

fn mount_unit(mount_point: &str) -> Result<String, String> {
    let argv = vec![
        "systemd-escape".into(),
        "--path".into(),
        "--suffix=mount".into(),
        mount_point.into(),
    ];
    let output = crate::system::process::run_bounded(&argv, Duration::from_secs(5))
        .map_err(|error| format!("could not determine mount unit: {error}"))?;
    if !output.status.success() {
        return Err("systemd-escape failed".into());
    }
    let unit = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if unit.is_empty()
        || !unit.ends_with(".mount")
        || !unit.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'@' | b'\\' | b'-')
        })
    {
        return Err("systemd-escape returned an invalid mount unit".into());
    }
    Ok(unit)
}

fn atomic_write(path: &Path, content: &str, mode: u32) -> Result<(), String> {
    if path.is_symlink() {
        return Err(format!("refusing to replace symlink: {}", path.display()));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if parent == Path::new(CREDENTIALS_DIR) {
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    crate::atomic_io::atomic_write_text(path, content, Some(mode))
        .map_err(|error| error.to_string())
}

pub fn add_share(request: &ShareRequest) -> Result<String, String> {
    let server = request
        .server
        .as_deref()
        .ok_or_else(|| "server is required".to_string())?;
    let share_path = request
        .share_path
        .as_deref()
        .ok_or_else(|| "share_path is required".to_string())?;
    let username = request
        .username
        .as_deref()
        .ok_or_else(|| "username is required".to_string())?;
    let password = request.password.as_deref().unwrap_or("");
    let domain = request.domain.as_deref().unwrap_or("");
    let unit = mount_unit(&request.mount_point)?;
    let credential_path = safe_artifact_path(Path::new(CREDENTIALS_DIR), &request.name);
    let unit_path = safe_artifact_path(Path::new(UNIT_DIR), &unit);
    ensure_no_symlink_in_path(Path::new(&request.mount_point))?;
    fs::create_dir_all(&request.mount_point).map_err(|error| error.to_string())?;

    let mut credentials = format!("username={username}\npassword={password}\n");
    if !domain.is_empty() {
        credentials.push_str(&format!("domain={domain}\n"));
    }
    atomic_write(&credential_path, &credentials, 0o600)?;
    let unc = format!("//{server}/{share_path}");
    let options = format!(
        "credentials={},uid={},gid={},iocharset=utf8,vers=3.0,nofail,_netdev",
        credential_path.display(),
        request.uid,
        request.gid
    );
    let unit_text = format!(
        "[Unit]\nDescription=KythOS SMB Share {}\nAfter=network-online.target\nWants=network-online.target\n\n[Mount]\nWhat={unc}\nWhere={}\nType=cifs\nOptions={options}\nTimeoutSec=30\n\n[Install]\nWantedBy=multi-user.target\n\n",
        request.name, request.mount_point
    );
    atomic_write(&unit_path, &unit_text, 0o644)?;
    run_systemctl(&["daemon-reload"], 30)?;
    if request.auto_mount {
        run_systemctl(&["enable", &unit], 30)?;
    } else {
        let _ = run_systemctl(&["disable", &unit], 30);
    }
    if request.mount_now {
        run_systemctl(&["start", &unit], 45)?;
    }
    Ok(format!("Configured SMB share {}.", request.name))
}

pub fn remove_share(request: &ShareRequest) -> Result<String, String> {
    let unit = mount_unit(&request.mount_point)?;
    let _ = run_systemctl(&["stop", &unit], 30);
    let _ = run_systemctl(&["disable", &unit], 30);
    let unit_path = safe_artifact_path(Path::new(UNIT_DIR), &unit);
    let credential_path = safe_artifact_path(Path::new(CREDENTIALS_DIR), &request.name);
    if unit_path.is_symlink() || credential_path.is_symlink() {
        return Err("refusing to remove a symlinked share artifact".into());
    }
    let _ = fs::remove_file(unit_path);
    if credential_path.is_file() {
        let argv = vec![
            "shred".into(),
            "-u".into(),
            "-n".into(),
            "3".into(),
            credential_path.display().to_string(),
        ];
        let _ = crate::system::process::run_bounded(&argv, Duration::from_secs(5));
    }
    let _ = fs::remove_file(credential_path);
    run_systemctl(&["daemon-reload"], 30)?;
    Ok(format!("Removed SMB share {}.", request.name))
}

fn run_systemctl(args: &[&str], timeout: u64) -> Result<(), String> {
    let mut argv = vec!["systemctl".to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    let output = crate::system::process::run_bounded(&argv, Duration::from_secs(timeout))
        .map_err(|error| format!("systemctl failed to run: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr)
            .trim()
            .chars()
            .take(400)
            .collect::<String>();
        Err(if detail.is_empty() {
            "systemctl operation failed".into()
        } else {
            detail
        })
    }
}

pub fn run_payload(action: &str, raw: &[u8]) -> Result<String, String> {
    if raw.len() > MAX_PAYLOAD {
        return Err("share payload is too large".into());
    }
    let payload: Value =
        serde_json::from_slice(raw).map_err(|error| format!("invalid share payload: {error}"))?;
    let request = validate_request(&payload, action == "add")?;
    match action {
        "add" => add_share(&request),
        "remove" => remove_share(&request),
        _ => Err("usage: kyth-network-share {add|remove}".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload() -> Value {
        json!({"name":"media", "server":"nas.local", "share_path":"media", "mount_point":"/mnt/media", "username":"pat", "password":"secret", "domain":"", "auto_mount":true, "mount_now":false, "uid":1000, "gid":1000})
    }

    #[test]
    fn validates_and_normalizes_share_payload() {
        let request = validate_request(&payload(), true).unwrap();
        assert_eq!(request.mount_point, "/mnt/media");
        assert_eq!(request.server.as_deref(), Some("nas.local"));
        assert_eq!(request.uid, 1000);
    }

    #[test]
    fn rejects_unsafe_paths_and_names() {
        let mut value = payload();
        value["name"] = json!("bad/name");
        assert!(validate_request(&value, true).is_err());
        value["name"] = json!("media");
        value["mount_point"] = json!("/etc/kyth");
        assert!(validate_request(&value, true).is_err());
        value["mount_point"] = json!("/mnt/../home/user/share");
        assert_eq!(
            validate_request(&value, true).unwrap().mount_point,
            "/home/user/share"
        );
    }

    #[test]
    fn remove_payload_does_not_require_secrets() {
        let value = json!({"name":"media", "mount_point":"/mnt/media", "uid":1000, "gid":1000});
        let request = validate_request(&value, false).unwrap();
        assert!(request.password.is_none());
        assert!(request.server.is_none());
    }
}
