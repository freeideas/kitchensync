use chrono::{DateTime, Utc, NaiveDateTime, TimeZone};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Format: YYYYMMDDTHHmmss.ffffffZ — UTC, microsecond precision
const FMT: &str = "%Y%m%dT%H%M%S%.6fZ";

static LAST_TS: Mutex<i64> = Mutex::new(0);

pub fn now() -> String {
    let mut last = LAST_TS.lock().unwrap();
    let mut micros = Utc::now().timestamp_micros();
    if micros <= *last {
        micros = *last + 1;
    }
    *last = micros;
    let dt = DateTime::from_timestamp_micros(micros).unwrap();
    dt.format(FMT).to_string()
}

pub fn parse(s: &str) -> Option<DateTime<Utc>> {
    // Format: YYYYMMDDTHHmmss.ffffffZ
    if s.len() < 22 {
        return None;
    }
    let naive = NaiveDateTime::parse_from_str(s, FMT).ok()?;
    Some(Utc.from_utc_datetime(&naive))
}

pub fn from_system_time(t: SystemTime) -> String {
    let dur = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let dt = DateTime::from_timestamp(dur.as_secs() as i64, dur.subsec_nanos()).unwrap();
    dt.format(FMT).to_string()
}

pub fn to_system_time(s: &str) -> Option<SystemTime> {
    let dt = parse(s)?;
    let secs = dt.timestamp();
    let nanos = dt.timestamp_subsec_nanos();
    Some(UNIX_EPOCH + std::time::Duration::new(secs as u64, nanos))
}

pub fn age_days(ts: &str) -> Option<f64> {
    let dt = parse(ts)?;
    let now = Utc::now();
    let diff = now.signed_duration_since(dt);
    Some(diff.num_seconds() as f64 / 86400.0)
}

/// Returns true if a and b are within tolerance_secs of each other.
pub fn within_tolerance(a: &str, b: &str, tolerance_secs: f64) -> bool {
    let da = match parse(a) { Some(d) => d, None => return false };
    let db = match parse(b) { Some(d) => d, None => return false };
    let diff = (da - db).num_milliseconds().unsigned_abs() as f64 / 1000.0;
    diff <= tolerance_secs
}

/// Compare two timestamps. Returns Ordering.
pub fn cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.cmp(b) // lexicographic sort works for this format
}

/// Returns true if a > b (a is newer).
pub fn is_newer(a: &str, b: &str) -> bool {
    a > b
}
