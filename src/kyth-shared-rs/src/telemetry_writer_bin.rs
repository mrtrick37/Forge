#![cfg(feature = "telemetry-writer")]

use std::path::PathBuf;
use std::time::Duration;

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"))
}

fn settings() -> (u64, u64) {
    let path = home().join(".config/kyth/telem.toml");
    let Ok(raw) = std::fs::read_to_string(path) else {
        return (10, 15);
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return (10, 15);
    };
    let section = value.get("telemetry").unwrap_or(&value);
    let interval = section
        .get("scan_interval")
        .and_then(toml::Value::as_integer)
        .unwrap_or(10)
        .max(1) as u64;
    let min_age = section
        .get("min_file_age")
        .and_then(toml::Value::as_integer)
        .unwrap_or(15)
        .max(0) as u64;
    (interval, min_age)
}

fn main() -> std::process::ExitCode {
    let once = std::env::args().any(|arg| arg == "--once");
    let home = home();
    let sessions = kyth_shared::system::telemetry_writer::sessions_path();
    if let Err(error) =
        kyth_shared::system::telemetry_writer::ensure_mangohud_output(&home, &sessions)
    {
        eprintln!("kyth-telem: {error}");
        return std::process::ExitCode::from(1);
    }
    let db_path = kyth_shared::system::telemetry_writer::database_path();
    let mut conn = match kyth_shared::system::telemetry_writer::open_database(&db_path) {
        Ok(conn) => conn,
        Err(error) => {
            eprintln!("kyth-telem: {error}");
            return std::process::ExitCode::from(1);
        }
    };
    let (interval, min_age) = settings();
    loop {
        match kyth_shared::system::telemetry_writer::scan_directory(
            &mut conn,
            &sessions,
            min_age,
            std::time::SystemTime::now(),
            kyth_shared::system::telemetry_writer::current_unix_time(),
        ) {
            Ok(count) if count > 0 => eprintln!("kyth-telem: ingested {count} session(s)"),
            Ok(_) => {}
            Err(error) => eprintln!("kyth-telem: scan failed: {error}"),
        }
        if once {
            break;
        }
        std::thread::sleep(Duration::from_secs(interval));
    }
    std::process::ExitCode::SUCCESS
}
