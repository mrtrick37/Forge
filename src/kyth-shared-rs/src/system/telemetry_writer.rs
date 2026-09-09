//! Opt-in MangoHud telemetry filesystem/SQLite writer.
//!
//! Enable with the `telemetry-writer` feature. The production `kyth-telem`
//! daemon remains Python-owned until this writer has been exercised against
//! real MangoHud output and its database is proven byte-for-byte compatible
//! where that matters. No daemon, config mutation, or automatic activation is
//! provided by this module.

#![cfg(feature = "telemetry-writer")]

use crate::system::telemetry_ingest::{
    derive_game_name, detect_launcher, parse_mangohud_csv, safe_float,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_FRAMES_PER_SESSION: usize = 36_000;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    game_name TEXT NOT NULL,
    executable TEXT,
    launcher TEXT,
    source_file TEXT UNIQUE,
    started_at INTEGER,
    ended_at INTEGER,
    duration_s REAL,
    avg_fps REAL,
    p1_low_fps REAL,
    p01_low_fps REAL,
    stutter_count INTEGER,
    peak_gpu_temp REAL,
    peak_vram_mb REAL,
    avg_cpu_load REAL,
    avg_gpu_load REAL,
    gpu_name TEXT,
    cpu_name TEXT,
    kernel TEXT,
    driver TEXT,
    scheduler TEXT,
    ingested_at INTEGER
);
CREATE TABLE IF NOT EXISTS frames (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    ts REAL,
    fps REAL,
    frametime REAL,
    cpu_load REAL,
    gpu_load REAL,
    cpu_temp REAL,
    gpu_temp REAL,
    vram_mb REAL
);
CREATE INDEX IF NOT EXISTS idx_frames_session ON frames(session_id);
CREATE INDEX IF NOT EXISTS idx_sessions_started ON sessions(started_at DESC);
"#;

#[derive(Debug, Clone, Copy)]
struct Frame {
    ts: f64,
    fps: f64,
    frametime: f64,
    cpu_load: f64,
    gpu_load: f64,
    cpu_temp: f64,
    gpu_temp: f64,
    vram_mb: f64,
}

pub fn initialize_database(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(SCHEMA)
        .map_err(|error| format!("could not initialize telemetry database: {error}"))
}

pub fn database_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"));
    home.join(".local/share/kyth/telemetry.db")
}

pub fn sessions_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"));
    home.join(".local/share/kyth/gaming-sessions")
}

pub fn open_database(path: impl AsRef<Path>) -> Result<Connection, String> {
    if let Some(parent) = path.as_ref().parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create telemetry data directory: {error}"))?;
    }
    let conn = Connection::open(path)
        .map_err(|error| format!("could not open telemetry database: {error}"))?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
        .map_err(|error| format!("could not configure telemetry database: {error}"))?;
    initialize_database(&conn)?;
    Ok(conn)
}

/// Keep MangoHud output pointed at the writer's stable landing zone. This is
/// deliberately a small config edit: unrelated user settings are preserved.
pub fn ensure_mangohud_output(
    home: impl AsRef<Path>,
    sessions: impl AsRef<Path>,
) -> Result<(), String> {
    let home = home.as_ref();
    let sessions = sessions.as_ref();
    std::fs::create_dir_all(sessions)
        .map_err(|error| format!("could not create telemetry session directory: {error}"))?;
    let config = home.join(".config/MangoHud/MangoHud.conf");
    if let Some(parent) = config.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create MangoHud config directory: {error}"))?;
    }
    let output_line = format!("output_folder={}", sessions.display());
    let old = std::fs::read_to_string(&config).unwrap_or_default();
    let mut found = false;
    let mut lines = Vec::new();
    for line in old.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("output_folder")
            && trimmed["output_folder".len()..]
                .trim_start()
                .starts_with('=')
        {
            if !found {
                lines.push(output_line.clone());
                found = true;
            }
        } else {
            lines.push(line.to_string());
        }
    }
    if !found {
        if !old.is_empty() {
            lines.push(String::new());
        }
        lines.push("# Added by kyth-telem".to_string());
        lines.push(output_line);
    }
    let text = format!("{}\n", lines.join("\n"));
    if text != old {
        crate::atomic_io::atomic_write_text(&config, &text, Some(0o600))
            .map_err(|error| format!("could not update MangoHud config: {error}"))?;
    }
    Ok(())
}

fn unix_mtime(path: &Path) -> Result<i64, String> {
    let modified = std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|error| format!("could not read telemetry file metadata: {error}"))?;
    Ok(modified
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64)
}

fn average(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn stats(
    frames: &[Frame],
) -> Option<(
    f64,
    f64,
    f64,
    i64,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
)> {
    let fps: Vec<f64> = frames
        .iter()
        .map(|frame| frame.fps)
        .filter(|value| *value > 0.0)
        .collect();
    if fps.is_empty() {
        return None;
    }
    let frametimes: Vec<f64> = frames
        .iter()
        .map(|frame| frame.frametime)
        .filter(|value| *value > 0.0)
        .collect();
    let gpu_temps: Vec<f64> = frames
        .iter()
        .map(|frame| frame.gpu_temp)
        .filter(|value| *value > 0.0)
        .collect();
    let vram: Vec<f64> = frames
        .iter()
        .map(|frame| frame.vram_mb)
        .filter(|value| *value > 0.0)
        .collect();
    let cpu_load: Vec<f64> = frames
        .iter()
        .map(|frame| frame.cpu_load)
        .filter(|value| *value > 0.0)
        .collect();
    let gpu_load: Vec<f64> = frames
        .iter()
        .map(|frame| frame.gpu_load)
        .filter(|value| *value > 0.0)
        .collect();

    let mut sorted_fps = fps.clone();
    sorted_fps.sort_by(f64::total_cmp);
    let p1_count = (sorted_fps.len() / 100).max(1);
    let p01_count = (sorted_fps.len() / 1000).max(1);
    let p1 = sorted_fps[..p1_count].iter().sum::<f64>() / p1_count as f64;
    let p01 = sorted_fps[..p01_count].iter().sum::<f64>() / p01_count as f64;

    let stutter_count = if frametimes.is_empty() {
        0
    } else {
        let mut sorted = frametimes.clone();
        sorted.sort_by(f64::total_cmp);
        let median = sorted[sorted.len() / 2];
        frametimes
            .iter()
            .filter(|value| **value > median * 2.0)
            .count() as i64
    };
    (
        average(&fps).unwrap_or(0.0),
        p1,
        p01,
        stutter_count,
        gpu_temps.iter().copied().max_by(f64::total_cmp),
        vram.iter().copied().max_by(f64::total_cmp),
        average(&cpu_load),
        average(&gpu_load),
    )
        .into()
}

/// Ingest one stable MangoHud CSV. Returns `true` only when a session row was
/// inserted; duplicate files, malformed data, and files without FPS samples
/// return `Ok(false)`.
pub fn ingest_csv(conn: &mut Connection, path: &Path, ingested_at: i64) -> Result<bool, String> {
    let source_file = path.to_string_lossy().into_owned();
    let existing: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM sessions WHERE source_file = ?1 LIMIT 1",
            [&source_file],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("could not check telemetry duplicate: {error}"))?;
    if existing.is_some() {
        return Ok(false);
    }

    let bytes =
        std::fs::read(path).map_err(|error| format!("could not read telemetry CSV: {error}"))?;
    let text = String::from_utf8_lossy(&bytes);
    let Some(parsed) = parse_mangohud_csv(&text) else {
        return Ok(false);
    };
    if parsed.rows.is_empty() {
        return Ok(false);
    }

    let mut frames = Vec::with_capacity(parsed.rows.len());
    for row in &parsed.rows {
        frames.push(Frame {
            ts: safe_float(row.get("time").map(String::as_str)),
            fps: safe_float(row.get("fps").map(String::as_str)),
            frametime: safe_float(row.get("frametime").map(String::as_str)),
            cpu_load: safe_float(row.get("cpu_load").map(String::as_str)),
            gpu_load: safe_float(row.get("gpu_load").map(String::as_str)),
            cpu_temp: safe_float(row.get("cpu_temp").map(String::as_str)),
            gpu_temp: safe_float(row.get("gpu_temp").map(String::as_str)),
            vram_mb: safe_float(row.get("vram_used").map(String::as_str)) / 1024.0,
        });
    }
    let Some((
        avg_fps,
        p1_low,
        p01_low,
        stutter_count,
        peak_gpu_temp,
        peak_vram_mb,
        avg_cpu_load,
        avg_gpu_load,
    )) = stats(&frames)
    else {
        return Ok(false);
    };

    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown");
    let (game_name, executable) = derive_game_name_with_proc(stem);
    let launcher = detect_launcher(
        &executable,
        parsed.metadata.get("driver").map(String::as_str),
    );
    let mtime = unix_mtime(path)?;
    let times: Vec<f64> = frames
        .iter()
        .map(|frame| frame.ts)
        .filter(|value| *value > 0.0)
        .collect();
    let (started_at, ended_at, duration_s) =
        if let (Some(first), Some(last)) = (times.first(), times.last()) {
            (mtime - (last - first) as i64, mtime, last - first)
        } else {
            let duration = frames.len() as f64 / avg_fps;
            (mtime - duration as i64, mtime, duration)
        };

    let transaction = conn
        .transaction()
        .map_err(|error| format!("could not start telemetry transaction: {error}"))?;
    transaction
        .execute(
            "INSERT OR IGNORE INTO sessions (game_name, executable, launcher, source_file, started_at, ended_at, duration_s, avg_fps, p1_low_fps, p01_low_fps, stutter_count, peak_gpu_temp, peak_vram_mb, avg_cpu_load, avg_gpu_load, gpu_name, cpu_name, kernel, driver, scheduler, ingested_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)",
            params![
                game_name,
                executable,
                launcher,
                source_file,
                started_at,
                ended_at,
                duration_s,
                avg_fps,
                p1_low,
                p01_low,
                stutter_count,
                peak_gpu_temp,
                peak_vram_mb,
                avg_cpu_load,
                avg_gpu_load,
                parsed.metadata.get("gpu"),
                parsed.metadata.get("cpu"),
                parsed.metadata.get("kernel"),
                parsed.metadata.get("driver"),
                parsed.metadata.get("cpu-scheduler"),
                ingested_at,
            ],
        )
        .map_err(|error| format!("could not insert telemetry session: {error}"))?;
    let session_id = transaction.last_insert_rowid();
    if session_id > 0 {
        let step = (frames.len() + MAX_FRAMES_PER_SESSION - 1) / MAX_FRAMES_PER_SESSION;
        for frame in frames.iter().step_by(step.max(1)) {
            transaction
                .execute(
                    "INSERT INTO frames (session_id, ts, fps, frametime, cpu_load, gpu_load, cpu_temp, gpu_temp, vram_mb) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    params![session_id, frame.ts, frame.fps, frame.frametime, frame.cpu_load, frame.gpu_load, frame.cpu_temp, frame.gpu_temp, frame.vram_mb],
                )
                .map_err(|error| format!("could not insert telemetry frame: {error}"))?;
        }
    }
    transaction
        .commit()
        .map_err(|error| format!("could not commit telemetry session: {error}"))?;
    Ok(session_id > 0)
}

fn proc_env(pid: &str, key: &str) -> Option<String> {
    let path = Path::new("/proc").join(pid).join("environ");
    let bytes = std::fs::read(path).ok()?;
    bytes
        .split(|byte| *byte == 0)
        .find_map(|item| {
            let mut parts = item.splitn(2, |byte| *byte == b'=');
            let name = parts.next()?;
            let value = parts.next()?;
            (name == key.as_bytes()).then(|| String::from_utf8_lossy(value).trim().to_string())
        })
        .filter(|value| !value.is_empty())
}

fn guess_game_name_from_proc(executable: &str) -> Option<String> {
    let wanted = Path::new(executable)
        .file_name()?
        .to_string_lossy()
        .to_ascii_lowercase();
    let entries = std::fs::read_dir("/proc").ok()?;
    for entry in entries.flatten() {
        let pid = entry.file_name().to_string_lossy().into_owned();
        if !pid.chars().all(|char| char.is_ascii_digit()) {
            continue;
        }
        let Ok(exe) = std::fs::read_link(entry.path().join("exe")) else {
            continue;
        };
        let name = exe
            .file_name()
            .map(|value| value.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if name != wanted && !name.starts_with(&wanted) {
            continue;
        }
        for key in ["STEAM_GAME", "STEAM_APP_ID", "HEROIC_APP_NAME", "GAME_NAME"] {
            if let Some(value) = proc_env(&pid, key) {
                return Some(value);
            }
        }
    }
    None
}

fn derive_game_name_with_proc(stem: &str) -> (String, String) {
    let (fallback, executable) = derive_game_name(stem);
    (
        guess_game_name_from_proc(&executable).unwrap_or(fallback),
        executable,
    )
}

pub fn scan_directory(
    conn: &mut Connection,
    directory: impl AsRef<Path>,
    min_file_age: u64,
    now: SystemTime,
    ingested_at: i64,
) -> Result<usize, String> {
    let mut entries = std::fs::read_dir(directory)
        .map_err(|error| format!("could not scan telemetry directory: {error}"))?
        .flatten()
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    let mut count = 0;
    for entry in entries {
        let path = entry.path();
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_none_or(|value| !value.eq_ignore_ascii_case("csv"))
        {
            continue;
        }
        let age = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|mtime| now.duration_since(mtime).ok());
        if age.is_none_or(|value| value < std::time::Duration::from_secs(min_file_age)) {
            continue;
        }
        if ingest_csv(conn, &path, ingested_at)? {
            count += 1;
        }
    }
    Ok(count)
}

pub fn current_unix_time() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn initializes_and_ingests_session_once() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("steam-game_2025-01-15_14:22:01.csv");
        std::fs::write(&path, "gpu,cpu,kernel,driver,cpu-scheduler\nAMD,Ryzen,6.1,Proton,scx_rusty\ntime,fps,frametime,cpu_load,gpu_load,gpu_temp,vram_used\n0,60,16.6,40,80,70,2048\n1,58,35,42,82,72,3072\n").unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();
        assert!(ingest_csv(&mut conn, &path, 200).unwrap());
        assert!(!ingest_csv(&mut conn, &path, 201).unwrap());
        assert_eq!(
            conn.query_row("SELECT game_name FROM sessions", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            "Steam Game"
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM frames", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
    }

    #[test]
    fn skips_csv_without_valid_fps() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("empty.csv");
        std::fs::write(&path, "gpu\nAMD\ntime,fps\n0,nan\n").unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();
        assert!(!ingest_csv(&mut conn, &path, 200).unwrap());
    }

    #[test]
    fn scans_only_stable_csv_files_and_preserves_mangohud_settings() {
        let directory = tempdir().unwrap();
        let config_home = directory.path().join("home");
        let sessions = config_home.join(".local/share/kyth/gaming-sessions");
        let config = config_home.join(".config/MangoHud/MangoHud.conf");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(&config, "fps_limit=60\noutput_folder=/old\n").unwrap();
        ensure_mangohud_output(&config_home, &sessions).unwrap();
        let configured = std::fs::read_to_string(config).unwrap();
        assert!(configured.contains("fps_limit=60"));
        assert!(configured.contains(&format!("output_folder={}", sessions.display())));

        let csv = sessions.join("scan-game_2025-01-15_14:22:01.csv");
        std::fs::write(&csv, "gpu\nAMD\ntime,fps,frametime\n0,60,16.6\n1,58,35\n").unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        initialize_database(&conn).unwrap();
        assert_eq!(
            scan_directory(
                &mut conn,
                &sessions,
                15,
                SystemTime::now() + std::time::Duration::from_secs(20),
                200
            )
            .unwrap(),
            1
        );
        assert_eq!(
            scan_directory(
                &mut conn,
                &sessions,
                15,
                SystemTime::now() + std::time::Duration::from_secs(20),
                201
            )
            .unwrap(),
            0
        );
    }
}
