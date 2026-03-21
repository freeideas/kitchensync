use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::connection_pool::ConnectedPeer;
use crate::database::Database;
use crate::filesystem::{EntryMeta, FsError, PeerFs};
use crate::hash;
use crate::timestamp;

/// A copy operation to be executed concurrently.
struct CopyOp {
    src_peer_idx: usize,
    dst_peer_idx: usize,
    rel_path: String,
    entry_id: String,
}

/// Deferred directory mod_time setting (applied after all copies complete).
struct DeferredDirMtime {
    peer_idx: usize,
    rel_path: String,
    mod_time: i64,
}

/// Run the combined-tree sync algorithm.
pub async fn run_sync(
    peers: &[Arc<ConnectedPeer>],
    db: &Database,
    config: &crate::config::Config,
    sync_timestamp: &str,
) -> Result<(), String> {
    let (tx, mut rx) = mpsc::channel::<CopyOp>(256);

    // Spawn copy worker
    let peers_clone: Vec<Arc<ConnectedPeer>> = peers.iter().cloned().collect();
    let sync_ts = sync_timestamp.to_string();
    let db_path = db.conn.path().unwrap_or("").to_string();
    let log_level = config.log_level().to_string();

    let copy_handle = tokio::spawn(async move {
        let copy_db = Database::open(std::path::Path::new(&db_path))
            .expect("cannot reopen db for copy worker");
        while let Some(op) = rx.recv().await {
            let src = &peers_clone[op.src_peer_idx];
            let dst = &peers_clone[op.dst_peer_idx];

            // Acquire semaphores from both pools
            let _src_permit = src.pool.semaphore.acquire().await;
            let _dst_permit = dst.pool.semaphore.acquire().await;

            if log_level == "trace" {
                let src_avail = src.pool.semaphore.available_permits();
                let dst_avail = dst.pool.semaphore.available_permits();
                copy_db.log("trace",
                    &format!("url={} connections={}/{}",
                        src.active_url, src.pool.max_connections as usize - src_avail, src.pool.max_connections),
                    &log_level);
                copy_db.log("trace",
                    &format!("url={} connections={}/{}",
                        dst.active_url, dst.pool.max_connections as usize - dst_avail, dst.pool.max_connections),
                    &log_level);
            }

            match execute_copy(src, dst, &op.rel_path, &sync_ts).await {
                Ok(()) => {
                    // Update last_seen on destination
                    let _ = copy_db.update_last_seen(&op.entry_id, dst.peer_id, &sync_ts);
                }
                Err(e) => {
                    copy_db.log("error", &format!("copy failed {}: {}", op.rel_path, e), &log_level);
                }
            }
        }
    });

    // Run the recursive combined-tree walk
    let peer_indices: Vec<usize> = (0..peers.len()).collect();
    let mut deferred_mtimes: Vec<DeferredDirMtime> = Vec::new();
    sync_directory(peers, &peer_indices, "", db, config, sync_timestamp, &tx, &mut deferred_mtimes).await?;

    // Close sender to signal copy worker to finish
    drop(tx);
    copy_handle.await.map_err(|e| format!("copy worker error: {}", e))?;

    // Apply deferred directory mod_times in reverse order (deepest first)
    // so that setting a child dir's mtime doesn't alter the parent's mtime.
    for dm in deferred_mtimes.iter().rev() {
        let _ = peers[dm.peer_idx].fs.set_mod_time(&dm.rel_path, dm.mod_time).await;
    }

    Ok(())
}

/// Execute a single file copy using pipelined transfer (two tasks + bounded channel).
async fn execute_copy(
    src: &ConnectedPeer,
    dst: &ConnectedPeer,
    rel_path: &str,
    sync_ts: &str,
) -> Result<(), String> {
    let xfer_uuid = uuid::Uuid::new_v4().to_string();
    let basename = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let parent = if let Some(pos) = rel_path.rfind('/') {
        &rel_path[..pos]
    } else {
        ""
    };

    let xfer_dir = format!(
        "{}/.kitchensync/XFER/{}/{}/",
        if parent.is_empty() { "" } else { parent },
        sync_ts,
        xfer_uuid
    );
    let xfer_path = format!("{}{}", xfer_dir.trim_start_matches('/'), basename);
    let xfer_path = if xfer_path.starts_with('/') {
        xfer_path[1..].to_string()
    } else {
        xfer_path
    };

    // Pipelined transfer: reader task → bounded channel → writer task
    let (data_tx, mut data_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(8);

    let src_fs = src.fs.clone();
    let src_path = rel_path.to_string();
    let reader_handle = tokio::spawn(async move {
        match src_fs.read_file(&src_path).await {
            Ok(mut reader) => {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match tokio::io::AsyncReadExt::read(&mut reader, &mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            if data_tx.send(buf[..n].to_vec()).await.is_err() {
                                return Err("writer dropped".to_string());
                            }
                        }
                        Err(e) => return Err(format!("read error: {}", e)),
                    }
                }
                Ok(())
            }
            Err(e) => Err(format!("open error: {}", e)),
        }
    });

    let dst_fs = dst.fs.clone();
    let xfer_path_clone = xfer_path.clone();
    let writer_handle = tokio::spawn(async move {
        // Create a pipe: collect from channel and write
        let (mut pipe_tx, pipe_rx) = tokio::io::duplex(64 * 1024);

        let write_handle = tokio::spawn({
            let dst_fs = dst_fs.clone();
            let xfer_path = xfer_path_clone.clone();
            async move {
                dst_fs
                    .write_file(&xfer_path, Box::pin(pipe_rx))
                    .await
                    .map_err(|e| format!("write error: {}", e))
            }
        });

        // Feed data from channel to pipe
        while let Some(chunk) = data_rx.recv().await {
            if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut pipe_tx, &chunk).await {
                return Err(format!("pipe write error: {}", e));
            }
        }
        drop(pipe_tx); // Signal EOF

        write_handle
            .await
            .map_err(|e| format!("write task error: {}", e))?
    });

    let (reader_result, writer_result) = tokio::join!(reader_handle, writer_handle);

    let reader_ok = reader_result
        .map_err(|e| format!("reader task panic: {}", e))?;
    let writer_ok = writer_result
        .map_err(|e| format!("writer task panic: {}", e))?;

    if let Err(e) = reader_ok {
        // Clean up XFER staging
        let _ = dst.fs.delete_file(&xfer_path).await;
        return Err(e);
    }
    if let Err(e) = writer_ok {
        let _ = dst.fs.delete_file(&xfer_path).await;
        return Err(e);
    }

    // If destination already has a file at target, displace to BACK
    let back_dir = format!(
        "{}/.kitchensync/BACK/{}/",
        if parent.is_empty() { "" } else { parent },
        sync_ts
    );
    let back_dir = back_dir.trim_start_matches('/');

    match dst.fs.stat(rel_path).await {
        Ok(_) => {
            let back_path = format!("{}{}", back_dir, basename);
            dst.fs.create_dir(back_dir).await
                .map_err(|e| format!("cannot create BACK dir: {}", e))?;
            if let Err(e) = dst.fs.rename(rel_path, &back_path).await {
                // Displacement failed, clean up XFER
                let _ = dst.fs.delete_file(&xfer_path).await;
                return Err(format!("displacement failed: {}", e));
            }
        }
        Err(FsError::NotFound(_)) => {}
        Err(e) => {
            // Stat failed but not NotFound — try to proceed
        }
    }

    // Swap: rename XFER → final path
    dst.fs
        .rename(&xfer_path, rel_path)
        .await
        .map_err(|e| format!("swap failed: {}", e))?;

    // Set mod_time to match source
    if let Ok(src_meta) = src.fs.stat(rel_path).await {
        let _ = dst.fs.set_mod_time(rel_path, src_meta.mod_time).await;
    }

    // Clean up empty XFER directories (UUID dir, then timestamp dir)
    let uuid_dir = xfer_dir.trim_start_matches('/').trim_end_matches('/');
    let _ = dst.fs.delete_dir(uuid_dir).await;
    // Also try to remove the parent timestamp directory if empty
    if let Some(pos) = uuid_dir.rfind('/') {
        let ts_dir = &uuid_dir[..pos];
        let _ = dst.fs.delete_dir(ts_dir).await;
    }

    Ok(())
}

/// Recursive combined-tree walk.
async fn sync_directory(
    peers: &[Arc<ConnectedPeer>],
    active_indices: &[usize],
    path: &str,
    db: &Database,
    config: &crate::config::Config,
    sync_ts: &str,
    copy_tx: &mpsc::Sender<CopyOp>,
    deferred_mtimes: &mut Vec<DeferredDirMtime>,
) -> Result<(), String> {
    if active_indices.len() < 2 {
        return Ok(());
    }

    // Phase 1: List all peers in parallel
    let mut listing_futures = Vec::new();
    for &idx in active_indices {
        let fs = peers[idx].fs.clone();
        let p = path.to_string();
        listing_futures.push(tokio::spawn(async move {
            (idx, fs.list_dir(&p).await)
        }));
    }

    let mut listings: HashMap<usize, Vec<EntryMeta>> = HashMap::new();
    let mut failed_indices = Vec::new();

    for fut in listing_futures {
        let (idx, result) = fut.await.map_err(|e| format!("listing task error: {}", e))?;
        match result {
            Ok(entries) => { listings.insert(idx, entries); }
            Err(e) => {
                db.log("error", &format!("listing failed for peer '{}' at {}: {}", peers[idx].name, path, e), config.log_level());
                failed_indices.push(idx);
            }
        }
    }

    let active: Vec<usize> = active_indices
        .iter()
        .filter(|i| !failed_indices.contains(i))
        .copied()
        .collect();

    if active.len() < 2 {
        return Ok(());
    }

    // Phase 2: Union entry names
    let mut all_names: HashSet<String> = HashSet::new();
    for idx in &active {
        if let Some(entries) = listings.get(idx) {
            for e in entries {
                all_names.insert(e.name.clone());
            }
        }
    }

    // Find canon peer index (if any)
    let canon_idx = active.iter().find(|&&i| peers[i].canon).copied();

    // Phase 3: Decide and act on each entry
    for name in &all_names {
        let rel = if path.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", path, name)
        };

        // Gather per-peer state
        let mut peer_states: Vec<(usize, Option<&EntryMeta>)> = Vec::new();
        for &idx in &active {
            let entry = listings.get(&idx).and_then(|entries| {
                entries.iter().find(|e| &e.name == name)
            });
            peer_states.push((idx, entry));
        }

        // Determine entry type (file vs directory) and make decision
        let decision = make_decision(&peer_states, &rel, canon_idx, db, peers, sync_ts);

        match decision {
            Decision::NoAction => {}
            Decision::SyncFile {
                src_idx,
                mod_time,
                byte_size,
                targets,
                deletes,
            } => {
                let eid = hash::entry_id(&rel, false);
                let pid = hash::parent_id(&rel);
                let basename = name.clone();
                let mod_time_str = crate::timestamp::format_micros(mod_time);

                // Log the copy once
                if !targets.is_empty() {
                    db.log("info", &format!("C {}", rel), config.log_level());
                }

                // Update snapshot for source (confirmed present)
                db.upsert_snapshot(
                    &eid, peers[src_idx].peer_id, &pid, &basename,
                    &mod_time_str, byte_size, Some(sync_ts), None,
                ).ok();

                // Update snapshot for other peers that have it unchanged
                for &(idx, ref entry) in &peer_states {
                    if idx == src_idx { continue; }
                    if let Some(e) = entry {
                        if !e.is_dir && !targets.contains(&idx) && !deletes.contains(&idx) {
                            let mt = crate::timestamp::format_micros(e.mod_time);
                            db.upsert_snapshot(
                                &eid, peers[idx].peer_id, &pid, &basename,
                                &mt, e.byte_size, Some(sync_ts), None,
                            ).ok();
                        }
                    }
                }

                // Displace directories at target locations (type conflict)
                for &(idx, ref entry) in &peer_states {
                    if let Some(e) = entry {
                        if e.is_dir && (targets.contains(&idx) || src_idx == idx) {
                            // Type conflict: displace directory
                            displace_entry(peers, idx, &rel, sync_ts, db, config).await;
                        }
                    }
                }

                // Enqueue file copies
                for &dst_idx in &targets {
                    // Upsert destination snapshot (push decision, no last_seen update)
                    db.upsert_snapshot(
                        &eid, peers[dst_idx].peer_id, &pid, &basename,
                        &mod_time_str, byte_size, None, None,
                    ).ok();

                    let _ = copy_tx
                        .send(CopyOp {
                            src_peer_idx: src_idx,
                            dst_peer_idx: dst_idx,
                            rel_path: rel.clone(),
                            entry_id: eid.clone(),
                        })
                        .await;
                }

                // Handle deletes
                for &del_idx in &deletes {
                    db.log("info", &format!("X {}", rel), config.log_level());
                    displace_entry(peers, del_idx, &rel, sync_ts, db, config).await;
                }
            }
            Decision::SyncDir {
                src_idx,
                mod_time,
                targets,
                deletes,
                recurse_peers,
            } => {
                let eid = hash::entry_id(&rel, true);
                let pid = hash::parent_id(&rel);
                let basename = name.clone();
                let mod_time_str = crate::timestamp::format_micros(mod_time);

                // Displace type conflicts (files where we need dirs)
                for &(idx, ref entry) in &peer_states {
                    if let Some(e) = entry {
                        if !e.is_dir && recurse_peers.contains(&idx) {
                            displace_entry(peers, idx, &rel, sync_ts, db, config).await;
                        }
                    }
                }

                // Handle deletes (displace directories)
                for &del_idx in &deletes {
                    db.log("info", &format!("X {}", rel), config.log_level());
                    displace_entry(peers, del_idx, &rel, sync_ts, db, config).await;
                }

                // Create dirs where needed
                for &tgt_idx in &targets {
                    if let Err(e) = peers[tgt_idx].fs.create_dir(&rel).await {
                        db.log("error", &format!("create_dir failed {}: {}", rel, e), config.log_level());
                        continue;
                    }
                    // Defer mod_time setting until after all copies complete
                    deferred_mtimes.push(DeferredDirMtime {
                        peer_idx: tgt_idx,
                        rel_path: rel.clone(),
                        mod_time,
                    });
                    db.upsert_snapshot(
                        &eid, peers[tgt_idx].peer_id, &pid, &basename,
                        &mod_time_str, -1, Some(sync_ts), None,
                    ).ok();
                }

                // Update snapshot for existing dirs
                for &idx in &recurse_peers {
                    if let Some(e) = peer_states.iter().find(|(i, _)| *i == idx).and_then(|(_, e)| *e) {
                        if e.is_dir {
                            let mt = crate::timestamp::format_micros(e.mod_time);
                            db.upsert_snapshot(
                                &eid, peers[idx].peer_id, &pid, &basename,
                                &mt, -1, Some(sync_ts), None,
                            ).ok();
                        }
                    }
                }

                // Recurse into directory with appropriate peers
                let mut recurse_indices: Vec<usize> = recurse_peers.clone();
                recurse_indices.extend(&targets);
                recurse_indices.sort();
                recurse_indices.dedup();

                if recurse_indices.len() >= 2 {
                    Box::pin(sync_directory(
                        peers, &recurse_indices, &rel, db, config, sync_ts, copy_tx, deferred_mtimes,
                    ))
                    .await?;
                }
            }
        }
    }

    // BACK/XFER cleanup at this directory level
    cleanup_kitchensync_dirs(peers, &active, path, config).await;

    Ok(())
}

/// Displace an entry to BACK/.
async fn displace_entry(
    peers: &[Arc<ConnectedPeer>],
    peer_idx: usize,
    rel_path: &str,
    sync_ts: &str,
    db: &Database,
    config: &crate::config::Config,
) {
    let basename = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let parent = if let Some(pos) = rel_path.rfind('/') {
        &rel_path[..pos]
    } else {
        ""
    };

    let back_dir = if parent.is_empty() {
        format!(".kitchensync/BACK/{}", sync_ts)
    } else {
        format!("{}/.kitchensync/BACK/{}", parent, sync_ts)
    };
    let back_path = format!("{}/{}", back_dir, basename);

    if let Err(e) = peers[peer_idx].fs.create_dir(&back_dir).await {
        db.log("error", &format!("cannot create BACK dir: {}", e), config.log_level());
        return;
    }

    if let Err(e) = peers[peer_idx].fs.rename(rel_path, &back_path).await {
        db.log("error", &format!("displacement failed {}: {}", rel_path, e), config.log_level());
        return;
    }

    // Mark deleted in snapshot and cascade for directories
    let is_dir = peers[peer_idx].fs.stat(&back_path).await.map(|m| m.is_dir).unwrap_or(false);
    let eid = hash::entry_id(rel_path, is_dir);
    db.mark_deleted(&eid, peers[peer_idx].peer_id).ok();
    if is_dir {
        if let Some(snap) = db.get_snapshot(&eid, peers[peer_idx].peer_id) {
            let del_time = snap.last_seen.as_deref().unwrap_or(sync_ts);
            db.cascade_delete(&eid, peers[peer_idx].peer_id, del_time).ok();
        }
    }
}

/// Clean up expired BACK/ and XFER/ directories.
async fn cleanup_kitchensync_dirs(
    peers: &[Arc<ConnectedPeer>],
    active: &[usize],
    path: &str,
    config: &crate::config::Config,
) {
    let ks_path = if path.is_empty() {
        ".kitchensync".to_string()
    } else {
        format!("{}/.kitchensync", path)
    };

    let back_retention = config.back_retention_days();
    let xfer_cleanup = config.xfer_cleanup_days();

    for &idx in active {
        // Check if .kitchensync exists
        if peers[idx].fs.stat(&ks_path).await.is_err() {
            continue;
        }

        // Clean BACK/
        let back_path = format!("{}/BACK", ks_path);
        if let Ok(entries) = peers[idx].fs.list_dir(&back_path).await {
            for entry in entries {
                if entry.is_dir {
                    if let Some(ts_us) = timestamp::parse_to_micros(&entry.name) {
                        let age_days = (chrono::Utc::now().timestamp_micros() - ts_us) / (86400 * 1_000_000);
                        if age_days > back_retention as i64 {
                            let dir = format!("{}/{}", back_path, entry.name);
                            // Delete contents then directory
                            if let Ok(contents) = peers[idx].fs.list_dir(&dir).await {
                                for item in contents {
                                    let item_path = format!("{}/{}", dir, item.name);
                                    if item.is_dir {
                                        let _ = peers[idx].fs.delete_dir(&item_path).await;
                                    } else {
                                        let _ = peers[idx].fs.delete_file(&item_path).await;
                                    }
                                }
                            }
                            let _ = peers[idx].fs.delete_dir(&dir).await;
                        }
                    }
                }
            }
        }

        // Clean XFER/
        let xfer_path = format!("{}/XFER", ks_path);
        if let Ok(entries) = peers[idx].fs.list_dir(&xfer_path).await {
            for entry in entries {
                if entry.is_dir {
                    if let Some(ts_us) = timestamp::parse_to_micros(&entry.name) {
                        let age_days = (chrono::Utc::now().timestamp_micros() - ts_us) / (86400 * 1_000_000);
                        if age_days > xfer_cleanup as i64 {
                            let dir = format!("{}/{}", xfer_path, entry.name);
                            // Delete contents recursively
                            if let Ok(contents) = peers[idx].fs.list_dir(&dir).await {
                                for item in contents {
                                    let item_path = format!("{}/{}", dir, item.name);
                                    if item.is_dir {
                                        // UUID subdirectory
                                        if let Ok(sub_items) = peers[idx].fs.list_dir(&item_path).await {
                                            for si in sub_items {
                                                let _ = peers[idx].fs.delete_file(&format!("{}/{}", item_path, si.name)).await;
                                            }
                                        }
                                        let _ = peers[idx].fs.delete_dir(&item_path).await;
                                    } else {
                                        let _ = peers[idx].fs.delete_file(&item_path).await;
                                    }
                                }
                            }
                            let _ = peers[idx].fs.delete_dir(&dir).await;
                        }
                    }
                }
            }
        }
    }
}

enum Decision {
    NoAction,
    SyncFile {
        src_idx: usize,
        mod_time: i64,
        byte_size: i64,
        targets: Vec<usize>,  // peers that need the file
        deletes: Vec<usize>,  // peers where file should be deleted
    },
    SyncDir {
        src_idx: usize,
        mod_time: i64,
        targets: Vec<usize>,  // peers that need the dir created
        deletes: Vec<usize>,  // peers where dir should be deleted
        recurse_peers: Vec<usize>, // peers to recurse into
    },
}

fn make_decision(
    peer_states: &[(usize, Option<&EntryMeta>)],
    rel_path: &str,
    canon_idx: Option<usize>,
    db: &Database,
    peers: &[Arc<ConnectedPeer>],
    sync_ts: &str,
) -> Decision {
    // With canon peer
    if let Some(canon) = canon_idx {
        let canon_entry = peer_states.iter().find(|(i, _)| *i == canon).and_then(|(_, e)| *e);
        return make_canon_decision(peer_states, canon, canon_entry, rel_path, db, peers, sync_ts);
    }

    // Without canon peer - standard bidirectional rules
    make_bidir_decision(peer_states, rel_path, db, peers, sync_ts)
}

fn make_canon_decision(
    peer_states: &[(usize, Option<&EntryMeta>)],
    canon_idx: usize,
    canon_entry: Option<&EntryMeta>,
    rel_path: &str,
    db: &Database,
    peers: &[Arc<ConnectedPeer>],
    sync_ts: &str,
) -> Decision {
    match canon_entry {
        Some(entry) => {
            // Canon has the entry → push to all others
            let targets: Vec<usize> = peer_states
                .iter()
                .filter(|(idx, e)| {
                    *idx != canon_idx && needs_update(*idx, e, entry, rel_path, db, peers)
                })
                .map(|(idx, _)| *idx)
                .collect();

            if entry.is_dir {
                let mut recurse_peers: Vec<usize> = peer_states
                    .iter()
                    .filter(|(_, e)| e.map(|e| e.is_dir).unwrap_or(false))
                    .map(|(idx, _)| *idx)
                    .collect();
                recurse_peers.extend(&targets);
                recurse_peers.sort();
                recurse_peers.dedup();

                Decision::SyncDir {
                    src_idx: canon_idx,
                    mod_time: entry.mod_time,
                    targets,
                    deletes: Vec::new(),
                    recurse_peers,
                }
            } else {
                Decision::SyncFile {
                    src_idx: canon_idx,
                    mod_time: entry.mod_time,
                    byte_size: entry.byte_size,
                    targets,
                    deletes: Vec::new(),
                }
            }
        }
        None => {
            // Canon lacks the entry → delete everywhere
            let deletes: Vec<usize> = peer_states
                .iter()
                .filter(|(idx, e)| *idx != canon_idx && e.is_some())
                .map(|(idx, _)| *idx)
                .collect();

            if deletes.is_empty() {
                Decision::NoAction
            } else {
                // Use file sync decision for deletes (the type doesn't matter since we're deleting)
                Decision::SyncFile {
                    src_idx: canon_idx,
                    mod_time: 0,
                    byte_size: 0,
                    targets: Vec::new(),
                    deletes,
                }
            }
        }
    }
}

fn make_bidir_decision(
    peer_states: &[(usize, Option<&EntryMeta>)],
    rel_path: &str,
    db: &Database,
    peers: &[Arc<ConnectedPeer>],
    sync_ts: &str,
) -> Decision {
    let present: Vec<(usize, &EntryMeta)> = peer_states
        .iter()
        .filter_map(|(idx, e)| e.map(|e| (*idx, e)))
        .collect();
    let absent: Vec<usize> = peer_states
        .iter()
        .filter(|(_, e)| e.is_none())
        .map(|(idx, _)| *idx)
        .collect();

    if present.is_empty() {
        return Decision::NoAction;
    }

    // Determine winning entry: newest mod_time wins (with tolerance)
    let max_mod_time = present.iter().map(|(_, e)| e.mod_time).max().unwrap();

    // Check for deletions on absent peers
    let mut deletion_votes: Vec<(usize, i64)> = Vec::new(); // (peer_idx, deletion_estimate)
    for &absent_idx in &absent {
        // Check both file and dir IDs
        for is_dir in [false, true] {
            let eid = hash::entry_id(rel_path, is_dir);
            if let Some(snap) = db.get_snapshot(&eid, peers[absent_idx].peer_id) {
                if let Some(ref del_time) = snap.deleted_time {
                    // Already a tombstone
                    if let Some(dt_us) = timestamp::parse_to_micros(del_time) {
                        deletion_votes.push((absent_idx, dt_us));
                    }
                } else {
                    // Absent-unconfirmed (rule 4b)
                    if let Some(ref last_seen) = snap.last_seen {
                        if let Some(ls_us) = timestamp::parse_to_micros(last_seen) {
                            // last_seen must exceed max mod_time by more than tolerance
                            if ls_us > max_mod_time + timestamp::TOLERANCE_US {
                                deletion_votes.push((absent_idx, ls_us));
                            }
                            // Otherwise: re-enqueue copy (not a deletion vote)
                        }
                    }
                    // last_seen NULL: not a deletion vote, re-enqueue
                }
            }
        }
    }

    let deletion_wins = if !deletion_votes.is_empty() {
        let max_deletion_estimate = deletion_votes.iter().map(|(_, est)| *est).max().unwrap();
        // Deletion wins if estimate > max mod_time (with tolerance)
        max_deletion_estimate > max_mod_time + timestamp::TOLERANCE_US
    } else {
        false
    };

    if deletion_wins {
        // Delete from all peers that have it
        let deletes: Vec<usize> = present.iter().map(|(idx, _)| *idx).collect();
        if deletes.is_empty() {
            return Decision::NoAction;
        }
        Decision::SyncFile {
            src_idx: deletes[0], // doesn't matter, no targets
            mod_time: 0,
            byte_size: 0,
            targets: Vec::new(),
            deletes,
        }
    } else {
        // Find winner(s): within tolerance of max_mod_time
        let winners: Vec<(usize, &EntryMeta)> = present
            .iter()
            .filter(|(_, e)| timestamp::within_tolerance(e.mod_time, max_mod_time))
            .copied()
            .collect();

        // Among tied winners, prefer files over dirs (rule 6), then larger over smaller (rule 5)
        let winner = winners
            .iter()
            .max_by(|(_, a), (_, b)| {
                // Files win over dirs (byte_size -1 means dir)
                let a_file = !a.is_dir;
                let b_file = !b.is_dir;
                a_file.cmp(&b_file)
                    .then(a.byte_size.cmp(&b.byte_size))
            })
            .unwrap();

        let (src_idx, winner_entry) = *winner;

        if winner_entry.is_dir {
            // Directory sync
            let targets: Vec<usize> = peer_states
                .iter()
                .filter(|(idx, e)| {
                    *idx != src_idx && match e {
                        Some(e) if e.is_dir => false, // already has dir
                        _ => true, // absent or wrong type
                    }
                })
                .filter(|(idx, _)| !deletion_wins)
                .map(|(idx, _)| *idx)
                .collect();

            let recurse_peers: Vec<usize> = peer_states
                .iter()
                .filter(|(_, e)| e.map(|e| e.is_dir).unwrap_or(false))
                .map(|(idx, _)| *idx)
                .collect();

            Decision::SyncDir {
                src_idx,
                mod_time: winner_entry.mod_time,
                targets,
                deletes: Vec::new(),
                recurse_peers,
            }
        } else {
            // File sync
            let targets: Vec<usize> = peer_states
                .iter()
                .filter(|(idx, e)| {
                    *idx != src_idx
                        && needs_update(*idx, e, winner_entry, rel_path, db, peers)
                })
                .map(|(idx, _)| *idx)
                .collect();

            Decision::SyncFile {
                src_idx,
                mod_time: winner_entry.mod_time,
                byte_size: winner_entry.byte_size,
                targets,
                deletes: Vec::new(),
            }
        }
    }
}

fn needs_update(
    peer_idx: usize,
    peer_entry: &Option<&EntryMeta>,
    winner: &EntryMeta,
    rel_path: &str,
    db: &Database,
    peers: &[Arc<ConnectedPeer>],
) -> bool {
    match peer_entry {
        Some(e) => {
            if e.is_dir != winner.is_dir {
                return true; // type conflict
            }
            if winner.is_dir {
                return false; // dir already exists
            }
            // Same type (file): check if mod_time and size match
            if timestamp::within_tolerance(e.mod_time, winner.mod_time)
                && e.byte_size == winner.byte_size
            {
                return false; // already up to date
            }
            true
        }
        None => true, // peer doesn't have it
    }
}
