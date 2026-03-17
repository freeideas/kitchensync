use chrono::{DateTime, Utc, TimeZone};
use std::sync::atomic::{AtomicU64, Ordering};

static LAST_TIMESTAMP_MICROS: AtomicU64 = AtomicU64::new(0);

/// Generate a monotonic timestamp in YYYYMMDDTHHmmss.ffffffZ format.
/// Guarantees uniqueness within a process by incrementing if needed.
pub fn now() -> String {
    let current = Utc::now();
    let micros = current.timestamp_micros() as u64;

    // Ensure monotonicity
    let last = LAST_TIMESTAMP_MICROS.load(Ordering::SeqCst);
    let actual_micros = if micros <= last {
        last + 1
    } else {
        micros
    };
    LAST_TIMESTAMP_MICROS.store(actual_micros, Ordering::SeqCst);

    // Convert back to DateTime
    let secs = (actual_micros / 1_000_000) as i64;
    let nanos = ((actual_micros % 1_000_000) * 1000) as u32;
    let dt = Utc.timestamp_opt(secs, nanos).unwrap();

    format_timestamp(&dt)
}

/// Format a DateTime to YYYYMMDDTHHmmss.ffffffZ format.
pub fn format_timestamp(dt: &DateTime<Utc>) -> String {
    dt.format("%Y%m%dT%H%M%S%.6fZ").to_string()
}

/// Parse a timestamp from YYYYMMDDTHHmmss.ffffffZ format.
pub fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    // Format: 20260314T091523.847291Z
    if s.len() != 24 || !s.ends_with('Z') || s.chars().nth(8) != Some('T') {
        return None;
    }

    let year: i32 = s[0..4].parse().ok()?;
    let month: u32 = s[4..6].parse().ok()?;
    let day: u32 = s[6..8].parse().ok()?;
    let hour: u32 = s[9..11].parse().ok()?;
    let minute: u32 = s[11..13].parse().ok()?;
    let second: u32 = s[13..15].parse().ok()?;
    let micros: u32 = s[16..22].parse().ok()?;

    Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
        .single()
        .map(|dt| dt + chrono::Duration::microseconds(micros as i64))
}

/// Compare two timestamps with 2-second tolerance.
/// Returns -1 if a < b, 0 if equal (within tolerance), 1 if a > b.
pub fn compare_timestamps(a: &str, b: &str) -> i32 {
    let dt_a = match parse_timestamp(a) {
        Some(dt) => dt,
        None => return 0,
    };
    let dt_b = match parse_timestamp(b) {
        Some(dt) => dt,
        None => return 0,
    };

    let diff = (dt_a - dt_b).num_milliseconds().abs();
    if diff <= 2000 {
        0
    } else if dt_a < dt_b {
        -1
    } else {
        1
    }
}

/// Check if timestamp is within 5 seconds of current time.
pub fn is_within_5_seconds(timestamp: &str) -> bool {
    let dt = match parse_timestamp(timestamp) {
        Some(dt) => dt,
        None => return false,
    };

    let now = Utc::now();
    let diff = (now - dt).num_seconds().abs();
    diff <= 5
}

/// Get timestamp from file modification time.
pub fn from_system_time(time: std::time::SystemTime) -> String {
    let dt: DateTime<Utc> = time.into();
    format_timestamp(&dt)
}
