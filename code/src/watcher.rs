use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::process::Command;

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher, Event, EventKind};
use crossbeam_channel::{bounded, Receiver};

use crate::database::{LocalDatabase, PeerDatabase};
use crate::ignore::IgnoreMatcher;
use crate::timestamp;

/// Run the filesystem watcher.
pub fn run_watcher(
    sync_root: PathBuf,
    local_db: Arc<Mutex<LocalDatabase>>,
    peer_dbs: Vec<Arc<Mutex<PeerDatabase>>>,
    shutdown_flag: Arc<AtomicBool>,
    ignore_matcher: Arc<IgnoreMatcher>,
) {
    let (tx, rx) = bounded(1000);

    let mut watcher = match RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        },
        Config::default(),
    ) {
        Ok(w) => w,
        Err(_) => return,
    };

    if watcher.watch(&sync_root, RecursiveMode::Recursive).is_err() {
        return;
    }

    while !shutdown_flag.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => {
                process_event(
                    &event,
                    &sync_root,
                    &local_db,
                    &peer_dbs,
                    &ignore_matcher,
                );
            }
            Err(_) => continue,
        }
    }
}

fn process_event(
    event: &Event,
    sync_root: &Path,
    local_db: &Arc<Mutex<LocalDatabase>>,
    peer_dbs: &[Arc<Mutex<PeerDatabase>>],
    ignore_matcher: &Arc<IgnoreMatcher>,
) {
    for path in &event.paths {
        // Get relative path
        let rel_path = match path.strip_prefix(sync_root) {
            Ok(p) => p.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };

        // Skip if ignored
        if ignore_matcher.is_ignored(path, path.is_dir()) {
            continue;
        }

        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) => {
                // File created or modified
                if path.exists() {
                    let is_dir = path.is_dir();
                    let metadata = match path.metadata() {
                        Ok(m) => m,
                        Err(_) => continue,
                    };

                    let byte_size = if is_dir { -1 } else { metadata.len() as i64 };
                    let mod_time = if is_dir {
                        None
                    } else {
                        metadata.modified().ok().map(|t| timestamp::from_system_time(t))
                    };

                    // Update local database
                    {
                        let mut db = local_db.lock().unwrap();
                        db.upsert_snapshot(&rel_path, is_dir, mod_time.as_deref(), byte_size);
                    }

                    // Enqueue to all peers
                    for peer_db in peer_dbs {
                        let mut db = peer_db.lock().unwrap();
                        db.enqueue(&rel_path);
                    }
                }
            }
            EventKind::Remove(_) => {
                // File removed
                let del_time = timestamp::now();

                // Check if it was a file or directory
                let entry = {
                    let db = local_db.lock().unwrap();
                    db.get_snapshot_entry(&rel_path, false)
                        .or_else(|| db.get_snapshot_entry(&rel_path, true))
                };

                if let Some(e) = entry {
                    let is_dir = e.is_dir();

                    // Set del_time
                    {
                        let mut db = local_db.lock().unwrap();
                        db.set_del_time(&rel_path, is_dir, &del_time);
                    }

                    // Enqueue to all peers
                    for peer_db in peer_dbs {
                        let mut db = peer_db.lock().unwrap();
                        db.enqueue(&rel_path);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Watch peers.conf for changes and restart on modification.
pub fn watch_config_file(
    peers_conf: PathBuf,
    shutdown_flag: Arc<AtomicBool>,
    original_args: Vec<String>,
) {
    let (tx, rx) = bounded(100);

    let mut watcher = match RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        },
        Config::default(),
    ) {
        Ok(w) => w,
        Err(_) => return,
    };

    if let Some(parent) = peers_conf.parent() {
        if watcher.watch(parent, RecursiveMode::NonRecursive).is_err() {
            return;
        }
    }

    let mut last_event_time: Option<Instant> = None;

    while !shutdown_flag.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => {
                // Check if this event is for peers.conf
                let is_peers_conf = event.paths.iter().any(|p| {
                    p.file_name().map(|n| n == "peers.conf").unwrap_or(false)
                });

                if is_peers_conf {
                    last_event_time = Some(Instant::now());
                }
            }
            Err(_) => {
                // Check if we should trigger restart (500ms debounce)
                if let Some(last_time) = last_event_time {
                    if last_time.elapsed() >= Duration::from_millis(500) {
                        // Trigger restart
                        shutdown_flag.store(true, Ordering::SeqCst);

                        // Re-execute with original arguments
                        if let Some(exe) = original_args.first() {
                            let args: Vec<&str> = original_args.iter().skip(1).map(|s| s.as_str()).collect();
                            let _ = Command::new(exe).args(&args).spawn();
                        }

                        return;
                    }
                }
            }
        }
    }
}
