use chrono::{Utc, DateTime, NaiveDateTime};
use std::sync::Mutex;

static LAST_STAMP: Mutex<Option<DateTime<Utc>>> = Mutex::new(None);

pub fn now() -> String {
    let mut last = LAST_STAMP.lock().unwrap();
    let mut ts = Utc::now();
    if let Some(prev) = *last {
        if ts <= prev {
            ts = prev + chrono::Duration::microseconds(1);
        }
    }
    *last = Some(ts);
    format_timestamp(ts)
}

pub fn format_timestamp(dt: DateTime<Utc>) -> String {
    dt.format("%Y%m%dT%H%M%S%.6fZ").to_string()
}

pub fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    let naive = NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%S%.6fZ").ok()?;
    Some(naive.and_utc())
}

pub fn is_within_tolerance(a: &str, b: &str, seconds: i64) -> bool {
    match (parse_timestamp(a), parse_timestamp(b)) {
        (Some(ta), Some(tb)) => {
            let diff = (ta - tb).num_seconds().abs();
            diff <= seconds
        }
        _ => false,
    }
}
