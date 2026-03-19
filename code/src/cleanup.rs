use crate::database;
use crate::peer::Peer;
use crate::timestamp;
use rusqlite::Connection;

pub fn purge_all(
    conn: &Connection,
    peers: &std::collections::HashMap<String, Box<dyn Peer>>,
    xfer_cleanup_days: u64,
    back_retention_days: u64,
    tombstone_retention_days: u64,
    log_retention_days: u64,
) {
    database::purge_tombstones(conn, tombstone_retention_days);
    // Log purge happens on every log insert, but do an explicit one here
    if let Some(cutoff) = cutoff_timestamp(log_retention_days) {
        conn.execute("DELETE FROM applog WHERE stamp < ?1", [&cutoff])
            .ok();
    }

    // Clean stale XFER and BACK directories on each peer
    for peer in peers.values() {
        clean_kitchensync_dirs(peer.as_ref(), "XFER", xfer_cleanup_days);
        clean_kitchensync_dirs(peer.as_ref(), "BACK", back_retention_days);
    }
}

fn clean_kitchensync_dirs(peer: &dyn Peer, subdir: &str, retention_days: u64) {
    let cutoff = match cutoff_timestamp(retention_days) {
        Some(c) => c,
        None => return,
    };

    // We need to traverse the peer's tree looking for .kitchensync/<subdir>/ directories
    // and remove stale timestamp dirs within them.
    // This is a best-effort cleanup; we walk the root and look for .kitchensync dirs.
    clean_dir_recursive(peer, ".", subdir, &cutoff);
}

fn clean_dir_recursive(peer: &dyn Peer, path: &str, subdir: &str, cutoff: &str) {
    let ks_path = if path == "." {
        format!(".kitchensync/{}", subdir)
    } else {
        format!("{}/.kitchensync/{}", path, subdir)
    };

    if let Ok(entries) = peer.list_dir(&ks_path) {
        for entry in &entries {
            if entry.is_dir && entry.name.len() >= 15 {
                // Timestamp dirs start with YYYYMMDD
                if *entry.name < *cutoff {
                    let stamp_dir = format!("{}/{}", ks_path, entry.name);
                    remove_tree(peer, &stamp_dir);
                }
            }
        }
    }

    // Recurse into subdirectories
    if let Ok(entries) = peer.list_dir(path) {
        for entry in &entries {
            if entry.is_dir && entry.name != ".kitchensync" {
                let child = if path == "." {
                    entry.name.clone()
                } else {
                    format!("{}/{}", path, entry.name)
                };
                clean_dir_recursive(peer, &child, subdir, cutoff);
            }
        }
    }
}

fn remove_tree(peer: &dyn Peer, path: &str) {
    if let Ok(entries) = peer.list_dir(path) {
        for entry in &entries {
            let child = format!("{}/{}", path, entry.name);
            if entry.is_dir {
                remove_tree(peer, &child);
            } else {
                peer.delete_file(&child).ok();
            }
        }
    }
    peer.delete_dir(path).ok();
}

fn cutoff_timestamp(days: u64) -> Option<String> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
    Some(timestamp::format_timestamp(cutoff))
}
