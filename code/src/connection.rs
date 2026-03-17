use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use crossbeam_channel::{bounded, Receiver, Sender};

use crate::config::{GlobalConfig, PeerConfig};
use crate::database::{LocalDatabase, PeerDatabase};
use crate::filesystem::{connect_to_peer, FileSystem};
use crate::ignore::IgnoreMatcher;
use crate::reconcile::{decide_action, Action};
use crate::transfer;
use crate::walker;
use crate::cleanup;

/// Run the connection manager for a peer.
pub fn run_connection_manager(
    sync_root: std::path::PathBuf,
    peer_config: PeerConfig,
    global_config: GlobalConfig,
    local_db: Arc<Mutex<LocalDatabase>>,
    peer_db: Arc<Mutex<PeerDatabase>>,
    all_peer_dbs: Vec<Arc<Mutex<PeerDatabase>>>,
    shutdown_flag: Arc<AtomicBool>,
    ignore_matcher: Arc<IgnoreMatcher>,
    once_mode: bool,
) {
    // Set queue max size
    {
        let mut db = peer_db.lock().unwrap();
        db.queue_max_size = global_config.queue_max_size;
    }

    let mut last_walk_time = {
        let db = peer_db.lock().unwrap();
        db.get_config("last_walk_time")
    };

    loop {
        if shutdown_flag.load(Ordering::SeqCst) {
            break;
        }

        let queue_empty = {
            let db = peer_db.lock().unwrap();
            db.queue_is_empty()
        };

        let time_to_rewalk = should_rewalk(&last_walk_time, peer_config.rewalk_after_minutes);

        if !queue_empty || time_to_rewalk {
            // Try to connect
            let peer_fs = try_connect(&peer_config.urls, global_config.connection_timeout, global_config.retry_interval, &shutdown_flag);

            if let Some(fs) = peer_fs {
                // Run peer walker
                walker::run_peer_walker(
                    fs.as_ref(),
                    &peer_db,
                    &all_peer_dbs,
                    &ignore_matcher,
                    once_mode,
                    &sync_root,
                );

                // Clean up stale XFER directories on peer
                cleanup::cleanup_peer_xfer(fs.as_ref(), global_config.xfer_cleanup_days);

                // Update last walk time
                last_walk_time = {
                    let db = peer_db.lock().unwrap();
                    db.get_config("last_walk_time")
                };

                // Spawn workers to drain queue
                drain_queue(
                    &sync_root,
                    fs.as_ref(),
                    &local_db,
                    &peer_db,
                    global_config.workers_per_peer,
                    &shutdown_flag,
                );

                // In once mode, exit after one cycle
                if once_mode {
                    break;
                }
            } else if once_mode {
                // In once mode, skip unreachable peers
                break;
            }
        } else {
            // Nothing to do - sleep briefly
            thread::sleep(Duration::from_secs(1));
        }

        if once_mode {
            break;
        }
    }
}

fn should_rewalk(last_walk_time: &Option<String>, rewalk_after_minutes: u32) -> bool {
    match last_walk_time {
        None => true, // Never walked
        Some(ts) => {
            if let Some(dt) = crate::timestamp::parse_timestamp(ts) {
                let elapsed = chrono::Utc::now() - dt;
                elapsed.num_minutes() >= rewalk_after_minutes as i64
            } else {
                true
            }
        }
    }
}

fn try_connect(
    urls: &[String],
    timeout_secs: u32,
    retry_interval: u32,
    shutdown_flag: &Arc<AtomicBool>,
) -> Option<Box<dyn FileSystem>> {
    for url in urls {
        if shutdown_flag.load(Ordering::SeqCst) {
            return None;
        }

        match connect_to_peer(url, timeout_secs) {
            Ok(fs) => return Some(fs),
            Err(_) => continue,
        }
    }

    // All URLs failed - wait and return None
    let sleep_duration = Duration::from_secs(retry_interval as u64);
    let start = Instant::now();
    while start.elapsed() < sleep_duration {
        if shutdown_flag.load(Ordering::SeqCst) {
            return None;
        }
        thread::sleep(Duration::from_millis(100));
    }

    None
}

fn drain_queue(
    sync_root: &Path,
    peer_fs: &dyn FileSystem,
    local_db: &Arc<Mutex<LocalDatabase>>,
    peer_db: &Arc<Mutex<PeerDatabase>>,
    workers: usize,
    shutdown_flag: &Arc<AtomicBool>,
) {
    // Create a channel for work items
    let (tx, rx): (Sender<String>, Receiver<String>) = bounded(workers * 2);

    // Spawn worker threads
    let mut handles = Vec::new();
    for _ in 0..workers {
        let sync_root = sync_root.to_path_buf();
        let local_db = local_db.clone();
        let peer_db = peer_db.clone();
        let rx = rx.clone();
        let shutdown = shutdown_flag.clone();

        // We need to share peer_fs across threads, but it's not Clone
        // For simplicity, we'll process sequentially in this implementation
        // A production version would use a connection pool or channels
        handles.push(thread::spawn(move || {
            while let Ok(path) = rx.recv() {
                if shutdown.load(Ordering::SeqCst) {
                    break;
                }
                // Worker would process here, but we need peer_fs
                // For now, this is a placeholder
            }
        }));
    }
    drop(rx); // Close receiver clone

    // Process queue items
    loop {
        if shutdown_flag.load(Ordering::SeqCst) {
            break;
        }

        let path = {
            let mut db = peer_db.lock().unwrap();
            db.dequeue()
        };

        match path {
            Some(p) => {
                process_queue_item(sync_root, peer_fs, local_db, peer_db, &p);
            }
            None => break,
        }
    }

    // Close sender to signal workers
    drop(tx);

    // Wait for workers
    for handle in handles {
        handle.join().ok();
    }
}

fn process_queue_item(
    sync_root: &Path,
    peer_fs: &dyn FileSystem,
    local_db: &Arc<Mutex<LocalDatabase>>,
    peer_db: &Arc<Mutex<PeerDatabase>>,
    path: &str,
) {
    // Determine if path is a directory
    let is_dir = {
        let local_entry = local_db.lock().unwrap().get_snapshot_entry(path, true);
        let peer_entry = peer_db.lock().unwrap().get_snapshot_entry(path, true);
        local_entry.map(|e| e.is_dir()).unwrap_or(false)
            || peer_entry.map(|e| e.is_dir()).unwrap_or(false)
    };

    // Get entries for decision
    let local_entry = local_db.lock().unwrap().get_snapshot_entry(path, is_dir);
    let peer_entry = peer_db.lock().unwrap().get_snapshot_entry(path, is_dir);

    let action = decide_action(local_entry.as_ref(), peer_entry.as_ref());

    match action {
        Action::NoAction => {}
        Action::PushFile => {
            transfer::push_file(sync_root, path, local_db, peer_db, peer_fs);
        }
        Action::PullFile => {
            transfer::pull_file(sync_root, path, local_db, peer_db, peer_fs);
        }
        Action::PushDelete => {
            transfer::push_delete(path, peer_db, peer_fs);
        }
        Action::PullDelete => {
            transfer::pull_delete(sync_root, path, local_db);
        }
        Action::CreateDirOnPeer => {
            transfer::create_dir_on_peer(path, peer_db, peer_fs);
        }
        Action::CreateDirLocally => {
            transfer::create_dir_locally(sync_root, path, local_db);
        }
        Action::DeleteDirOnPeer => {
            if peer_fs.is_dir_empty(path) {
                peer_fs.delete_dir(path);
                let mut db = peer_db.lock().unwrap();
                db.set_del_time(path, true, &crate::timestamp::now());
            }
        }
    }
}
