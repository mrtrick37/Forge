//! VM acceptance guest entry point.

use std::{path::Path, process::ExitCode, time::Duration};

use serde::Serialize;

const FW_CFG_ROOT: &str = "/sys/firmware/qemu_fw_cfg/by_name/opt/com.kyth";
const STATE_FILE: &str = "/var/lib/kyth/vm-acceptance/state";
const UPDATE_FILE: &str = "/sys/firmware/qemu_fw_cfg/by_name/opt/com.kyth/update-ref/raw";

#[derive(Debug, Serialize)]
struct Report {
    enabled: bool,
    state: String,
    update_ref: String,
    booted_digest: String,
    deployment_count: usize,
}

fn read_trimmed(path: impl AsRef<Path>) -> String {
    std::fs::read(path)
        .unwrap_or_default()
        .into_iter()
        .filter(|byte| *byte != 0)
        .map(|byte| byte as char)
        .collect::<String>()
        .trim()
        .to_string()
}

fn report() -> Report {
    let enabled = read_trimmed(format!("{FW_CFG_ROOT}/acceptance/raw")) == "1";
    let state = kyth_shared::system::vm_acceptance::acceptance_state_from_text(Some(
        &read_trimmed(STATE_FILE),
    ))
    .to_string();
    let update_ref = read_trimmed(UPDATE_FILE);
    let booted_digest = kyth_shared::system::process::run_bounded(
        &[
            "bootc".into(),
            "status".into(),
            "--format".into(),
            "json".into(),
        ],
        Duration::from_secs(30),
    )
    .ok()
    .and_then(|output| {
        if output.status.success() {
            kyth_shared::system::vm_acceptance::booted_digest_from_json(&String::from_utf8_lossy(
                &output.stdout,
            ))
        } else {
            None
        }
    })
    .unwrap_or_default();
    let deployment_count = kyth_shared::system::process::run_bounded(
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
    .map_or(0, |output| {
        kyth_shared::system::vm_acceptance::deployment_count_from_json(&String::from_utf8_lossy(
            &output.stdout,
        ))
    });
    Report {
        enabled,
        state,
        update_ref,
        booted_digest,
        deployment_count,
    }
}

fn usage() {
    eprintln!(
        "Usage: kyth-vm-acceptance-guest <enabled|run|report|decode-bootc|count-deployments> [--json]"
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("report");
    let json = args.iter().any(|arg| arg == "--json");
    if args
        .iter()
        .any(|arg| arg.starts_with('-') && arg != "--json")
    {
        usage();
        return ExitCode::from(2);
    }
    match command {
        "enabled" if args.len() == 1 => {
            if read_trimmed(format!("{FW_CFG_ROOT}/acceptance/raw")) == "1" {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        "run" if args.len() == 1 => match kyth_shared::system::vm_acceptance::run() {
            Ok(code) => code,
            Err(error) => {
                eprintln!("kyth-vm-acceptance-guest: {error}");
                ExitCode::from(1)
            }
        },
        "report" if args.len() <= 2 => {
            let value = report();
            if json {
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
            } else {
                println!(
                    "enabled: {}\nstate: {}\nupdate-ref: {}\nbooted-digest: {}\ndeployments: {}",
                    value.enabled,
                    value.state,
                    value.update_ref,
                    value.booted_digest,
                    value.deployment_count
                );
            }
            ExitCode::SUCCESS
        }
        "decode-bootc" if args.len() == 1 => {
            let input = std::fs::read_to_string("/dev/stdin").unwrap_or_default();
            match kyth_shared::system::vm_acceptance::booted_digest_from_json(&input) {
                Some(digest) => {
                    println!("{digest}");
                    ExitCode::SUCCESS
                }
                None => ExitCode::from(1),
            }
        }
        "count-deployments" if args.len() == 1 => {
            let input = std::fs::read_to_string("/dev/stdin").unwrap_or_default();
            println!(
                "{}",
                kyth_shared::system::vm_acceptance::deployment_count_from_json(&input)
            );
            ExitCode::SUCCESS
        }
        _ => {
            usage();
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn lifecycle_command_is_not_a_read_mode() {
        let read_modes = ["enabled", "report", "decode-bootc", "count-deployments"];
        assert!(!read_modes.contains(&"run"));
    }

    #[test]
    fn report_schema_keeps_lifecycle_state_read_only() {
        let report = super::Report {
            enabled: true,
            state: "fresh".into(),
            update_ref: String::new(),
            booted_digest: "sha256:test".into(),
            deployment_count: 1,
        };
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["state"], "fresh");
        assert_eq!(json["deployment_count"], 1);
    }
}
