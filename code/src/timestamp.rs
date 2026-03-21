use chrono::Utc;
use std::sync::Mutex;

static LAST_TS: Mutex<Option<i64>> = Mutex::new(None);

/// Format: YYYY-MM-DD_HH-mm-ss_ffffffZ — UTC, microsecond precision, monotonic within process.
pub fn now() -> String {
    let mut last = LAST_TS.lock().unwrap();
    let mut us = Utc::now().timestamp_micros();
    if let Some(prev) = *last {
        if us <= prev {
            us = prev + 1;
        }
    }
    *last = Some(us);
    format_micros(us)
}

pub fn format_micros(us: i64) -> String {
    let secs = us.div_euclid(1_000_000);
    let frac = us.rem_euclid(1_000_000);
    let dt = chrono::DateTime::from_timestamp(secs, 0).unwrap();
    format!(
        "{}_{}Z",
        dt.format("%Y-%m-%d_%H-%M-%S"),
        format!("{:06}", frac)
    )
}

pub fn parse_to_micros(s: &str) -> Option<i64> {
    // YYYY-MM-DD_HH-mm-ss_ffffffZ
    if s.len() != 27 || !s.ends_with('Z') {
        return None;
    }
    let s = &s[..26];
    let parts: Vec<&str> = s.split('_').collect();
    if parts.len() != 3 {
        return None;
    }
    let date_parts: Vec<&str> = parts[0].split('-').collect();
    if date_parts.len() != 3 {
        return None;
    }
    let time_parts: Vec<&str> = parts[1].split('-').collect();
    if time_parts.len() != 3 {
        return None;
    }
    let year: i32 = date_parts[0].parse().ok()?;
    let month: u32 = date_parts[1].parse().ok()?;
    let day: u32 = date_parts[2].parse().ok()?;
    let hour: u32 = time_parts[0].parse().ok()?;
    let min: u32 = time_parts[1].parse().ok()?;
    let sec: u32 = time_parts[2].parse().ok()?;
    let frac: i64 = parts[2].parse().ok()?;

    let dt = chrono::NaiveDate::from_ymd_opt(year, month, day)?
        .and_hms_opt(hour, min, sec)?
        .and_utc();
    Some(dt.timestamp() * 1_000_000 + frac)
}

/// Timestamp tolerance: 5 seconds = 5_000_000 microseconds
pub const TOLERANCE_US: i64 = 5_000_000;

pub fn within_tolerance(a: i64, b: i64) -> bool {
    (a - b).abs() <= TOLERANCE_US
}
