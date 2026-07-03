use std::hash::Hasher;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use twox_hash::XxHash64;

use crate::api::*;

const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const FIVE_SECONDS_MICROS: i64 = 5_000_000;

struct FormatRulesImpl {
    last_generated: Mutex<Option<DateTime<Utc>>>,
}

impl FormatRules for FormatRulesImpl {
    fn normalize_peer_identity(
        &self,
        request: FormatRulesPeerIdentityRequest,
    ) -> Result<String, FormatRulesValidationError> {
        normalize_peer_identity_request(request)
    }

    fn validate_relative_path(
        &self,
        path: &str,
    ) -> Result<String, FormatRulesValidationError> {
        validate_relative_path_text(path)?;
        Ok(path.to_string())
    }

    fn snapshot_path_ids(
        &self,
        relative_path: &str,
    ) -> Result<FormatRulesSnapshotPathIds, FormatRulesValidationError> {
        if relative_path.is_empty() {
            return Err(validation_error(FormatRulesValidationErrorKind::RootSnapshotPath));
        }
        validate_relative_path_text(relative_path)?;
        let parent = match relative_path.rsplit_once('/') {
            Some((parent, _)) => parent,
            None => "/",
        };
        Ok(FormatRulesSnapshotPathIds {
            id: snapshot_id(relative_path.as_bytes()),
            parent_id: snapshot_id(parent.as_bytes()),
        })
    }

    fn parse_timestamp(
        &self,
        timestamp: &str,
    ) -> Result<FormatRulesTimestamp, FormatRulesValidationError> {
        parse_timestamp_text(timestamp)?;
        Ok(FormatRulesTimestamp {
            text: timestamp.to_string(),
        })
    }

    fn format_timestamp(&self, timestamp: SystemTime) -> FormatRulesTimestamp {
        let value = DateTime::<Utc>::from(timestamp);
        FormatRulesTimestamp {
            text: format_datetime(value),
        }
    }

    fn current_timestamp(&self) -> FormatRulesTimestamp {
        let mut last = self.last_generated.lock().expect("timestamp mutex poisoned");
        let now = Utc::now();
        let next = match *last {
            Some(previous) if now <= previous => previous + Duration::microseconds(1),
            _ => now,
        };
        *last = Some(next);
        FormatRulesTimestamp {
            text: format_datetime(next),
        }
    }

    fn timestamp_text(&self, timestamp: &FormatRulesTimestamp) -> String {
        timestamp.text.clone()
    }

    fn timestamp_system_time(&self, timestamp: &FormatRulesTimestamp) -> SystemTime {
        let value = parse_timestamp_text(&timestamp.text).expect("stored timestamp is invalid");
        SystemTime::from(value)
    }

    fn confirmed_absence_deleted_time(
        &self,
        existing_last_seen: &FormatRulesTimestamp,
        existing_deleted_time: Option<&FormatRulesTimestamp>,
    ) -> FormatRulesDeletionEstimateUpdate {
        if existing_deleted_time.is_some() {
            FormatRulesDeletionEstimateUpdate::NoWrite
        } else {
            FormatRulesDeletionEstimateUpdate::Write(existing_last_seen.clone())
        }
    }

    fn displacement_deleted_time(
        &self,
        existing_last_seen: &FormatRulesTimestamp,
    ) -> FormatRulesTimestamp {
        existing_last_seen.clone()
    }

    fn displacement_cascade_deleted_time(
        &self,
        displaced_deleted_time: &FormatRulesTimestamp,
    ) -> FormatRulesTimestamp {
        displaced_deleted_time.clone()
    }

    fn bak_directory_path(
        &self,
        parent_relative_path: Option<&str>,
        timestamp: &FormatRulesTimestamp,
    ) -> Result<String, FormatRulesValidationError> {
        let base = metadata_parent_prefix(parent_relative_path)?;
        Ok(format!("{base}.kitchensync/BAK/{}/", timestamp.text))
    }

    fn tmp_directory_path(&self, timestamp: &FormatRulesTimestamp) -> String {
        format!(".kitchensync/TMP/{}/", timestamp.text)
    }

    fn user_swap_paths(
        &self,
        parent_relative_path: Option<&str>,
        target_basename: &str,
    ) -> Result<FormatRulesUserSwapPaths, FormatRulesValidationError> {
        let encoded = encode_swap_basename(target_basename)?;
        let base = metadata_parent_prefix(parent_relative_path)?;
        let directory_path = format!("{base}.kitchensync/SWAP/{encoded}");
        Ok(FormatRulesUserSwapPaths {
            new_path: format!("{directory_path}/new"),
            old_path: format!("{directory_path}/old"),
            directory_path,
        })
    }

    fn snapshot_swap_paths(&self) -> FormatRulesSnapshotSwapPaths {
        FormatRulesSnapshotSwapPaths {
            new_path: ".kitchensync/SWAP/snapshot.db/new".to_string(),
            old_path: ".kitchensync/SWAP/snapshot.db/old".to_string(),
        }
    }

    fn file_mod_times_same(
        &self,
        current_mod_time: &FormatRulesTimestamp,
        snapshot_mod_time: &FormatRulesTimestamp,
    ) -> bool {
        absolute_difference_micros(current_mod_time, snapshot_mod_time) <= FIVE_SECONDS_MICROS
    }

    fn peer_mod_time_tied_with_max(
        &self,
        candidate_mod_time: &FormatRulesTimestamp,
        max_mod_time: &FormatRulesTimestamp,
    ) -> bool {
        signed_difference_micros(max_mod_time, candidate_mod_time) <= FIVE_SECONDS_MICROS
    }

    fn peer_mod_time_older_than_max(
        &self,
        candidate_mod_time: &FormatRulesTimestamp,
        max_mod_time: &FormatRulesTimestamp,
    ) -> bool {
        signed_difference_micros(max_mod_time, candidate_mod_time) > FIVE_SECONDS_MICROS
    }

    fn deletion_estimate_wins_over_file_mod_time(
        &self,
        deletion_estimate: &FormatRulesTimestamp,
        file_mod_time: &FormatRulesTimestamp,
    ) -> bool {
        signed_difference_micros(deletion_estimate, file_mod_time) > FIVE_SECONDS_MICROS
    }

    fn absent_unconfirmed_file_counts_as_deletion(
        &self,
        last_seen: &FormatRulesTimestamp,
        max_live_file_mod_time: &FormatRulesTimestamp,
    ) -> bool {
        signed_difference_micros(last_seen, max_live_file_mod_time) > FIVE_SECONDS_MICROS
    }

    fn directory_live_file_timestamp_evidence(
        &self,
        live_file_mod_times: &[FormatRulesTimestamp],
    ) -> Option<FormatRulesTimestamp> {
        live_file_mod_times
            .iter()
            .max_by_key(|timestamp| parse_timestamp_text(&timestamp.text).expect("stored timestamp is invalid"))
            .cloned()
    }

    fn directory_deletion_estimate_newer_than_live_file_evidence(
        &self,
        deletion_estimate: &FormatRulesTimestamp,
        newest_live_file_mod_time: &FormatRulesTimestamp,
    ) -> bool {
        signed_difference_micros(deletion_estimate, newest_live_file_mod_time) > FIVE_SECONDS_MICROS
    }
}

fn normalize_peer_identity_request(
    request: FormatRulesPeerIdentityRequest,
) -> Result<String, FormatRulesValidationError> {
    match split_scheme(&request.peer_url) {
        Some((scheme, rest)) => match scheme.to_ascii_lowercase().as_str() {
            "file" => normalize_file_identity(rest, &request.current_working_directory, true),
            "sftp" => normalize_sftp_identity(rest, request.os_username.as_deref()),
            _ => Err(validation_error(FormatRulesValidationErrorKind::InvalidPeerUrl)),
        },
        None => normalize_file_identity(&request.peer_url, &request.current_working_directory, false),
    }
}

fn split_scheme(text: &str) -> Option<(&str, &str)> {
    let marker = text.find("://")?;
    let scheme = &text[..marker];
    if scheme.is_empty() {
        return None;
    }
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic()
        || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.')
    {
        return None;
    }
    Some((scheme, &text[marker + 3..]))
}

fn normalize_file_identity(
    rest: &str,
    current_working_directory: &PathBuf,
    has_file_scheme: bool,
) -> Result<String, FormatRulesValidationError> {
    let path_text = strip_query_and_fragment(rest);
    let path_text = if path_text.starts_with('/') || path_text.is_empty() {
        path_text.to_string()
    } else if let Some(path_without_authority) = path_text.strip_prefix("localhost/") {
        format!("/{path_without_authority}")
    } else if looks_like_windows_absolute_path(path_text) {
        path_text.to_string()
    } else if has_file_scheme && path_text.contains('/') {
        return Err(validation_error(FormatRulesValidationErrorKind::InvalidPeerUrl));
    } else {
        path_text.to_string()
    };

    let absolute = if is_absolute_path_text(&path_text) {
        path_text
    } else {
        let cwd = path_to_slash_text(current_working_directory)?;
        if !is_absolute_path_text(&cwd) {
            return Err(validation_error(FormatRulesValidationErrorKind::InvalidPeerUrl));
        }
        if cwd.ends_with('/') {
            format!("{cwd}{path_text}")
        } else {
            format!("{cwd}/{path_text}")
        }
    };

    let normalized = normalize_url_path(&absolute)?;
    let normalized = if looks_like_windows_absolute_path(&normalized) {
        format!("/{normalized}")
    } else {
        normalized
    };
    Ok(format!("file://{normalized}"))
}

fn normalize_sftp_identity(
    rest: &str,
    os_username: Option<&str>,
) -> Result<String, FormatRulesValidationError> {
    let without_query = strip_query_and_fragment(rest);
    let (authority, path) = match without_query.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (without_query, String::new()),
    };
    if authority.is_empty() {
        return Err(validation_error(FormatRulesValidationErrorKind::InvalidPeerUrl));
    }

    let (username, host_port) = match authority.rsplit_once('@') {
        Some((username, host_port)) => {
            if username.is_empty() || username.contains(':') {
                return Err(validation_error(FormatRulesValidationErrorKind::InvalidPeerUrl));
            }
            (username.to_string(), host_port)
        }
        None => {
            let username = os_username
                .filter(|value| !value.is_empty())
                .ok_or_else(|| validation_error(FormatRulesValidationErrorKind::MissingOsUsername))?;
            (username.to_string(), authority)
        }
    };

    let (host, port) = split_host_port(host_port)?;
    let path = normalize_url_path(&path)?;
    let port_text = match port {
        Some("22") | None => String::new(),
        Some(value) => format!(":{value}"),
    };
    Ok(format!(
        "sftp://{}@{}{}{}",
        username,
        host.to_ascii_lowercase(),
        port_text,
        path
    ))
}

fn split_host_port(authority: &str) -> Result<(&str, Option<&str>), FormatRulesValidationError> {
    if authority.is_empty() {
        return Err(validation_error(FormatRulesValidationErrorKind::InvalidPeerUrl));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !port.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()) => {
            Ok((host, Some(port)))
        }
        Some(_) => Err(validation_error(FormatRulesValidationErrorKind::InvalidPeerUrl)),
        None => Ok((authority, None)),
    }
}

fn strip_query_and_fragment(text: &str) -> &str {
    text.split(['?', '#']).next().unwrap_or(text)
}

fn normalize_url_path(path: &str) -> Result<String, FormatRulesValidationError> {
    let decoded = percent_decode_unreserved(path)?;
    let mut collapsed = String::new();
    let mut previous_slash = false;
    for ch in decoded.replace('\\', "/").chars() {
        if ch == '/' {
            if !previous_slash {
                collapsed.push(ch);
            }
            previous_slash = true;
        } else {
            collapsed.push(ch);
            previous_slash = false;
        }
    }
    while collapsed.len() > 1 && collapsed.ends_with('/') {
        collapsed.pop();
    }
    Ok(collapsed)
}

fn percent_decode_unreserved(text: &str) -> Result<String, FormatRulesValidationError> {
    let bytes = text.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(validation_error(FormatRulesValidationErrorKind::InvalidPeerUrl));
            }
            let high = hex_value(bytes[index + 1])
                .ok_or_else(|| validation_error(FormatRulesValidationErrorKind::InvalidPeerUrl))?;
            let low = hex_value(bytes[index + 2])
                .ok_or_else(|| validation_error(FormatRulesValidationErrorKind::InvalidPeerUrl))?;
            let value = high * 16 + low;
            if is_unreserved(value) {
                output.push(value);
            } else {
                output.extend_from_slice(&bytes[index..index + 3]);
            }
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).map_err(|_| validation_error(FormatRulesValidationErrorKind::InvalidPeerUrl))
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn is_unreserved(value: u8) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, b'-' | b'.' | b'_' | b'~')
}

fn is_absolute_path_text(path: &str) -> bool {
    path.starts_with('/') || looks_like_windows_absolute_path(path)
}

fn looks_like_windows_absolute_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

fn path_to_slash_text(path: &PathBuf) -> Result<String, FormatRulesValidationError> {
    path.to_str()
        .map(|value| value.replace('\\', "/"))
        .ok_or_else(|| validation_error(FormatRulesValidationErrorKind::InvalidPeerUrl))
}

fn validate_relative_path_text(path: &str) -> Result<(), FormatRulesValidationError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains('\\')
        || path.contains('\0')
    {
        return Err(validation_error(FormatRulesValidationErrorKind::InvalidRelativePath));
    }
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(validation_error(FormatRulesValidationErrorKind::InvalidRelativePath));
        }
    }
    Ok(())
}

fn snapshot_id(bytes: &[u8]) -> String {
    let mut hasher = XxHash64::with_seed(0);
    hasher.write(bytes);
    base62_11(hasher.finish())
}

fn base62_11(mut value: u64) -> String {
    let mut digits = [b'0'; 11];
    for digit in digits.iter_mut().rev() {
        *digit = BASE62[(value % 62) as usize];
        value /= 62;
    }
    String::from_utf8(digits.to_vec()).expect("base62 output is ascii")
}

fn parse_timestamp_text(text: &str) -> Result<DateTime<Utc>, FormatRulesValidationError> {
    if text.len() != 27 || !text.ends_with('Z') {
        return Err(validation_error(FormatRulesValidationErrorKind::InvalidTimestamp));
    }
    let body = &text[..text.len() - 1];
    let (seconds, micros) = body
        .rsplit_once('_')
        .ok_or_else(|| validation_error(FormatRulesValidationErrorKind::InvalidTimestamp))?;
    if micros.len() != 6 || !micros.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(validation_error(FormatRulesValidationErrorKind::InvalidTimestamp));
    }
    let micros: i64 = micros
        .parse()
        .map_err(|_| validation_error(FormatRulesValidationErrorKind::InvalidTimestamp))?;
    let naive = NaiveDateTime::parse_from_str(seconds, "%Y-%m-%d_%H-%M-%S")
        .map_err(|_| validation_error(FormatRulesValidationErrorKind::InvalidTimestamp))?
        .checked_add_signed(Duration::microseconds(micros))
        .ok_or_else(|| validation_error(FormatRulesValidationErrorKind::InvalidTimestamp))?;
    Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
}

fn format_datetime(value: DateTime<Utc>) -> String {
    format!(
        "{}_{:06}Z",
        value.format("%Y-%m-%d_%H-%M-%S"),
        value.timestamp_subsec_micros()
    )
}

fn metadata_parent_prefix(
    parent_relative_path: Option<&str>,
) -> Result<String, FormatRulesValidationError> {
    match parent_relative_path {
        Some(parent) => {
            validate_relative_path_text(parent)?;
            Ok(format!("{parent}/"))
        }
        None => Ok(String::new()),
    }
}

fn encode_swap_basename(basename: &str) -> Result<String, FormatRulesValidationError> {
    if basename.is_empty()
        || basename == "."
        || basename == ".."
        || basename.contains('/')
        || basename.contains('\\')
        || basename.contains('\0')
    {
        return Err(validation_error(FormatRulesValidationErrorKind::InvalidSwapBasename));
    }

    let mut encoded = String::new();
    for byte in basename.as_bytes() {
        if is_unreserved(*byte) {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(encoded)
}

fn absolute_difference_micros(
    left: &FormatRulesTimestamp,
    right: &FormatRulesTimestamp,
) -> i64 {
    signed_difference_micros(left, right).abs()
}

fn signed_difference_micros(
    newer: &FormatRulesTimestamp,
    older: &FormatRulesTimestamp,
) -> i64 {
    let newer = parse_timestamp_text(&newer.text).expect("stored timestamp is invalid");
    let older = parse_timestamp_text(&older.text).expect("stored timestamp is invalid");
    newer
        .signed_duration_since(older)
        .num_microseconds()
        .expect("timestamp difference is outside supported range")
}

fn validation_error(kind: FormatRulesValidationErrorKind) -> FormatRulesValidationError {
    FormatRulesValidationError { kind }
}

pub fn new() -> std::sync::Arc<dyn FormatRules> {
    Arc::new(FormatRulesImpl {
        last_generated: Mutex::new(Some(DateTime::<Utc>::from(UNIX_EPOCH))),
    })
}
