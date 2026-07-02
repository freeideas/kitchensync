use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::*;

const MICROSECONDS_PER_SECOND: i128 = 1_000_000;
const MICROSECONDS_PER_DAY: i128 = 86_400 * MICROSECONDS_PER_SECOND;

struct StagingCleanupImpl;

impl StagingCleanup for StagingCleanupImpl {
    fn clean_expired_staging(
        &self,
        request: StagingCleanupRequest,
        file_operations: &dyn StagingCleanupFileOperations,
    ) -> Result<(), StagingCleanupFailure> {
        clean_area(
            &request,
            file_operations,
            StagingCleanupArea::Bak,
            "BAK",
            request.keep_bak_days,
        )?;
        clean_area(
            &request,
            file_operations,
            StagingCleanupArea::Tmp,
            "TMP",
            request.keep_tmp_days,
        )
    }
}

pub fn new() -> std::sync::Arc<dyn StagingCleanup> {
    Arc::new(StagingCleanupImpl)
}

fn clean_area(
    request: &StagingCleanupRequest,
    file_operations: &dyn StagingCleanupFileOperations,
    area: StagingCleanupArea,
    area_name: &str,
    keep_days: u64,
) -> Result<(), StagingCleanupFailure> {
    let root = join_path(
        &join_path(&request.parent_directory, ".kitchensync"),
        area_name,
    );
    let listing = file_operations
        .list_direct_timestamp_directories(&request.peer, &root)
        .map_err(|error| failure(
            request,
            area,
            root.clone(),
            None,
            StagingCleanupFailureOperation::InspectCleanupRoot,
            StagingCleanupFailureCause::Filesystem(error),
        ))?;

    let StagingCleanupDirectoryListing::Present {
        direct_timestamp_directories,
    } = listing
    else {
        return Ok(());
    };

    let current_microseconds = system_time_to_microseconds(request.current_time);
    let keep_microseconds = i128::from(keep_days) * MICROSECONDS_PER_DAY;

    for timestamp_directory in direct_timestamp_directories {
        let timestamp_path = join_path(&root, &timestamp_directory);
        let timestamp_microseconds =
            parse_timestamp_microseconds(&timestamp_directory).map_err(|message| {
                failure(
                    request,
                    area,
                    timestamp_path.clone(),
                    Some(timestamp_directory.clone()),
                    StagingCleanupFailureOperation::DetermineTimestampAge,
                    StagingCleanupFailureCause::InvalidTimestamp { message },
                )
            })?;

        if current_microseconds - timestamp_microseconds > keep_microseconds {
            file_operations
                .remove_timestamp_directory_tree(&request.peer, &timestamp_path)
                .map_err(|error| failure(
                    request,
                    area,
                    timestamp_path,
                    Some(timestamp_directory),
                    StagingCleanupFailureOperation::RemoveTimestampDirectory,
                    StagingCleanupFailureCause::Filesystem(error),
                ))?;
        }
    }

    Ok(())
}

fn failure(
    request: &StagingCleanupRequest,
    area: StagingCleanupArea,
    failed_path: String,
    timestamp_directory: Option<String>,
    operation: StagingCleanupFailureOperation,
    cause: StagingCleanupFailureCause,
) -> StagingCleanupFailure {
    StagingCleanupFailure {
        peer: request.peer.clone(),
        parent_directory: request.parent_directory.clone(),
        area,
        failed_path,
        timestamp_directory,
        operation,
        cause,
    }
}

fn parse_timestamp_microseconds(timestamp: &str) -> Result<i128, String> {
    let bytes = timestamp.as_bytes();
    if bytes.len() != 27
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'_'
        || bytes[13] != b'-'
        || bytes[16] != b'-'
        || bytes[19] != b'_'
        || bytes[26] != b'Z'
    {
        return Err("timestamp must use YYYY-MM-DD_HH-mm-ss_ffffffZ".to_string());
    }

    let year = parse_decimal(&bytes[0..4], "year")?;
    let month = parse_decimal(&bytes[5..7], "month")?;
    let day = parse_decimal(&bytes[8..10], "day")?;
    let hour = parse_decimal(&bytes[11..13], "hour")?;
    let minute = parse_decimal(&bytes[14..16], "minute")?;
    let second = parse_decimal(&bytes[17..19], "second")?;
    let micros = parse_decimal(&bytes[20..26], "microseconds")?;

    validate_timestamp_parts(year, month, day, hour, minute, second)?;

    let days = days_from_civil(year, month, day);
    let day_microseconds =
        ((hour * 3_600 + minute * 60 + second) * MICROSECONDS_PER_SECOND) + micros;
    Ok(days * MICROSECONDS_PER_DAY + day_microseconds)
}

fn parse_decimal(bytes: &[u8], name: &str) -> Result<i128, String> {
    let mut value = 0_i128;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return Err(format!("timestamp {name} must contain only decimal digits"));
        }
        value = value * 10 + i128::from(*byte - b'0');
    }
    Ok(value)
}

fn validate_timestamp_parts(
    year: i128,
    month: i128,
    day: i128,
    hour: i128,
    minute: i128,
    second: i128,
) -> Result<(), String> {
    if !(1..=12).contains(&month) {
        return Err("timestamp month is outside 01..12".to_string());
    }
    if day < 1 || day > days_in_month(year, month) {
        return Err("timestamp day is outside the valid range for its month".to_string());
    }
    if hour > 23 {
        return Err("timestamp hour is outside 00..23".to_string());
    }
    if minute > 59 {
        return Err("timestamp minute is outside 00..59".to_string());
    }
    if second > 59 {
        return Err("timestamp second is outside 00..59".to_string());
    }
    Ok(())
}

fn days_in_month(year: i128, month: i128) -> i128 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i128) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_from_civil(year: i128, month: i128, day: i128) -> i128 {
    let adjusted_year = year - if month <= 2 { 1 } else { 0 };
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let month_parameter = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_parameter + 2) / 5 + day - 1;
    let day_of_era =
        year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

    era * 146_097 + day_of_era - 719_468
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

fn join_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{}/{}", parent, child)
    }
}
