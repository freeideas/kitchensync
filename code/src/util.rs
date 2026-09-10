//! Timestamps, path hashing, and small helpers shared across modules.
//! See specs/database.md, sections "Path Hashing" and "Timestamps".

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn format_micros(micros: i64) -> String {
    let secs = micros.div_euclid(1_000_000);
    let frac = micros.rem_euclid(1_000_000);
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}_{:06}Z",
        y, m, d, sod / 3600, (sod % 3600) / 60, sod % 60, frac
    )
}

/// Parse the timestamp format back to microseconds since the epoch.
pub fn parse_time(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 27 || b[26] != b'Z' {
        return None;
    }
    let num = |a: usize, l: usize| -> Option<i64> { s.get(a..a + l)?.parse::<i64>().ok() };
    let (y, mo, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (h, mi, se) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    let frac = num(20, 6)?;
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let days = days_from_civil(y, mo, d);
    Some(((days * 86_400) + h * 3600 + mi * 60 + se) * 1_000_000 + frac)
}

pub fn micros_to_system(micros: i64) -> SystemTime {
    if micros >= 0 {
        UNIX_EPOCH + Duration::from_micros(micros as u64)
    } else {
        UNIX_EPOCH - Duration::from_micros((-micros) as u64)
    }
}

pub fn system_to_micros(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_micros() as i64,
        Err(e) => -(e.duration().as_micros() as i64),
    }
}

// Howard Hinnant's civil date algorithms.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

static LAST_NOW: Mutex<i64> = Mutex::new(0);

/// Process-monotonic "now" in microseconds: strictly greater than every value
/// previously returned in this process (adds 1us on collision).
pub fn now_micros() -> i64 {
    let wall = system_to_micros(SystemTime::now());
    let mut last = LAST_NOW.lock().unwrap();
    let v = if wall > *last { wall } else { *last + 1 };
    *last = v;
    v
}

/// Fresh monotonic timestamp string.
pub fn now_string() -> String {
    format_micros(now_micros())
}

pub fn basename(rel_path: &str) -> &str {
    match rel_path.rfind('/') {
        Some(i) => &rel_path[i + 1..],
        None => rel_path,
    }
}

pub fn parent_path(rel_path: &str) -> &str {
    match rel_path.rfind('/') {
        Some(i) => &rel_path[..i],
        None => "",
    }
}

/// Percent-encode a basename so it is a single safe path segment on every
/// transport (used for SWAP directory names).
pub fn encode_segment(name: &str) -> String {
    use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
    const SET: &AsciiSet = &CONTROLS.add(b'%').add(b'/').add(b'\\').add(b':').add(b'*').add(b'?').add(b'"').add(b'<').add(b'>').add(b'|');
    utf8_percent_encode(name, SET).to_string()
}

pub fn decode_segment(seg: &str) -> String {
    percent_encoding::percent_decode_str(seg).decode_utf8_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        let s = "2024-01-01_12-00-00_000000Z";
        assert_eq!(format_micros(parse_time(s).unwrap()), s);
        assert_eq!(parse_time("1970-01-01_00-00-00_000000Z"), Some(0));
        assert_eq!(format_micros(0), "1970-01-01_00-00-00_000000Z");
    }
}
