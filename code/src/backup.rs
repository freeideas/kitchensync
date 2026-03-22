use crate::staging;
use crate::transport::Transport;
use std::io;

/// Move a file or directory to .kitchensync/BAK/<timestamp>/<basename> in its parent directory.
/// REQ_SYNCOP_015: A displaced directory is moved as a single rename, preserving its entire subtree.
pub fn backup_file(transport: &dyn Transport, rel_path: &str) -> io::Result<()> {
    let (parent, basename) = split_path(rel_path);
    let timestamp = staging::format_timestamp();

    let bak_path = if parent.is_empty() {
        format!(".kitchensync/BAK/{}/{}", timestamp, basename)
    } else {
        format!("{}/.kitchensync/BAK/{}/{}", parent, timestamp, basename)
    };

    transport.mkdir(&dir_of(&bak_path))?;
    transport.rename(rel_path, &bak_path)
}

/// Clean up backup files older than the given number of days.
/// REQ_SYNCOP_017, REQ_SYNCOP_024: Parse timestamp from directory name to determine age.
pub fn purge_old_backups(
    transport: &dyn Transport,
    rel_path: &str,
    max_age_days: u64,
) -> io::Result<()> {
    let bak_dir = if rel_path.is_empty() {
        ".kitchensync/BAK".to_string()
    } else {
        format!("{}/.kitchensync/BAK", rel_path)
    };

    let entries = match transport.list_dir(&bak_dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    let cutoff = chrono::Utc::now().timestamp() - (max_age_days as i64 * 86400);

    for entry in entries {
        if entry.is_dir {
            if let Some(ts) = staging::parse_timestamp(&entry.name) {
                if ts < cutoff {
                    let path = format!("{}/{}", bak_dir, entry.name);
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

fn dir_of(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}
