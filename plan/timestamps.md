# Timestamps

## Risk

The specs use `YYYY-MM-DD_HH-mm-ss_ffffffZ` timestamps for database columns and
metadata directory names. The product needs UTC parsing, six-digit microsecond
formatting, five-second tolerance comparisons, and a process-local monotonic
microsecond bump.

## Experiment

`experiments/timestamps` is a Rust mini-project using:

- `chrono` `0.4.38`

It parses the seconds part with
`NaiveDateTime::parse_from_str(..., "%Y-%m-%d_%H-%M-%S")`, parses the final
six-digit field as microseconds, adds it with `Duration::microseconds`, and
wraps the result with `DateTime::<Utc>::from_naive_utc_and_offset`.

Formatting uses `DateTime::format("%Y-%m-%d_%H-%M-%S")` for the seconds and
`timestamp_subsec_micros()` for the six-digit suffix.

## Proved Calls

- `2024-01-01_12-00-00_123456Z` round-trips exactly.
- A timestamp exactly five seconds away is within tolerance.
- A timestamp five seconds and one microsecond away is outside tolerance.
- If a new timestamp is not greater than the previous generated timestamp,
  adding `Duration::microseconds(1)` produces the next monotonic value.

## Surprise

Chrono's `%f` parser is not the right direct parser for this spec string. In the
experiment, parsing `123456` through `%f` produced `000123` microseconds because
the token is nanosecond-oriented in this position. Split the final underscore
field and parse the six digits as microseconds explicitly.
