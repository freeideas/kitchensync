use std::path::Path;
use std::fs;

use crate::config::PeerConfig;
use crate::filesystem::FileSystem;
use crate::timestamp;

/// Clean up peer databases for peers not listed in config.
pub fn cleanup_unlisted_peers(peer_dir: &Path, peers: &[PeerConfig]) {
    if !peer_dir.exists() {
        return;
    }

    let peer_names: std::collections::HashSet<_> = peers.iter().map(|p| format!("{}.db", p.name)).collect();

    if let Ok(entries) = fs::read_dir(peer_dir) {
        for entry in entries.flatten() {
            let filename = entry.file_name().to_string_lossy().to_string();
            if filename.ends_with(".db") && !peer_names.contains(&filename) {
                fs::remove_file(entry.path()).ok();
            }
        }
    }
}

/// Clean up stale XFER directories on a peer.
pub fn cleanup_peer_xfer(peer_fs: &dyn FileSystem, xfer_cleanup_days: u32) {
    cleanup_xfer_recursive(peer_fs, "", xfer_cleanup_days);
}

fn cleanup_xfer_recursive(peer_fs: &dyn FileSystem, path: &str, xfer_cleanup_days: u32) {
    let entries = peer_fs.list_dir(path);

    for entry in entries {
        if entry.is_symlink {
            continue;
        }

        let full_path = if path.is_empty() {
            entry.name.clone()
        } else {
            format!("{}/{}", path, entry.name)
        };

        if entry.is_dir {
            // Check if this is a .kitchensync directory
            if entry.name == ".kitchensync" {
                // Look for XFER subdirectory
                let xfer_path = format!("{}/XFER", full_path);
                cleanup_xfer_dir(peer_fs, &xfer_path, xfer_cleanup_days);
            } else {
                // Recurse into regular directories
                cleanup_xfer_recursive(peer_fs, &full_path, xfer_cleanup_days);
            }
        }
    }
}

fn cleanup_xfer_dir(peer_fs: &dyn FileSystem, xfer_path: &str, xfer_cleanup_days: u32) {
    let entries = peer_fs.list_dir(xfer_path);

    let cutoff = chrono::Utc::now() - chrono::Duration::days(xfer_cleanup_days as i64);

    for entry in entries {
        if !entry.is_dir {
            continue;
        }

        // Entry name should be a timestamp
        if let Some(ts) = timestamp::parse_timestamp(&entry.name) {
            if ts < cutoff {
                // Delete this directory recursively
                let ts_path = format!("{}/{}", xfer_path, entry.name);
                delete_dir_recursive(peer_fs, &ts_path);
            }
        }
    }

    // Try to delete empty XFER directory
    if peer_fs.is_dir_empty(xfer_path) {
        peer_fs.delete_dir(xfer_path);
    }
}

fn delete_dir_recursive(peer_fs: &dyn FileSystem, path: &str) {
    let entries = peer_fs.list_dir(path);

    for entry in entries {
        let full_path = format!("{}/{}", path, entry.name);
        if entry.is_dir {
            delete_dir_recursive(peer_fs, &full_path);
        } else {
            peer_fs.delete_file(&full_path);
        }
    }

    peer_fs.delete_dir(path);
}

/// Clean up local XFER directories.
pub fn cleanup_local_xfer(sync_root: &Path, xfer_cleanup_days: u32) {
    cleanup_local_xfer_recursive(sync_root, sync_root, xfer_cleanup_days);
}

fn cleanup_local_xfer_recursive(root: &Path, current: &Path, xfer_cleanup_days: u32) {
    if let Ok(entries) = fs::read_dir(current) {
        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_symlink() {
                continue;
            }

            if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();

                if name == ".kitchensync" {
                    let xfer_path = path.join("XFER");
                    if xfer_path.exists() {
                        cleanup_local_xfer_timestamps(&xfer_path, xfer_cleanup_days);
                    }
                } else {
                    cleanup_local_xfer_recursive(root, &path, xfer_cleanup_days);
                }
            }
        }
    }
}

fn cleanup_local_xfer_timestamps(xfer_path: &Path, xfer_cleanup_days: u32) {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(xfer_cleanup_days as i64);

    if let Ok(entries) = fs::read_dir(xfer_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();

            if let Some(ts) = timestamp::parse_timestamp(&name) {
                if ts < cutoff {
                    fs::remove_dir_all(&path).ok();
                }
            }
        }
    }

    // Try to delete empty XFER directory
    if fs::read_dir(xfer_path).map(|mut d| d.next().is_none()).unwrap_or(true) {
        fs::remove_dir(xfer_path).ok();
    }
}

/// Clean up old BACK directories.
pub fn cleanup_back_dirs(sync_root: &Path, back_retention_days: u32) {
    let back_path = sync_root.join(".kitchensync").join("BACK");

    if !back_path.exists() {
        return;
    }

    let cutoff = chrono::Utc::now() - chrono::Duration::days(back_retention_days as i64);

    if let Ok(entries) = fs::read_dir(&back_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();

            if let Some(ts) = timestamp::parse_timestamp(&name) {
                if ts < cutoff {
                    fs::remove_dir_all(&path).ok();
                }
            }
        }
    }
}

/// Clean up old BACK directories on a peer.
pub fn cleanup_peer_back_dirs(peer_fs: &dyn FileSystem, back_retention_days: u32) {
    let back_path = ".kitchensync/BACK";
    let entries = peer_fs.list_dir(back_path);

    let cutoff = chrono::Utc::now() - chrono::Duration::days(back_retention_days as i64);

    for entry in entries {
        if !entry.is_dir {
            continue;
        }

        if let Some(ts) = timestamp::parse_timestamp(&entry.name) {
            if ts < cutoff {
                let full_path = format!("{}/{}", back_path, entry.name);
                delete_dir_recursive(peer_fs, &full_path);
            }
        }
    }
}
