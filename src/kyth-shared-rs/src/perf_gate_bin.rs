//! Native CLI for the runtime perf-gate recipe.

use std::env;
use std::fs;
use std::path::Path;
use std::time::Instant;

use kyth_shared::system::perf_gate::{check, config_path, load, PerfGateResult};

const LEDGER: &str = "/var/cache/kyth/perf-ledger.jsonl";

fn print_result(result: &PerfGateResult) {
    println!(
        "{}",
        serde_json::to_string_pretty(result).unwrap_or_else(|_| "{}".into())
    );
}

fn usage() {
    eprintln!("usage: kyth-perf-gate [status|measure] [--current-ms <milliseconds>] [--ledger PATH] [--record]");
}

fn measure_current_ms() -> f64 {
    let mut samples = (0..7)
        .map(|_| {
            let started = Instant::now();
            let _ = kyth_shared::system::probe::collect_snapshot();
            started.elapsed().as_secs_f64() * 1000.0
        })
        .collect::<Vec<_>>();
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn record(ledger: &Path, current: f64) -> Result<(), String> {
    let mut lines = fs::read_to_string(ledger)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    lines.push(serde_json::json!({
        "p95": (current * 100.0).round() / 100.0,
        "commit": env::var("GITHUB_SHA").unwrap_or_else(|_| "local".into()),
        "recorded_at": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
    }).to_string());
    let start = lines.len().saturating_sub(50);
    let text = format!("{}\n", lines[start..].join("\n"));
    kyth_shared::atomic_io::atomic_write_text(ledger, &text, Some(0o644))
        .map_err(|error| error.to_string())
}

fn main() -> std::process::ExitCode {
    let mut current_ms = None;
    let mut measure = false;
    let mut record_baseline = false;
    let mut ledger = Path::new(LEDGER).to_path_buf();
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "status" => {}
            "measure" => measure = true,
            "--record" => record_baseline = true,
            "--ledger" => {
                let Some(value) = args.next() else {
                    usage();
                    return std::process::ExitCode::from(2);
                };
                ledger = value.into();
            }
            "--current-ms" => {
                let Some(value) = args.next() else {
                    usage();
                    return std::process::ExitCode::from(2);
                };
                current_ms = match value.parse::<f64>() {
                    Ok(value) => Some(value),
                    Err(_) => {
                        usage();
                        return std::process::ExitCode::from(2);
                    }
                };
            }
            "-h" | "--help" => {
                usage();
                return std::process::ExitCode::SUCCESS;
            }
            _ => {
                usage();
                return std::process::ExitCode::from(2);
            }
        }
    }
    if measure {
        current_ms = Some(measure_current_ms());
    }
    if record_baseline {
        let current = current_ms.unwrap_or_else(measure_current_ms);
        if let Err(error) = record(&ledger, current) {
            eprintln!("kyth-perf-gate: {error}");
            return std::process::ExitCode::from(1);
        }
        println!(
            "perf gate: recorded new baseline {current:.1}ms -> {}",
            ledger.display()
        );
        return std::process::ExitCode::SUCCESS;
    }
    let result = check(load(config_path(None::<&Path>)), current_ms, &ledger);
    print_result(&result);
    if result.enabled && !result.pass {
        std::process::ExitCode::from(1)
    } else {
        std::process::ExitCode::SUCCESS
    }
}
