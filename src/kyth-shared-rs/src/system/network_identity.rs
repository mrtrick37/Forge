//! Port of `kyth_shared.system.network_identity` — VPN/SMB/cloud single view.

use std::path::Path;
use std::time::Duration;

fn run_with_timeout(cmd: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut argv = vec![cmd.to_string()];
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    let output = super::process::run_bounded(&argv, timeout).ok()?;
    Some(if output.status.success() {
        String::from_utf8_lossy(&output.stdout).to_string()
    } else {
        String::new()
    })
}

fn vpn_status() -> (bool, String) {
    if let Some(stdout) = run_with_timeout(
        "nmcli",
        &["connection", "show", "--active"],
        Duration::from_secs(5),
    ) {
        for line in stdout.lines() {
            let low = line.to_lowercase();
            if low.contains("vpn") || low.contains("wireguard") || low.contains("globalprotect") {
                let name = line.split_whitespace().next().unwrap_or("VPN").to_string();
                return (true, name);
            }
        }
    }
    (false, String::new())
}

fn smb_mounts() -> i32 {
    if let Ok(text) = std::fs::read_to_string("/proc/mounts") {
        return text.lines().filter(|l| l.contains(" cifs ")).count() as i32;
    }
    0
}

fn cloud_providers() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let cfg = Path::new(&home).join(".config/kyth-cloud-sync.json");
    if let Ok(text) = std::fs::read_to_string(&cfg) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            let mut out = Vec::new();
            if v.get("onedrive").and_then(|x| x.as_object()).is_some() {
                out.push("onedrive".to_string());
            }
            if v.get("gdrive").and_then(|x| x.as_object()).is_some() {
                out.push("gdrive".to_string());
            }
            if v.get("dropbox").and_then(|x| x.as_object()).is_some() {
                out.push("dropbox".to_string());
            }
            return out;
        }
    }
    Vec::new()
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NetworkIdentity {
    pub vpn_connected: bool,
    pub vpn_name: String,
    pub smb_mounts: i32,
    pub cloud_providers: Vec<String>,
    pub detail: String,
}

pub fn get_network_identity() -> NetworkIdentity {
    let (vpn_connected, vpn_name) = vpn_status();
    let smb = smb_mounts();
    let providers = cloud_providers();
    let mut parts = Vec::new();
    if vpn_connected {
        parts.push(format!("VPN {} connected", vpn_name));
    }
    if smb > 0 {
        parts.push(format!("{} SMB mount(s)", smb));
    }
    if !providers.is_empty() {
        parts.push(format!("cloud: {}", providers.join(", ")));
    }
    let detail = if parts.is_empty() {
        "No active work network".to_string()
    } else {
        parts.join("; ")
    };
    NetworkIdentity {
        vpn_connected,
        vpn_name,
        smb_mounts: smb,
        cloud_providers: providers,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn returns_identity() {
        let n = get_network_identity();
        // detail always non-empty
        assert!(!n.detail.is_empty());
    }
}
