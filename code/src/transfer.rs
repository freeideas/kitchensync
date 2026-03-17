use std::path::Path;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::database::{LocalDatabase, PeerDatabase};
use crate::filesystem::FileSystem;
use crate::timestamp;
use crate::reconcile::{Action, decide_action};

/// Execute a push operation (local -> peer).
pub fn push_file(
    sync_root: &Path,
    rel_path: &str,
    local_db: &Arc<Mutex<LocalDatabase>>,
    peer_db: &Arc<Mutex<PeerDatabase>>,
    peer_fs: &dyn FileSystem,
) -> bool {
    // Read local file
    let full_path = sync_root.join(rel_path);
    let data = match std::fs::read(&full_path) {
        Ok(d) => d,
        Err(_) => return false,
    };

    // Get local metadata
    let metadata = match full_path.metadata() {
        Ok(m) => m,
        Err(_) => return false,
    };
    let mod_time = metadata.modified().ok().map(|t| timestamp::from_system_time(t));
    let byte_size = metadata.len() as i64;

    // Create staging path
    let parent = Path::new(rel_path).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let basename = Path::new(rel_path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let ts = timestamp::now();
    let uuid = Uuid::new_v4().to_string()[..8].to_string();

    let xfer_dir = if parent.is_empty() {
        format!(".kitchensync/XFER/{}/{}", ts, uuid)
    } else {
        format!("{}/.kitchensync/XFER/{}/{}", parent, ts, uuid)
    };
    let xfer_path = format!("{}/{}", xfer_dir, basename);

    // Create staging directory
    peer_fs.create_dir_all(&xfer_dir);

    // Transfer file
    if !peer_fs.write_file(&xfer_path, &data) {
        // Cleanup
        peer_fs.delete_file(&xfer_path);
        return false;
    }

    // Recheck peer state
    let peer_stat = peer_fs.stat(rel_path);
    {
        let peer_db_lock = peer_db.lock().unwrap();
        let local_db_lock = local_db.lock().unwrap();
        let local_entry = local_db_lock.get_snapshot_entry(rel_path, false);
        let peer_entry = peer_db_lock.get_snapshot_entry(rel_path, false);

        let action = decide_action(local_entry.as_ref(), peer_entry.as_ref());
        if action != Action::PushFile {
            // Transfer no longer warranted
            peer_fs.delete_file(&xfer_path);
            cleanup_empty_xfer_dirs(peer_fs, &xfer_dir);
            return true;
        }
    }

    // Displace existing file to BACK/
    if peer_stat.is_some() && !peer_stat.as_ref().unwrap().is_dir {
        let back_dir = format!(".kitchensync/BACK/{}", timestamp::now());
        peer_fs.create_dir_all(&back_dir);
        let back_path = format!("{}/{}", back_dir, basename);
        peer_fs.clear_read_only(rel_path);
        peer_fs.rename(rel_path, &back_path);
    }

    // Create parent directories if needed
    if !parent.is_empty() {
        peer_fs.create_dir_all(&parent);
    }

    // Swap - rename from XFER to final location
    if !peer_fs.rename(&xfer_path, rel_path) {
        return false;
    }

    // Set modification time
    if let Some(ref mt) = mod_time {
        peer_fs.set_mtime(rel_path, mt);
    }

    // Cleanup empty XFER directories
    cleanup_empty_xfer_dirs(peer_fs, &xfer_dir);

    // Update peer database
    {
        let mut peer_db_lock = peer_db.lock().unwrap();
        peer_db_lock.upsert_snapshot(rel_path, false, mod_time.as_deref(), byte_size);
    }

    true
}

/// Execute a pull operation (peer -> local).
pub fn pull_file(
    sync_root: &Path,
    rel_path: &str,
    local_db: &Arc<Mutex<LocalDatabase>>,
    peer_db: &Arc<Mutex<PeerDatabase>>,
    peer_fs: &dyn FileSystem,
) -> bool {
    // Read peer file
    let data = match peer_fs.read_file(rel_path) {
        Some(d) => d,
        None => return false,
    };

    // Get peer metadata
    let peer_stat = match peer_fs.stat(rel_path) {
        Some(s) => s,
        None => return false,
    };
    let mod_time = peer_stat.mod_time.clone();
    let byte_size = peer_stat.byte_size;

    // Create local staging path
    let parent = Path::new(rel_path).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let basename = Path::new(rel_path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let ts = timestamp::now();
    let uuid = Uuid::new_v4().to_string()[..8].to_string();

    let parent_full = if parent.is_empty() {
        sync_root.to_path_buf()
    } else {
        sync_root.join(&parent)
    };

    let xfer_dir = parent_full.join(".kitchensync").join("XFER").join(&ts).join(&uuid);
    std::fs::create_dir_all(&xfer_dir).ok();

    let xfer_path = xfer_dir.join(&basename);

    // Write to staging
    if std::fs::write(&xfer_path, &data).is_err() {
        std::fs::remove_file(&xfer_path).ok();
        return false;
    }

    // Recheck local state
    let full_path = sync_root.join(rel_path);
    {
        let local_db_lock = local_db.lock().unwrap();
        let peer_db_lock = peer_db.lock().unwrap();
        let local_entry = local_db_lock.get_snapshot_entry(rel_path, false);
        let peer_entry = peer_db_lock.get_snapshot_entry(rel_path, false);

        let action = decide_action(local_entry.as_ref(), peer_entry.as_ref());
        if action != Action::PullFile {
            std::fs::remove_file(&xfer_path).ok();
            cleanup_local_xfer_dirs(&xfer_dir);
            return true;
        }
    }

    // Displace existing file to BACK/
    if full_path.exists() && full_path.is_file() {
        let back_dir = sync_root.join(".kitchensync").join("BACK").join(timestamp::now());
        std::fs::create_dir_all(&back_dir).ok();
        let back_path = back_dir.join(&basename);

        // Clear read-only if needed
        if let Ok(mut perms) = full_path.metadata().map(|m| m.permissions()) {
            if perms.readonly() {
                perms.set_readonly(false);
                let _ = std::fs::set_permissions(&full_path, perms);
            }
        }

        std::fs::rename(&full_path, &back_path).ok();
    }

    // Ensure parent directory exists
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    // Swap - rename from XFER to final location
    if std::fs::rename(&xfer_path, &full_path).is_err() {
        return false;
    }

    // Set modification time
    if let Some(ref mt) = mod_time {
        if let Some(dt) = crate::timestamp::parse_timestamp(mt) {
            let secs = dt.timestamp();
            let system_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs as u64);
            let _ = filetime::set_file_mtime(&full_path, filetime::FileTime::from_system_time(system_time));
        }
    }

    // Cleanup empty XFER directories
    cleanup_local_xfer_dirs(&xfer_dir);

    // Update local database
    {
        let mut local_db_lock = local_db.lock().unwrap();
        local_db_lock.upsert_snapshot(rel_path, false, mod_time.as_deref(), byte_size);
    }

    true
}

/// Push a deletion to peer.
pub fn push_delete(
    rel_path: &str,
    peer_db: &Arc<Mutex<PeerDatabase>>,
    peer_fs: &dyn FileSystem,
) -> bool {
    let peer_stat = peer_fs.stat(rel_path);

    if let Some(stat) = peer_stat {
        if stat.is_dir {
            // Directory - check if empty
            if !peer_fs.is_dir_empty(rel_path) {
                return false; // Skip non-empty directories
            }
            peer_fs.delete_dir(rel_path);
        } else {
            // File - displace to BACK/
            let basename = Path::new(rel_path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let back_dir = format!(".kitchensync/BACK/{}", timestamp::now());
            peer_fs.create_dir_all(&back_dir);
            let back_path = format!("{}/{}", back_dir, basename);
            peer_fs.clear_read_only(rel_path);
            peer_fs.rename(rel_path, &back_path);
        }
    }

    // Update peer database
    {
        let mut peer_db_lock = peer_db.lock().unwrap();
        peer_db_lock.set_del_time(rel_path, false, &timestamp::now());
    }

    true
}

/// Pull a deletion from peer.
pub fn pull_delete(
    sync_root: &Path,
    rel_path: &str,
    local_db: &Arc<Mutex<LocalDatabase>>,
) -> bool {
    let full_path = sync_root.join(rel_path);

    if full_path.exists() {
        if full_path.is_dir() {
            // Check if empty
            if std::fs::read_dir(&full_path).map(|mut d| d.next().is_some()).unwrap_or(false) {
                return false; // Skip non-empty directories
            }
            std::fs::remove_dir(&full_path).ok();
        } else {
            // File - displace to BACK/
            let basename = full_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let back_dir = sync_root.join(".kitchensync").join("BACK").join(timestamp::now());
            std::fs::create_dir_all(&back_dir).ok();
            let back_path = back_dir.join(&basename);

            // Clear read-only if needed
            #[cfg(windows)]
            {
                if let Ok(mut perms) = full_path.metadata().map(|m| m.permissions()) {
                    if perms.readonly() {
                        perms.set_readonly(false);
                        let _ = std::fs::set_permissions(&full_path, perms);
                    }
                }
            }

            std::fs::rename(&full_path, &back_path).ok();
        }
    }

    // Update local database
    {
        let mut local_db_lock = local_db.lock().unwrap();
        local_db_lock.set_del_time(rel_path, false, &timestamp::now());
    }

    true
}

/// Create a directory on peer.
pub fn create_dir_on_peer(
    rel_path: &str,
    peer_db: &Arc<Mutex<PeerDatabase>>,
    peer_fs: &dyn FileSystem,
) -> bool {
    peer_fs.create_dir_all(rel_path);

    // Update peer database
    {
        let mut peer_db_lock = peer_db.lock().unwrap();
        peer_db_lock.upsert_snapshot(rel_path, true, None, -1);
    }

    true
}

/// Create a directory locally.
pub fn create_dir_locally(
    sync_root: &Path,
    rel_path: &str,
    local_db: &Arc<Mutex<LocalDatabase>>,
) -> bool {
    let full_path = sync_root.join(rel_path);
    std::fs::create_dir_all(&full_path).ok();

    // Update local database
    {
        let mut local_db_lock = local_db.lock().unwrap();
        local_db_lock.upsert_snapshot(rel_path, true, None, -1);
    }

    true
}

fn cleanup_empty_xfer_dirs(peer_fs: &dyn FileSystem, xfer_dir: &str) {
    // Try to delete uuid dir
    peer_fs.delete_dir(xfer_dir);

    // Try to delete timestamp dir
    if let Some(pos) = xfer_dir.rfind('/') {
        let parent = &xfer_dir[..pos];
        peer_fs.delete_dir(parent);

        // Try to delete XFER dir if empty
        if let Some(pos2) = parent.rfind('/') {
            let xfer_parent = &parent[..pos2];
            peer_fs.delete_dir(xfer_parent);
        }
    }
}

fn cleanup_local_xfer_dirs(xfer_dir: &Path) {
    // Try to delete uuid dir
    std::fs::remove_dir(xfer_dir).ok();

    // Try to delete timestamp dir
    if let Some(parent) = xfer_dir.parent() {
        std::fs::remove_dir(parent).ok();

        // Try to delete XFER dir if empty
        if let Some(xfer_parent) = parent.parent() {
            std::fs::remove_dir(xfer_parent).ok();
        }
    }
}
