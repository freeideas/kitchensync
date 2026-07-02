use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::*;
use xxhash_rust::xxh64::xxh64;

const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const MICROSECONDS_PER_SECOND: i128 = 1_000_000;
const MICROSECONDS_PER_DAY: i128 = 86_400 * MICROSECONDS_PER_SECOND;

struct SnapshotIdentityImpl {
    last_generated_microseconds: Mutex<Option<i128>>,
}

impl SnapshotIdentity for SnapshotIdentityImpl {
    fn path_id(&self, relative_path: &str) -> SnapshotIdentityResult<String> {
        validate_relative_path(relative_path)?;
        Ok(path_id_unchecked(relative_path))
    }

    fn parent_path_id(&self, relative_path: &str) -> SnapshotIdentityResult<String> {
        validate_relative_path(relative_path)?;
        match relative_path.rsplit_once('/') {
            Some((parent_path, _)) => Ok(path_id_unchecked(parent_path)),
            None => Ok(SNAPSHOT_ROOT_PARENT_ID.to_string()),
        }
    }

    fn format_utc_timestamp(&self, time: SystemTime) -> SnapshotIdentityResult<String> {
        format_microseconds(system_time_to_microseconds(time))
    }

    fn generate_timestamp(&self) -> SnapshotIdentityResult<String> {
        let current = system_time_to_microseconds(SystemTime::now());

        let mut last_generated = self
            .last_generated_microseconds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let selected = match *last_generated {
            Some(previous) if current <= previous => previous + 1,
            _ => current,
        };
        let formatted = format_microseconds(selected)?;
        *last_generated = Some(selected);
        Ok(formatted)
    }
}

pub fn new() -> std::sync::Arc<dyn SnapshotIdentity> {
    Arc::new(SnapshotIdentityImpl {
        last_generated_microseconds: Mutex::new(None),
    })
}

fn validate_relative_path(relative_path: &str) -> SnapshotIdentityResult<()> {
    if relative_path.is_empty()
        || relative_path.starts_with('/')
        || relative_path.ends_with('/')
        || relative_path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(error(
            SnapshotIdentityErrorKind::InvalidRelativePath,
            "relative path must name an entry below the sync root",
        ));
    }

    Ok(())
}

fn path_id_unchecked(relative_path: &str) -> String {
    base62_11(xxh64(relative_path.as_bytes(), 0))
}

fn base62_11(mut value: u64) -> String {
    let mut out = [b'0'; 11];
    for slot in out.iter_mut().rev() {
        *slot = BASE62[(value % 62) as usize];
        value /= 62;
    }
    String::from_utf8(out.to_vec()).expect("base62 alphabet is ASCII")
}

fn system_time_to_microseconds(time: SystemTime) -> i128 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => {
            i128::from(duration.as_secs()) * MICROSECONDS_PER_SECOND
                + i128::from(duration.subsec_micros())
        }
        Err(error) => {
            let duration = error.duration();
            let whole_microseconds = i128::from(duration.as_secs()) * MICROSECONDS_PER_SECOND;
            let fractional_microseconds =
                (i128::from(duration.subsec_nanos()) + 999) / 1_000;
            -(whole_microseconds + fractional_microseconds)
        }
    }
}

fn format_microseconds(microseconds: i128) -> SnapshotIdentityResult<String> {
    let days = microseconds.div_euclid(MICROSECONDS_PER_DAY);
    let day_microseconds = microseconds.rem_euclid(MICROSECONDS_PER_DAY);
    let (year, month, day) = civil_from_days(days);

    if !(0..=9999).contains(&year) {
        return Err(error(
            SnapshotIdentityErrorKind::TimestampOutOfRange,
            "timestamp year is outside YYYY range",
        ));
    }

    let total_seconds = day_microseconds / MICROSECONDS_PER_SECOND;
    let hour = total_seconds / 3_600;
    let minute = (total_seconds % 3_600) / 60;
    let second = total_seconds % 60;
    let micros = day_microseconds % MICROSECONDS_PER_SECOND;

    Ok(format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}_{:06}Z",
        year, month, day, hour, minute, second, micros
    ))
}

fn civil_from_days(days_since_unix_epoch: i128) -> (i128, i128, i128) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year =
        day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_param = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_param + 2) / 5 + 1;
    let month = month_param + if month_param < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }

    (year, month, day)
}

fn error(kind: SnapshotIdentityErrorKind, message: &str) -> SnapshotIdentityError {
    SnapshotIdentityError {
        kind,
        message: message.to_string(),
    }
}
