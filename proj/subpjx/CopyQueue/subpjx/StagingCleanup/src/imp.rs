use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use crate::api::*;

const DEFAULT_BAK_DAYS: u64 = 90;
const DEFAULT_TMP_DAYS: u64 = 2;

struct StagingCleanupImpl;

impl StagingCleanup for StagingCleanupImpl {
    fn cleanup(
        &self,
        fs: &dyn PeerFs,
        dir_path: &str,
        bak_keep_days: Option<u64>,
        tmp_keep_days: Option<u64>,
        now: SystemTime,
        dry_run: bool,
    ) {
        if dry_run {
            return;
        }
        let bak_limit = Duration::from_secs(bak_keep_days.unwrap_or(DEFAULT_BAK_DAYS) * 86400);
        let tmp_limit = Duration::from_secs(tmp_keep_days.unwrap_or(DEFAULT_TMP_DAYS) * 86400);
        purge_area(fs, dir_path, "BAK", bak_limit, now);
        purge_area(fs, dir_path, "TMP", tmp_limit, now);
    }
}

fn purge_area(fs: &dyn PeerFs, dir_path: &str, area: &str, limit: Duration, now: SystemTime) {
    let base = if dir_path.is_empty() {
        format!(".kitchensync/{}", area)
    } else {
        format!("{}/.kitchensync/{}", dir_path, area)
    };
    for name in fs.list(&base) {
        if let Some(ts) = parse_timestamp(&name) {
            if now.duration_since(ts).map_or(false, |age| age > limit) {
                fs.remove(&format!("{}/{}", base, name));
            }
        }
    }
}

/// Parse a directory entry name that is a timestamp.
/// Supports:
///   - Unix epoch seconds (10 decimal digits) or milliseconds (13 decimal digits)
///   - ISO 8601 extended:  YYYY-MM-DDTHH:MM:SS[Z]
///   - ISO 8601 compact:   YYYYMMDDTHHMMSS[Z]
///   - Underscore-separated: YYYYMMDD_HHMMSS or YYYY-MM-DD_HH:MM:SS
/// Returns None if the name cannot be parsed, causing the entry to be skipped.
fn parse_timestamp(name: &str) -> Option<SystemTime> {
    if name.bytes().all(|b| b.is_ascii_digit()) {
        if let Ok(n) = name.parse::<u64>() {
            let secs = if name.len() >= 13 { n / 1000 } else { n };
            return Some(UNIX_EPOCH + Duration::from_secs(secs));
        }
    }
    parse_datetime_str(name)
}

fn parse_datetime_str(s: &str) -> Option<SystemTime> {
    let s = s.strip_suffix('Z').unwrap_or(s);
    // Drop timezone offset starting at position 19 (after YYYY-MM-DDTHH:MM:SS)
    let s = if s.len() > 19 && matches!(s.as_bytes().get(19), Some(b'+') | Some(b'-')) {
        &s[..19]
    } else {
        s
    };

    // Locate the separator between date and time: 'T' anywhere, or '_' at position 8 or 10
    let sep = s.bytes().enumerate().find(|&(i, b)| {
        b == b'T' || (b == b'_' && (i == 8 || i == 10))
    });

    let (date_s, time_s) = match sep {
        Some((pos, _)) => (&s[..pos], &s[pos + 1..]),
        None => (s, ""),
    };

    let (y, mo, d) = parse_date_part(date_s)?;
    let (h, mi, sec) = if time_s.is_empty() { (0, 0, 0) } else { parse_time_part(time_s)? };

    let days = days_since_epoch(y, mo, d)?;
    let secs = days * 86400 + h as i64 * 3600 + mi as i64 * 60 + sec as i64;
    (secs >= 0).then(|| UNIX_EPOCH + Duration::from_secs(secs as u64))
}

fn parse_date_part(s: &str) -> Option<(u32, u32, u32)> {
    match s.len() {
        10 if s.as_bytes().get(4) == Some(&b'-') && s.as_bytes().get(7) == Some(&b'-') => {
            Some((s[..4].parse().ok()?, s[5..7].parse().ok()?, s[8..10].parse().ok()?))
        }
        8 if s.bytes().all(|b| b.is_ascii_digit()) => {
            Some((s[..4].parse().ok()?, s[4..6].parse().ok()?, s[6..8].parse().ok()?))
        }
        _ => None,
    }
}

fn parse_time_part(s: &str) -> Option<(u32, u32, u32)> {
    let s = if s.len() > 8 { &s[..8] } else { s };
    match s.len() {
        8 if (s.as_bytes().get(2) == Some(&b':') && s.as_bytes().get(5) == Some(&b':'))
            || (s.as_bytes().get(2) == Some(&b'-') && s.as_bytes().get(5) == Some(&b'-')) =>
        {
            Some((s[..2].parse().ok()?, s[3..5].parse().ok()?, s[6..8].parse().ok()?))
        }
        6 if s.bytes().all(|b| b.is_ascii_digit()) => {
            Some((s[..2].parse().ok()?, s[2..4].parse().ok()?, s[4..6].parse().ok()?))
        }
        _ => None,
    }
}

fn days_since_epoch(year: u32, month: u32, day: u32) -> Option<i64> {
    if year < 1970 || month < 1 || month > 12 || day < 1 {
        return None;
    }
    let is_leap = |y: u32| (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_len = |y: u32, m: u32| -> u32 {
        match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => if is_leap(y) { 29 } else { 28 },
            _ => 0,
        }
    };
    if day > month_len(year, month) {
        return None;
    }
    let mut days: i64 = 0;
    for y in 1970..year {
        days += if is_leap(y) { 366 } else { 365 };
    }
    for m in 1..month {
        days += month_len(year, m) as i64;
    }
    days += (day - 1) as i64;
    Some(days)
}

pub fn new() -> std::sync::Arc<dyn StagingCleanup> {
    Arc::new(StagingCleanupImpl)
}
