use crate::transport::Transport;
use chrono::Utc;
use std::io;

/// Format a timestamp as YYYY-MM-DD_HH-mm-ss_ffffffZ (REQ_SYNCOP_016, REQ_SYNCOP_018).
pub fn format_timestamp() -> String {
    Utc::now().format("%Y-%m-%d_%H-%M-%S_%6fZ").to_string()
}

/// Parse a YYYY-MM-DD_HH-mm-ss_ffffffZ timestamp string to a Unix timestamp.
pub fn parse_timestamp(s: &str) -> Option<i64> {
    chrono::NaiveDateTime::parse_from_str(s.trim_end_matches('Z'), "%Y-%m-%d_%H-%M-%S_%6f")
        .ok()
        .map(|dt| dt.and_utc().timestamp())
}

/// Write a file atomically via staging: write to TMP, then rename to final location.
pub fn write_via_staging(
    transport: &dyn Transport,
    rel_path: &str,
    data: &[u8],
    mod_time: i64,
) -> io::Result<()> {
    let (parent, basename) = split_path(rel_path);
    let timestamp = format_timestamp();
    let uuid = uuid::Uuid::new_v4().to_string();

    let ks_tmp_base = if parent.is_empty() {
        ".kitchensync/TMP".to_string()
    } else {
        format!("{}/.kitchensync/TMP", parent)
    };

    let ts_dir = format!("{}/{}", ks_tmp_base, timestamp);
    let tmp_dir = format!("{}/{}", ts_dir, uuid);
    let tmp_path = format!("{}/{}", tmp_dir, basename);

    transport.mkdir(&tmp_dir)?;
    transport.write_file(&tmp_path, data)?;
    transport.set_mod_time(&tmp_path, mod_time)?;

    if !parent.is_empty() {
        transport.mkdir(&parent)?;
    }

    transport.rename(&tmp_path, rel_path)?;
    // Clean up empty TMP directories (REQ_SYNCOP_012)
    let _ = transport.remove_dir(&tmp_dir);
    let _ = transport.remove_dir(&ts_dir);
    let _ = transport.remove_dir(&ks_tmp_base);

    Ok(())
}

/// Clean up stale staging directories older than the given number of days.
/// REQ_SYNCOP_019, REQ_SYNCOP_024: Parse timestamp from directory name to determine age.
pub fn purge_old_staging(
    transport: &dyn Transport,
    rel_path: &str,
    max_age_days: u64,
) -> io::Result<()> {
    let tmp_dir = if rel_path.is_empty() {
        ".kitchensync/TMP".to_string()
    } else {
        format!("{}/.kitchensync/TMP", rel_path)
    };

    let entries = match transport.list_dir(&tmp_dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    let cutoff = Utc::now().timestamp() - (max_age_days as i64 * 86400);

    for entry in entries {
        if entry.is_dir {
            if let Some(ts) = parse_timestamp(&entry.name) {
                if ts < cutoff {
                    let path = format!("{}/{}", tmp_dir, entry.name);
                    remove_recursive(transport, &path);
                }
            }
        }
    }

    Ok(())
}

fn remove_recursive(transport: &dyn Transport, path: &str) {
    if let Ok(children) = transport.list_dir(path) {
        for child in children {
            let child_path = format!("{}/{}", path, child.name);
            if child.is_dir {
                remove_recursive(transport, &child_path);
            } else {
                let _ = transport.delete_file(&child_path);
            }
        }
    }
    let _ = transport.remove_dir(path);
}

fn split_path(rel_path: &str) -> (String, String) {
    match rel_path.rfind('/') {
        Some(i) => (rel_path[..i].to_string(), rel_path[i + 1..].to_string()),
        None => (String::new(), rel_path.to_string()),
    }
}
