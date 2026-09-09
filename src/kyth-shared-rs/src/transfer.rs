//! Small, dependency-free transfer and byte-formatting helpers.
//!
//! These mirror the shared installer/welcome helpers.  They intentionally do
//! not own network polling or UI state; callers can use the deterministic
//! formatters from native Rust surfaces without crossing into Python.

use std::collections::VecDeque;
use std::fs;
use std::time::Instant;

pub fn parse_size_bytes(size: &str) -> u64 {
    let mut parts = size.split_whitespace();
    let Some(value) = parts.next().and_then(|value| value.parse::<f64>().ok()) else {
        return 0;
    };
    let unit = parts
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase()
        .trim_end_matches('B')
        .replace('I', "");
    let multiplier = match unit.as_str() {
        "" => 1_u64,
        "K" => 1024,
        "M" => 1024_u64.pow(2),
        "G" => 1024_u64.pow(3),
        "T" => 1024_u64.pow(4),
        _ => return 0,
    } as f64;
    if !value.is_finite() || value < 0.0 {
        return 0;
    }
    (value * multiplier) as u64
}

pub fn human_bytes(bytes: f64) -> String {
    if bytes < 1024.0 {
        return format_number(bytes, 0, "B");
    }
    let mut value = bytes;
    for unit in ["KB", "MB", "GB"] {
        value /= 1024.0;
        if value < 1024.0 {
            return format_number(value, 1, unit);
        }
    }
    format_number(value / 1024.0, 1, "TB")
}

fn format_number(value: f64, decimals: usize, unit: &str) -> String {
    if decimals == 0 {
        format!("{} {unit}", value as i64)
    } else {
        format!("{value:.decimals$} {unit}")
    }
}

pub fn human_bytes_pair(downloaded: u64, total: u64) -> (String, String) {
    for (unit, threshold) in [
        ("GB", 1024_u64.pow(3)),
        ("MB", 1024_u64.pow(2)),
        ("KB", 1024),
    ] {
        if total >= threshold {
            return (
                format!("{:.1}", downloaded as f64 / threshold as f64),
                format!("{:.1} {unit}", total as f64 / threshold as f64),
            );
        }
    }
    (downloaded.to_string(), format!("{total} B"))
}

/// Parse the receive-byte column from `/proc/net/dev`.
///
/// The Python shared helper treats a malformed proc file as unavailable and
/// returns zero. Keep the parsing function separate so callers and tests can
/// supply a captured proc payload without mutating global state.
pub fn rx_bytes_from_proc_net_dev(text: &str) -> Option<u64> {
    let mut total = 0_u64;
    for line in text.lines() {
        let Some((interface, data)) = line.split_once(':') else {
            continue;
        };
        if interface.trim() == "lo" {
            continue;
        }
        let value = data.split_whitespace().next()?.parse::<u64>().ok()?;
        total = total.checked_add(value)?;
    }
    Some(total)
}

/// Return total received bytes across non-loopback interfaces.
pub fn get_rx_bytes() -> u64 {
    fs::read_to_string("/proc/net/dev")
        .ok()
        .and_then(|text| rx_bytes_from_proc_net_dev(&text))
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkStats {
    pub downloaded: u64,
    pub total: u64,
    pub speed: u64,
    pub eta_sec: u64,
}

/// Compatibility name used by the original installer-oriented projection.
pub type TransferProgress = NetworkStats;

/// Rolling-window network throughput tracker shared by download UIs.
///
/// `tick_with_now` accepts an explicit monotonic timestamp so installer and
/// welcome tests can exercise the same logic without sleeping. Production
/// callers can use `tick`, which supplies `Instant::now()`.
#[derive(Debug, Clone)]
pub struct NetStatsTracker {
    total: u64,
    rx_start: u64,
    rx_prev: u64,
    t_prev: Instant,
    t_prev_seconds: f64,
    samples: VecDeque<f64>,
}

impl NetStatsTracker {
    pub fn new(total: u64, rx_start: u64) -> Self {
        Self::new_at(total, rx_start, Instant::now())
    }

    pub fn new_at(total: u64, rx_start: u64, now: Instant) -> Self {
        Self {
            total,
            rx_start,
            rx_prev: 0,
            t_prev: now,
            t_prev_seconds: 0.0,
            samples: VecDeque::with_capacity(5),
        }
    }

    pub fn tick(&mut self, rx_now: u64) -> NetworkStats {
        self.tick_with_now(rx_now, Instant::now())
    }

    pub fn tick_with_now(&mut self, rx_now: u64, now: Instant) -> NetworkStats {
        let downloaded = rx_now.saturating_sub(self.rx_start).min(self.total);
        let elapsed = now.saturating_duration_since(self.t_prev).as_secs_f64();
        if elapsed > 0.0 && self.rx_prev > 0 {
            let delta = rx_now.saturating_sub(self.rx_prev);
            if delta > 0 {
                if self.samples.len() == 5 {
                    self.samples.pop_front();
                }
                self.samples.push_back(delta as f64 / elapsed);
            }
        }
        self.rx_prev = rx_now;
        self.t_prev = now;
        let speed = if self.samples.is_empty() {
            0
        } else {
            (self.samples.iter().sum::<f64>() / self.samples.len() as f64) as u64
        };
        let remaining = self.total.saturating_sub(downloaded);
        let eta_sec = if speed > 0 { remaining / speed } else { 0 };
        NetworkStats {
            downloaded,
            total: self.total,
            speed,
            eta_sec,
        }
    }

    /// Test-friendly variant that accepts a monotonic timestamp in seconds.
    pub fn tick_at(&mut self, rx_now: u64, time_now: f64) -> TransferProgress {
        let downloaded = rx_now.saturating_sub(self.rx_start).min(self.total);
        let elapsed = time_now - self.t_prev_seconds;
        if elapsed > 0.0 && self.rx_prev > 0 {
            let delta = rx_now.saturating_sub(self.rx_prev);
            if delta > 0 {
                if self.samples.len() == 5 {
                    self.samples.pop_front();
                }
                self.samples.push_back(delta as f64 / elapsed);
            }
        }
        self.rx_prev = rx_now;
        self.t_prev_seconds = time_now;
        let speed = if self.samples.is_empty() {
            0
        } else {
            (self.samples.iter().sum::<f64>() / self.samples.len() as f64) as u64
        };
        NetworkStats {
            downloaded,
            total: self.total,
            speed,
            eta_sec: if speed > 0 {
                self.total.saturating_sub(downloaded) / speed
            } else {
                0
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn parses_iec_and_rejects_invalid_sizes() {
        assert_eq!(
            parse_size_bytes("8.3 GB"),
            (8.3_f64 * 1024_u64.pow(3) as f64) as u64
        );
        assert_eq!(parse_size_bytes("2 GiB"), 2 * 1024_u64.pow(3));
        assert_eq!(parse_size_bytes("not a size"), 0);
        assert_eq!(parse_size_bytes("-1 GB"), 0);
    }

    #[test]
    fn formats_bytes_and_download_pairs() {
        assert_eq!(human_bytes(1024.0), "1.0 KB");
        assert_eq!(human_bytes(1.0), "1 B");
        assert_eq!(
            human_bytes_pair(1_400_000, 1_500_000),
            ("1.3".into(), "1.4 MB".into())
        );
        assert_eq!(human_bytes_pair(10, 12), ("10".into(), "12 B".into()));
    }

    #[test]
    fn tracks_rolling_transfer_rate_without_polling() {
        let mut tracker = NetStatsTracker::new(1_000, 100);
        assert_eq!(tracker.tick_at(100, 1.0).speed, 0);
        let progress = tracker.tick_at(300, 3.0);
        assert_eq!(progress.downloaded, 200);
        assert_eq!(progress.speed, 100);
        assert_eq!(progress.eta_sec, 8);
    }

    #[test]
    fn parses_proc_receive_bytes_without_counting_loopback() {
        let proc = "Inter-| Receive |\n lo: 999 0 0\n eth0: 120 0 0\n wlan0: 80 0 0\n";
        assert_eq!(rx_bytes_from_proc_net_dev(proc), Some(200));
        assert_eq!(rx_bytes_from_proc_net_dev("eth0: not-a-counter"), None);
    }

    #[test]
    fn tracks_download_window_and_eta_with_an_injected_clock() {
        let start = Instant::now();
        let mut tracker = NetStatsTracker::new_at(1_000, 100, start);
        assert_eq!(
            tracker.tick_with_now(100, start + Duration::from_secs(1)),
            NetworkStats {
                downloaded: 0,
                total: 1_000,
                speed: 0,
                eta_sec: 0
            }
        );
        let stats = tracker.tick_with_now(600, start + Duration::from_secs(2));
        assert_eq!(stats.downloaded, 500);
        assert_eq!(stats.speed, 500);
        assert_eq!(stats.eta_sec, 1);
        assert_eq!(
            tracker
                .tick_with_now(1_500, start + Duration::from_secs(3))
                .downloaded,
            1_000
        );
    }
}
