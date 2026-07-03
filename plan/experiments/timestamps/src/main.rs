use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use std::error::Error;

fn parse_stamp(text: &str) -> Result<DateTime<Utc>, Box<dyn Error>> {
    let without_z = text.strip_suffix('Z').ok_or("timestamp must end in Z")?;
    let (seconds, micros) = without_z
        .rsplit_once('_')
        .ok_or("timestamp must contain a microsecond field")?;
    assert_eq!(micros.len(), 6, "microsecond field must be six digits");
    let micros: i64 = micros.parse()?;
    let naive = NaiveDateTime::parse_from_str(seconds, "%Y-%m-%d_%H-%M-%S")?
        + Duration::microseconds(micros);
    Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
}

fn format_stamp(value: DateTime<Utc>) -> String {
    format!(
        "{}_{:06}Z",
        value.format("%Y-%m-%d_%H-%M-%S"),
        value.timestamp_subsec_micros()
    )
}

fn within_five_seconds(a: DateTime<Utc>, b: DateTime<Utc>) -> bool {
    (a - b).num_microseconds().unwrap().abs() <= 5_000_000
}

fn next_monotonic(candidate: DateTime<Utc>, previous: DateTime<Utc>) -> DateTime<Utc> {
    if candidate <= previous {
        previous + Duration::microseconds(1)
    } else {
        candidate
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let parsed = parse_stamp("2024-01-01_12-00-00_123456Z")?;
    assert_eq!(format_stamp(parsed), "2024-01-01_12-00-00_123456Z");

    let same = parse_stamp("2024-01-01_12-00-05_123456Z")?;
    let too_late = parse_stamp("2024-01-01_12-00-05_123457Z")?;
    assert!(within_five_seconds(parsed, same));
    assert!(!within_five_seconds(parsed, too_late));

    let first = parse_stamp("2024-01-01_12-00-00_000000Z")?;
    let repeated_clock = parse_stamp("2024-01-01_12-00-00_000000Z")?;
    let second = next_monotonic(repeated_clock, first);
    assert_eq!(format_stamp(second), "2024-01-01_12-00-00_000001Z");

    println!("checked chrono parsing, six-digit UTC formatting, tolerance, and monotonic microsecond bump");
    Ok(())
}
