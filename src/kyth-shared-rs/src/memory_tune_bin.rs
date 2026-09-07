//! Native RAM-aware memory tuning writer.

use std::env;
use std::path::Path;
use std::process::ExitCode;

use kyth_shared::atomic_io::atomic_write_text;
use kyth_shared::system::memory_tune::{config_path, load, MemoryTuning};
use kyth_shared::system::process::run_bounded;

const SYSCTL_PATH: &str = "/etc/sysctl.d/99-kyth-memory.conf";
const ZRAM_PATH: &str = "/etc/systemd/zram-generator.conf";
const RUNTIME_ENV_PATH: &str = "/etc/kyth/zram-runtime.env";

fn usage() {
    eprintln!("usage: kyth-memory-tune [status|apply] [--config PATH] [--mem-kb N]");
}

fn write(config: &MemoryTuning) -> Result<(), String> {
    atomic_write_text(
        SYSCTL_PATH,
        &kyth_shared::system::memory_tune::sysctl_content(config),
        Some(0o644),
    )
    .map_err(|error| error.to_string())?;
    atomic_write_text(
        ZRAM_PATH,
        &kyth_shared::system::memory_tune::zram_content(config),
        Some(0o644),
    )
    .map_err(|error| error.to_string())?;
    atomic_write_text(
        RUNTIME_ENV_PATH,
        &kyth_shared::system::memory_tune::runtime_env_content(config),
        Some(0o644),
    )
    .map_err(|error| error.to_string())?;
    let result = run_bounded(
        &[
            "sysctl".into(),
            "--load=/etc/sysctl.d/99-kyth-memory.conf".into(),
        ],
        std::time::Duration::from_secs(30),
    )
    .map_err(|error| error.to_string())?;
    if !result.status.success() {
        return Err("sysctl rejected the generated memory policy".into());
    }
    Ok(())
}

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let command = args.first().cloned().unwrap_or_else(|| "status".into());
    let config = args
        .iter()
        .position(|value| value == "--config")
        .and_then(|index| args.get(index + 1))
        .cloned();
    let mem_kb = args
        .iter()
        .position(|value| value == "--mem-kb")
        .and_then(|index| args.get(index + 1))
        .and_then(|value| value.parse::<i64>().ok());
    if args.iter().any(|value| value == "--help" || value == "-h") {
        usage();
        return ExitCode::SUCCESS;
    }
    let policy = load(config_path(config.as_deref()), mem_kb);
    match command.as_str() {
        "status" => {
            println!(
                "tier={} swappiness={} dirty={} active={}",
                policy.tier.as_str(),
                policy.swappiness,
                policy.dirty_bytes,
                Path::new(SYSCTL_PATH).is_file()
            );
            ExitCode::SUCCESS
        }
        "apply" => {
            if unsafe { libc::geteuid() } != 0 {
                eprintln!("kyth-memory-tune: apply must run as root");
                return ExitCode::from(1);
            }
            match write(&policy) {
                Ok(()) => {
                    println!("memory tune applied: tier={}", policy.tier.as_str());
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("kyth-memory-tune: {error}");
                    ExitCode::from(1)
                }
            }
        }
        _ => {
            usage();
            ExitCode::from(2)
        }
    }
}
