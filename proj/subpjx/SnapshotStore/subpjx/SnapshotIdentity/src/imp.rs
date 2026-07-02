use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::*;

const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const MICROSECONDS_PER_SECOND: i128 = 1_000_000;
const MICROSECONDS_PER_DAY: i128 = 86_400 * MICROSECONDS_PER_SECOND;
const XXH64_PRIME_1: u64 = 11_400_714_785_074_694_791;
const XXH64_PRIME_2: u64 = 14_029_467_366_897_019_727;
const XXH64_PRIME_3: u64 = 1_609_587_929_392_839_161;
const XXH64_PRIME_4: u64 = 9_650_029_242_287_828_579;
const XXH64_PRIME_5: u64 = 2_870_177_450_012_600_261;

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
    base62_11(xxhash64_seed_0(relative_path.as_bytes()))
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

fn xxhash64_seed_0(input: &[u8]) -> u64 {
    let mut offset = 0;
    let mut hash = if input.len() >= 32 {
        let mut v1 = XXH64_PRIME_1.wrapping_add(XXH64_PRIME_2);
        let mut v2 = XXH64_PRIME_2;
        let mut v3 = 0;
        let mut v4 = 0_u64.wrapping_sub(XXH64_PRIME_1);

        while offset <= input.len() - 32 {
            v1 = xxhash64_round(v1, read_u64(input, offset));
            v2 = xxhash64_round(v2, read_u64(input, offset + 8));
            v3 = xxhash64_round(v3, read_u64(input, offset + 16));
            v4 = xxhash64_round(v4, read_u64(input, offset + 24));
            offset += 32;
        }

        let mut combined = v1
            .rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18));
        combined = xxhash64_merge_round(combined, v1);
        combined = xxhash64_merge_round(combined, v2);
        combined = xxhash64_merge_round(combined, v3);
        xxhash64_merge_round(combined, v4)
    } else {
        XXH64_PRIME_5
    };

    hash = hash.wrapping_add(input.len() as u64);

    while offset + 8 <= input.len() {
        hash ^= xxhash64_round(0, read_u64(input, offset));
        hash = hash
            .rotate_left(27)
            .wrapping_mul(XXH64_PRIME_1)
            .wrapping_add(XXH64_PRIME_4);
        offset += 8;
    }

    if offset + 4 <= input.len() {
        hash ^= u64::from(read_u32(input, offset)).wrapping_mul(XXH64_PRIME_1);
        hash = hash
            .rotate_left(23)
            .wrapping_mul(XXH64_PRIME_2)
            .wrapping_add(XXH64_PRIME_3);
        offset += 4;
    }

    while offset < input.len() {
        hash ^= u64::from(input[offset]).wrapping_mul(XXH64_PRIME_5);
        hash = hash.rotate_left(11).wrapping_mul(XXH64_PRIME_1);
        offset += 1;
    }

    hash ^= hash >> 33;
    hash = hash.wrapping_mul(XXH64_PRIME_2);
    hash ^= hash >> 29;
    hash = hash.wrapping_mul(XXH64_PRIME_3);
    hash ^ (hash >> 32)
}

fn xxhash64_round(accumulator: u64, input: u64) -> u64 {
    accumulator
        .wrapping_add(input.wrapping_mul(XXH64_PRIME_2))
        .rotate_left(31)
        .wrapping_mul(XXH64_PRIME_1)
}

fn xxhash64_merge_round(accumulator: u64, value: u64) -> u64 {
    (accumulator ^ xxhash64_round(0, value))
        .wrapping_mul(XXH64_PRIME_1)
        .wrapping_add(XXH64_PRIME_4)
}

fn read_u64(input: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(input[offset..offset + 8].try_into().expect("slice has 8 bytes"))
}

fn read_u32(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(input[offset..offset + 4].try_into().expect("slice has 4 bytes"))
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
