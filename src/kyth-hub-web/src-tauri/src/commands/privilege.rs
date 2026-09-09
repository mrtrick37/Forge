use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

static JOBS: OnceLock<Mutex<HashMap<String, (String, String)>>> = OnceLock::new();

fn jobs() -> &'static Mutex<HashMap<String, (String, String)>> {
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Typed frontend payload for the allowlisted privileged operations. Fields
/// stay optional because the operation selects the required subset; unknown
/// JSON fields are rejected by the serde boundary rather than reaching the
/// root-owned service.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct PrivilegedPayload {
    pub(crate) app_id: Option<String>,
    pub(crate) flavor: Option<String>,
    pub(crate) device: Option<String>,
    pub(crate) key: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) server: Option<String>,
    pub(crate) share_path: Option<String>,
    pub(crate) mount_point: Option<String>,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<String>,
    pub(crate) domain: Option<String>,
    pub(crate) auto_mount: Option<bool>,
    pub(crate) mount_now: Option<bool>,
}

/// Validate and construct the only requests the root-owned service accepts.
/// The BitLocker key is copied into the request only after validation and is
/// never included in an error or job status message.
fn validated_request(operation: &str, payload: &PrivilegedPayload) -> Result<Value, String> {
    match operation {
        "flatpak_uninstall" => {
            let app_id = payload
                .app_id
                .as_deref()
                .ok_or_else(|| "Flatpak application id is required".to_string())?;
            validate_flatpak_id(app_id)?;
            Ok(json!({ "operation": "flatpak_uninstall", "app_id": app_id }))
        }
        "firmware_update" | "nvidia_install" | "secureboot_enroll" => {
            Ok(json!({ "operation": operation }))
        }
        "kernel_switch" => {
            let flavor = payload
                .flavor
                .as_deref()
                .ok_or_else(|| "kernel flavor is required".to_string())?;
            if !matches!(flavor, "fedora" | "cachy") {
                return Err("kernel flavor must be fedora or cachy".to_string());
            }
            Ok(json!({ "operation": "kernel_switch", "flavor": flavor }))
        }
        "bitlocker_unlock" => {
            let device = payload
                .device
                .as_deref()
                .ok_or_else(|| "block device is required".to_string())?;
            let key = payload
                .key
                .as_deref()
                .ok_or_else(|| "BitLocker key is required".to_string())?;
            if !valid_block_device(device) {
                return Err("invalid block device".to_string());
            }
            if !(8..=128).contains(&key.len()) || key.contains(['\n', '\r']) {
                return Err("invalid BitLocker key".to_string());
            }
            Ok(json!({ "operation": "bitlocker_unlock", "device": device, "key": key }))
        }
        "network_share_add" => {
            validate_network_share(payload, true)?;
            Ok(json!({ "operation": "network_share_add", "payload": payload }))
        }
        "network_share_remove" => {
            validate_network_share(payload, false)?;
            Ok(json!({ "operation": "network_share_remove", "payload": payload }))
        }
        _ => Err("privileged operation is not allowlisted".to_string()),
    }
}

fn share_text<'a>(
    payload: &'a PrivilegedPayload,
    field: &str,
    allow_empty: bool,
    maximum: usize,
) -> Result<&'a str, String> {
    let value = match field {
        "name" => payload.name.as_deref(),
        "server" => payload.server.as_deref(),
        "share_path" => payload.share_path.as_deref(),
        "mount_point" => payload.mount_point.as_deref(),
        "username" => payload.username.as_deref(),
        "password" => payload.password.as_deref(),
        "domain" => payload.domain.as_deref(),
        _ => None,
    }
    .ok_or_else(|| format!("{field} is required"))?;
    if (!allow_empty && value.is_empty())
        || value.len() > maximum
        || value.chars().any(|character| character.is_control())
    {
        return Err(format!("invalid {field}"));
    }
    Ok(value)
}

fn validate_network_share(payload: &PrivilegedPayload, adding: bool) -> Result<(), String> {
    let name = share_text(payload, "name", false, 64)?;
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err("invalid share name".to_string());
    }
    let mount_point = share_text(payload, "mount_point", false, 4096)?;
    let approved_mount = ["/mnt/", "/media/", "/run/media/", "/home/"];
    if !approved_mount
        .iter()
        .any(|prefix| mount_point.starts_with(prefix))
        || mount_point.contains("//")
        || mount_point.split('/').any(|part| part == "..")
        || !mount_point.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b' ' | b'-')
        })
    {
        return Err("invalid mount_point".to_string());
    }
    if !adding {
        return Ok(());
    }
    let server = share_text(payload, "server", false, 253)?;
    if !server
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err("invalid share server".to_string());
    }
    let share_path = share_text(payload, "share_path", false, 4096)?;
    if share_path.starts_with('.')
        || share_path.contains("//")
        || share_path.contains('%')
        || share_path.split('/').any(|part| part == "..")
    {
        return Err("invalid share_path".to_string());
    }
    share_text(payload, "username", false, 256)?;
    share_text(payload, "password", true, 4096)?;
    share_text(payload, "domain", true, 256)?;
    if payload.auto_mount.is_none() || payload.mount_now.is_none() {
        return Err("share mount options are required".to_string());
    }
    Ok(())
}

pub(crate) fn validate_flatpak_id(value: &str) -> Result<(), String> {
    if value.len() > 200 {
        return Err("invalid Flatpak application id".to_string());
    }
    let parts: Vec<&str> = value.split(['.', '-']).collect();
    if parts.len() < 2
        || !parts[0].bytes().all(|byte| byte.is_ascii_alphanumeric())
        || parts[1..].iter().any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
    {
        return Err("invalid Flatpak application id".to_string());
    }
    Ok(())
}

fn valid_block_device(value: &str) -> bool {
    let Some(name) = value.strip_prefix("/dev/") else {
        return false;
    };
    if name.is_empty() || name.len() > 64 || name.contains('/') || !name.is_ascii() {
        return false;
    }
    if let Some(rest) = name.strip_prefix("sd").or_else(|| name.strip_prefix("vd")) {
        return rest.len() >= 1
            && rest.as_bytes()[0].is_ascii_lowercase()
            && rest[1..].bytes().all(|byte| byte.is_ascii_digit());
    }
    if let Some(rest) = name.strip_prefix("nvme") {
        let Some((controller, namespace)) = rest.split_once('n') else {
            return false;
        };
        let (namespace, partition) = namespace
            .split_once('p')
            .map_or((namespace, ""), |(n, p)| (n, p));
        return !controller.is_empty()
            && controller.bytes().all(|byte| byte.is_ascii_digit())
            && !namespace.is_empty()
            && namespace.bytes().all(|byte| byte.is_ascii_digit())
            && partition.bytes().all(|byte| byte.is_ascii_digit());
    }
    if let Some(rest) = name.strip_prefix("mmcblk") {
        let Some((device, partition)) = rest.split_once('p') else {
            return false;
        };
        return !device.is_empty()
            && device.bytes().all(|byte| byte.is_ascii_digit())
            && !partition.is_empty()
            && partition.bytes().all(|byte| byte.is_ascii_digit());
    }
    false
}

#[tauri::command]
pub(crate) fn privileged_action(
    operation: String,
    payload: PrivilegedPayload,
) -> Result<PrivilegedActionLaunch, String> {
    let request = validated_request(&operation, &payload)?;
    let job = format!(
        "privileged-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    jobs()
        .lock()
        .map_err(|_| "privileged job store is unavailable".to_string())?
        .insert(
            job.clone(),
            ("running".into(), format!("Running {operation}…")),
        );
    let job_for_thread = job.clone();
    std::thread::spawn(move || {
        let result = send_request(request);
        let (state, detail) = match result {
            Ok(detail) => ("complete", detail),
            Err(detail) => ("failed", detail),
        };
        if let Ok(mut store) = jobs().lock() {
            store.insert(job_for_thread, (state.into(), detail));
        }
    });
    Ok(PrivilegedActionLaunch {
        job,
        state: "running".into(),
        detail: format!("Running {operation}…"),
    })
}

#[derive(Serialize)]
pub(crate) struct PrivilegedActionLaunch {
    pub(crate) job: String,
    pub(crate) state: String,
    pub(crate) detail: String,
}

pub(crate) fn send_request(request: Value) -> Result<String, String> {
    let mut stream = UnixStream::connect("/run/kyth/privileged.sock")
        .map_err(|_| "privileged service is unavailable".to_string())?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(910)))
        .map_err(|error| format!("could not configure privileged service timeout: {error}"))?;
    stream
        .write_all(format!("{request}\n").as_bytes())
        .map_err(|error| format!("could not contact privileged service: {error}"))?;
    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .map_err(|error| format!("could not read privileged service: {error}"))?;
    let value: Value = serde_json::from_str(&response)
        .map_err(|error| format!("invalid privileged service response: {error}"))?;
    if value.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        let detail = value
            .get("detail")
            .and_then(Value::as_str)
            .unwrap_or("Operation complete.");
        Ok(kyth_shared::privileged::redact_request_detail(
            &request, detail,
        ))
    } else {
        let detail = value
            .get("detail")
            .and_then(Value::as_str)
            .unwrap_or("privileged operation failed");
        Err(kyth_shared::privileged::redact_request_detail(
            &request, detail,
        ))
    }
}

#[allow(dead_code)]
pub(crate) fn bitlocker_request(device: &str, key: &str) -> Result<Value, String> {
    validated_request(
        "bitlocker_unlock",
        &PrivilegedPayload {
            device: Some(device.into()),
            key: Some(key.into()),
            ..Default::default()
        },
    )
}

pub(crate) fn flatpak_uninstall(app_id: &str) -> Result<String, String> {
    let request = validated_request(
        "flatpak_uninstall",
        &PrivilegedPayload {
            app_id: Some(app_id.into()),
            ..Default::default()
        },
    )?;
    send_request(request)
}

#[tauri::command]
pub(crate) fn privileged_action_status(job: String) -> crate::InstallStatus {
    let (state, detail) = jobs()
        .lock()
        .ok()
        .and_then(|store| store.get(&job).cloned())
        .unwrap_or(("unknown".into(), "Privileged job not found.".into()));
    crate::InstallStatus {
        id: job,
        state,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{validated_request, PrivilegedPayload};

    fn payload(value: serde_json::Value) -> PrivilegedPayload {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn only_allowlisted_operations_are_constructed() {
        assert!(validated_request("not-allowed", &payload(json!({}))).is_err());
        assert_eq!(
            validated_request("kernel_switch", &payload(json!({ "flavor": "cachy" }))).unwrap()
                ["flavor"],
            "cachy"
        );
        assert!(validated_request(
            "flatpak_uninstall",
            &payload(json!({ "app_id": "org.example.App" }))
        )
        .is_ok());
        assert!(validated_request(
            "flatpak_uninstall",
            &payload(json!({ "app_id": "org.example-App" }))
        )
        .is_ok());
        assert!(validated_request(
            "flatpak_uninstall",
            &payload(json!({ "app_id": "_org.example" }))
        )
        .is_err());
    }

    #[test]
    fn bitlocker_validation_rejects_bad_devices_and_keys_without_echoing_secret() {
        let error = validated_request(
            "bitlocker_unlock",
            &payload(json!({ "device": "/tmp/disk", "key": "secret-key" })),
        )
        .unwrap_err();
        assert_eq!(error, "invalid block device");
        let error = validated_request(
            "bitlocker_unlock",
            &payload(json!({ "device": "/dev/sda1", "key": "short" })),
        )
        .unwrap_err();
        assert_eq!(error, "invalid BitLocker key");
        assert!(!error.contains("short"));
    }

    #[test]
    fn network_share_request_is_nested_and_validated() {
        let value = json!({"name":"media", "server":"nas.local", "share_path":"media", "mount_point":"/mnt/media", "username":"pat", "password":"secret", "domain":"", "auto_mount":true, "mount_now":false});
        let request = validated_request("network_share_add", &payload(value)).unwrap();
        assert_eq!(request["operation"], "network_share_add");
        assert_eq!(request["payload"]["name"], "media");
        assert!(
            validated_request("network_share_add", &payload(json!({"name":"bad/name"}))).is_err()
        );
    }

    #[test]
    fn typed_payload_rejects_unknown_fields_before_validation() {
        assert!(serde_json::from_value::<PrivilegedPayload>(
            json!({"operation":"bitlocker_unlock"})
        )
        .is_err());
    }
}
