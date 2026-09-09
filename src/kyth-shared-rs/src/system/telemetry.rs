//! Read-only telemetry session reader — ports `kyth_shared.telemetry.recent_sessions`.
//! Pure sqlite read against `~/.local/share/kyth/telemetry.db`, no writes, no root.
//! Matches Python exactly: tries extended schema (with `avg_latency_ms/p99`) first,
//! falls back to legacy schema and supplements from `/var/cache/kyth/telem/latency.jsonl`.

use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct SessionRow {
    pub game_name: String,
    pub started_at: Option<f64>,
    pub duration_s: Option<f64>,
    pub avg_fps: Option<f64>,
    pub p1_low_fps: Option<f64>,
    pub stutter_count: i64,
    pub scheduler: String,
    pub avg_latency_ms: Option<f64>,
    pub p99_latency_ms: Option<f64>,
}

fn telemetry_db_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".local/share/kyth/telemetry.db")
}

fn latency_ledger_path() -> PathBuf {
    PathBuf::from("/var/cache/kyth/telem/latency.jsonl")
}

fn load_latency_map() -> std::collections::HashMap<i64, (f64, f64)> {
    let mut m = std::collections::HashMap::new();
    let p = latency_ledger_path();
    let Ok(text) = std::fs::read_to_string(&p) else {
        return m;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(sa) = obj.get("started_at").and_then(|v| v.as_f64()) else {
            continue;
        };
        if sa == 0.0 {
            continue;
        }
        let avg = obj.get("avg_ms").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let p99 = obj.get("p99_ms").and_then(|v| v.as_f64()).unwrap_or(0.0);
        m.insert(sa as i64, (avg, p99));
    }
    m
}

/// Read-only — returns empty vec if DB missing or unreadable, matching Python's `[]`.
/// `limit` bounds the query; `telemetry_db_path()` respects `$HOME` like Python's `Path.home()`.
pub fn recent_sessions(limit: usize) -> Vec<SessionRow> {
    let db_path = telemetry_db_path();
    if !db_path.exists() {
        return vec![];
    }
    let latency_map = load_latency_map();
    // Use `rusqlite` if available, else fall back to empty — we keep this crate
    // dependency-free when sqlite isn't linked. The Tauri shell enables the
    // `rusqlite` feature; without it this returns `[]` and charts stay Preview.
    #[cfg(feature = "rusqlite")]
    {
        return recent_sessions_with_rusqlite(&db_path, limit, &latency_map);
    }
    #[cfg(not(feature = "rusqlite"))]
    {
        let _ = (limit, latency_map);
        vec![]
    }
}

#[cfg(feature = "rusqlite")]
fn recent_sessions_with_rusqlite(
    db_path: &std::path::Path,
    limit: usize,
    latency_map: &std::collections::HashMap<i64, (f64, f64)>,
) -> Vec<SessionRow> {
    use rusqlite::Connection;
    let Ok(conn) = Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return vec![];
    };
    // Try extended schema first
    let mut rows: Vec<SessionRow> = Vec::new();
    let extended = conn.prepare("SELECT game_name, started_at, duration_s, avg_fps, p1_low_fps, stutter_count, scheduler, avg_latency_ms, p99_latency_ms FROM sessions ORDER BY started_at DESC LIMIT ?1");
    if let Ok(mut stmt) = extended {
        if let Ok(mapped) = stmt.query_map([limit as i64], |row| {
            Ok(SessionRow {
                game_name: row
                    .get::<_, Option<String>>(0)?
                    .unwrap_or_else(|| "Unknown".to_string()),
                started_at: row.get::<_, Option<f64>>(1)?,
                duration_s: row.get::<_, Option<f64>>(2)?,
                avg_fps: row.get::<_, Option<f64>>(3)?,
                p1_low_fps: row.get::<_, Option<f64>>(4)?,
                stutter_count: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                scheduler: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                avg_latency_ms: row.get::<_, Option<f64>>(7)?,
                p99_latency_ms: row.get::<_, Option<f64>>(8)?,
            })
        }) {
            for item in mapped.flatten() {
                rows.push(item);
            }
            if !rows.is_empty() {
                return rows;
            }
        }
        // If extended failed due to missing columns, fall through to legacy
        if !rows.is_empty() {
            return rows;
        }
    }
    // Legacy schema
    let Ok(mut stmt) = conn.prepare("SELECT game_name, started_at, duration_s, avg_fps, p1_low_fps, stutter_count, scheduler FROM sessions ORDER BY started_at DESC LIMIT ?1") else { return vec![]; };
    let Ok(mapped) = stmt.query_map([limit as i64], |row| {
        Ok(SessionRow {
            game_name: row
                .get::<_, Option<String>>(0)?
                .unwrap_or_else(|| "Unknown".to_string()),
            started_at: row.get::<_, Option<f64>>(1)?,
            duration_s: row.get::<_, Option<f64>>(2)?,
            avg_fps: row.get::<_, Option<f64>>(3)?,
            p1_low_fps: row.get::<_, Option<f64>>(4)?,
            stutter_count: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
            scheduler: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
            avg_latency_ms: None,
            p99_latency_ms: None,
        })
    }) else {
        return vec![];
    };
    for mut item in mapped.flatten() {
        if let Some(sa) = item.started_at {
            if let Some((avg, p99)) = latency_map.get(&(sa as i64)) {
                item.avg_latency_ms = Some(*avg);
                item.p99_latency_ms = Some(*p99);
            }
        }
        rows.push(item);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_db_returns_empty() {
        std::env::set_var("HOME", "/tmp/nonexistent-kyth-test-home-xyz");
        assert!(recent_sessions(5).is_empty());
    }
}
