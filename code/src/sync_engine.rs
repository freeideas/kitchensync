use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::copy::{self, CopyTask};
use crate::database::Database;
use crate::hash;
use crate::logging::Logger;
use crate::peer::{DirEntry, PeerError};
use crate::pool::ConnectedPeer;
use crate::timestamp;

const TIMESTAMP_TOLERANCE_SECS: f64 = 5.0;

pub fn run(
    peers: &[Arc<ConnectedPeer>],
    db: &Arc<Database>,
    logger: &Arc<Logger>,
    canon: Option<&str>,
    sync_stamp: &str,
    config: &crate::config::Config,
) {
    let copy_queue = Arc::new(Mutex::new(Vec::<CopyTask>::new()));

    sync_directory(peers, "", db, logger, canon, sync_stamp, &copy_queue, config);

    let tasks = std::mem::take(&mut *copy_queue.lock().unwrap());
    if !tasks.is_empty() {
        logger.debug(&format!("Executing {} file copies", tasks.len()));
        copy::execute_copies(tasks, peers, db, logger, sync_stamp);
    }
}

fn sync_directory(
    peers: &[Arc<ConnectedPeer>],
    path: &str,
    db: &Arc<Database>,
    logger: &Arc<Logger>,
    canon: Option<&str>,
    sync_stamp: &str,
    copy_queue: &Arc<Mutex<Vec<CopyTask>>>,
    config: &crate::config::Config,
) {
    // Phase 1: List all peers in parallel
    let listings: Vec<(String, Result<Vec<DirEntry>, PeerError>)> = thread::scope(|s| {
        let handles: Vec<_> = peers
            .iter()
            .map(|peer| {
                let name = peer.name.clone();
                let path = path.to_string();
                s.spawn(move || {
                    let result = peer.listing_conn.list_dir(&path);
                    (name, result)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    // Phase 1b: Drop peers with listing errors
    let mut active_listings: HashMap<String, Vec<DirEntry>> = HashMap::new();
    for (name, result) in listings {
        match result {
            Ok(entries) => {
                active_listings.insert(name, entries);
            }
            Err(e) => {
                logger.error(&format!(
                    "Listing failed for {} at {}: {}",
                    name,
                    if path.is_empty() { "/" } else { path },
                    e
                ));
            }
        }
    }

    let active_peers: Vec<&Arc<ConnectedPeer>> = peers
        .iter()
        .filter(|p| active_listings.contains_key(&p.name))
        .collect();

    if active_peers.is_empty() {
        return;
    }

    // Phase 2: Union entry names
    let mut all_names: HashSet<String> = HashSet::new();
    for entries in active_listings.values() {
        for entry in entries {
            all_names.insert(entry.name.clone());
        }
    }
    let mut sorted_names: Vec<String> = all_names.into_iter().collect();
    sorted_names.sort();

    // Phase 3: Decide and act on each entry
    for name in &sorted_names {
        let entry_path = if path.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", path, name)
        };

        let mut states: HashMap<String, Option<DirEntry>> = HashMap::new();
        for peer in &active_peers {
            let entry = active_listings
                .get(&peer.name)
                .and_then(|entries| entries.iter().find(|e| e.name == *name))
                .cloned();
            states.insert(peer.name.clone(), entry);
        }

        let decision = decide(&entry_path, &states, &active_peers, db, canon, sync_stamp);

        match decision {
            Decision::NoAction => {}
            Decision::SyncFile {
                winner_peer,
                winner_mod_time,
                winner_size,
                push_to,
                delete_from,
            } => {
                let id = hash::path_hash(&entry_path);
                let parent_id = hash::parent_hash(&entry_path);
                let bn = hash::basename(&entry_path);

                // Update snapshot for present peers
                for peer in &active_peers {
                    if let Some(Some(entry)) = states.get(&peer.name) {
                        if !entry.is_dir {
                            db.confirm_present(
                                &id, &peer.name, &parent_id, bn,
                                &entry.mod_time, entry.byte_size, sync_stamp,
                            );
                        }
                    }
                }

                // Confirm absent for peers that should have the entry gone
                for peer in &active_peers {
                    if states.get(&peer.name).and_then(|e| e.as_ref()).is_none() {
                        if delete_from.contains(&peer.name) || push_to.is_empty() {
                            db.confirm_absent(&id, &peer.name);
                        }
                    }
                }

                // Type conflicts: displace directories where a file should go
                for peer in &active_peers {
                    if let Some(Some(entry)) = states.get(&peer.name) {
                        if entry.is_dir {
                            if let Err(e) = copy::displace_to_back(
                                peer.listing_conn.as_ref(),
                                &entry_path,
                                logger,
                            ) {
                                logger.error(&format!("Displace type conflict {}: {}", entry_path, e));
                            } else {
                                let dir_id = hash::path_hash(&format!("{}/", entry_path));
                                db.cascade_delete(&dir_id, &peer.name, sync_stamp);
                            }
                        }
                    }
                }

                // Delete from specified peers
                for peer_name in &delete_from {
                    if let Some(peer) = active_peers.iter().find(|p| p.name == *peer_name) {
                        if let Err(e) = copy::displace_to_back(
                            peer.listing_conn.as_ref(),
                            &entry_path,
                            logger,
                        ) {
                            logger.error(&format!("Displace failed {}: {}", entry_path, e));
                        } else {
                            db.mark_deleted(&id, peer_name);
                        }
                    }
                }

                // Log once per decision
                if !push_to.is_empty() {
                    logger.info(&format!("C {}", entry_path));
                } else if !delete_from.is_empty() {
                    logger.info(&format!("X {}", entry_path));
                }

                // Snapshot for push destinations
                for dst in &push_to {
                    db.upsert_snapshot_push(&id, dst, &parent_id, bn, &winner_mod_time, winner_size);
                }

                // Enqueue copies
                for dst in push_to {
                    copy_queue.lock().unwrap().push(CopyTask {
                        src_peer: winner_peer.clone(),
                        dst_peer: dst,
                        path: entry_path.clone(),
                        src_mod_time: winner_mod_time.clone(),
                    });
                }
            }
            Decision::SyncDir {
                winner_peer,
                winner_mod_time,
                create_on,
                delete_from,
                recurse_peers,
            } => {
                let dir_path_slash = format!("{}/", entry_path);
                let id = hash::path_hash(&dir_path_slash);
                let parent_id = hash::parent_hash(&dir_path_slash);
                let bn = hash::basename(&entry_path);

                // Update snapshot for peers that have the dir
                for peer in &active_peers {
                    if let Some(Some(entry)) = states.get(&peer.name) {
                        if entry.is_dir {
                            db.confirm_present(
                                &id, &peer.name, &parent_id, bn,
                                &entry.mod_time, -1, sync_stamp,
                            );
                        }
                    }
                }

                // Type conflicts: displace files where a dir should go
                for peer in &active_peers {
                    if let Some(Some(entry)) = states.get(&peer.name) {
                        if !entry.is_dir {
                            if let Err(e) = copy::displace_to_back(
                                peer.listing_conn.as_ref(),
                                &entry_path,
                                logger,
                            ) {
                                logger.error(&format!("Displace type conflict {}: {}", entry_path, e));
                            }
                        }
                    }
                }

                // Delete dirs from specified peers (don't recurse into them)
                for peer_name in &delete_from {
                    if let Some(peer) = active_peers.iter().find(|p| p.name == *peer_name) {
                        if let Err(e) = copy::displace_to_back(
                            peer.listing_conn.as_ref(),
                            &entry_path,
                            logger,
                        ) {
                            logger.error(&format!("Displace dir failed {}: {}", entry_path, e));
                        } else {
                            logger.info(&format!("X {}", entry_path));
                            db.cascade_delete(&id, peer_name, sync_stamp);
                        }
                    }
                }

                // Create dirs on specified peers
                for peer_name in &create_on {
                    if let Some(peer) = active_peers.iter().find(|p| p.name == *peer_name) {
                        if let Err(e) = peer.listing_conn.create_dir(&entry_path) {
                            logger.error(&format!("Create dir failed {}: {}", entry_path, e));
                            continue;
                        }
                        let _ = peer.listing_conn.set_mod_time(&entry_path, &winner_mod_time);
                        db.confirm_present(
                            &id, peer_name, &parent_id, bn,
                            &winner_mod_time, -1, sync_stamp,
                        );
                    }
                }

                // Recurse into directory
                let recurse_peer_arcs: Vec<Arc<ConnectedPeer>> = active_peers
                    .iter()
                    .filter(|p| recurse_peers.contains(&p.name))
                    .map(|p| (*p).clone())
                    .collect();

                if !recurse_peer_arcs.is_empty() {
                    sync_directory(
                        &recurse_peer_arcs, &entry_path, db, logger,
                        canon, sync_stamp, copy_queue, config,
                    );
                }
            }
        }
    }

    // BACK/XFER cleanup at this directory level
    cleanup_back_xfer(&active_peers, path, config, logger);
}

enum Decision {
    NoAction,
    SyncFile {
        winner_peer: String,
        winner_mod_time: String,
        winner_size: i64,
        push_to: Vec<String>,
        delete_from: Vec<String>,
    },
    SyncDir {
        winner_peer: String,
        winner_mod_time: String,
        create_on: Vec<String>,
        delete_from: Vec<String>,
        recurse_peers: Vec<String>,
    },
}

#[derive(Debug, Clone)]
enum EntryClass {
    Unchanged,
    Modified,
    New,
    Deleted { estimate: String },
    AbsentUnconfirmed { last_seen: Option<String> },
    NeverExisted,
}

fn classify_entry(
    _peer_name: &str,
    has_entry: bool,
    entry: Option<&DirEntry>,
    snap: Option<&crate::database::SnapshotRow>,
) -> EntryClass {
    match (has_entry, snap) {
        (true, Some(row)) => {
            if row.deleted_time.is_some() {
                EntryClass::Modified // resurrection
            } else {
                let live_mod = entry.unwrap().mod_time.as_str();
                if timestamp::within_tolerance(live_mod, &row.mod_time, TIMESTAMP_TOLERANCE_SECS) {
                    EntryClass::Unchanged
                } else {
                    EntryClass::Modified
                }
            }
        }
        (true, None) => EntryClass::New,
        (false, Some(row)) => {
            if row.deleted_time.is_some() {
                EntryClass::Deleted {
                    estimate: row.deleted_time.clone().unwrap(),
                }
            } else {
                EntryClass::AbsentUnconfirmed {
                    last_seen: row.last_seen.clone(),
                }
            }
        }
        (false, None) => EntryClass::NeverExisted,
    }
}

fn decide(
    entry_path: &str,
    states: &HashMap<String, Option<DirEntry>>,
    active_peers: &[&Arc<ConnectedPeer>],
    db: &Database,
    canon: Option<&str>,
    sync_stamp: &str,
) -> Decision {
    let mut has_file = false;
    let mut has_dir = false;

    for (_, entry) in states {
        if let Some(e) = entry {
            if e.is_dir { has_dir = true; } else { has_file = true; }
        }
    }

    // Determine hash path
    let is_dir_decision = has_dir && !has_file;
    let hash_path = if is_dir_decision {
        format!("{}/", entry_path)
    } else {
        entry_path.to_string()
    };
    let id = hash::path_hash(&hash_path);

    // Get snapshot rows
    let mut snapshots: HashMap<String, Option<crate::database::SnapshotRow>> = HashMap::new();
    for peer in active_peers {
        snapshots.insert(peer.name.clone(), db.get_snapshot(&id, &peer.name));
    }

    // Classify each peer
    let mut classifications: HashMap<String, EntryClass> = HashMap::new();
    for peer in active_peers {
        let entry = states.get(&peer.name).and_then(|e| e.as_ref());
        let snap = snapshots.get(&peer.name).and_then(|s| s.as_ref());
        classifications.insert(
            peer.name.clone(),
            classify_entry(&peer.name, entry.is_some(), entry, snap),
        );
    }

    if let Some(canon_name) = canon {
        return decide_canon(entry_path, canon_name, states, active_peers, db, sync_stamp);
    }

    // Collect present peers
    let mut present_peers: Vec<(String, String, i64, bool)> = Vec::new();
    for peer in active_peers {
        if let Some(Some(entry)) = states.get(&peer.name) {
            present_peers.push((
                peer.name.clone(),
                entry.mod_time.clone(),
                entry.byte_size,
                entry.is_dir,
            ));
        }
    }

    if present_peers.is_empty() {
        return Decision::NoAction;
    }

    // Collect deletion estimates
    let mut deletion_estimates: Vec<(String, String)> = Vec::new();
    let max_present_mod = present_peers.iter().map(|(_, mt, _, _)| mt.as_str()).max().unwrap_or("");

    for peer in active_peers {
        match classifications.get(&peer.name) {
            Some(EntryClass::Deleted { estimate }) => {
                deletion_estimates.push((peer.name.clone(), estimate.clone()));
            }
            Some(EntryClass::AbsentUnconfirmed { last_seen }) => {
                if let Some(ls) = last_seen {
                    if !max_present_mod.is_empty() && timestamp::is_newer(ls, max_present_mod) {
                        deletion_estimates.push((peer.name.clone(), ls.clone()));
                    }
                }
            }
            _ => {}
        }
    }

    let max_mod_time = present_peers.iter().map(|(_, mt, _, _)| mt.as_str()).max().unwrap().to_string();

    // Check if deletion wins (rule 4)
    if !deletion_estimates.is_empty() {
        let max_del = deletion_estimates.iter().map(|(_, e)| e.as_str()).max().unwrap().to_string();
        if timestamp::is_newer(&max_del, &max_mod_time)
            && !timestamp::within_tolerance(&max_del, &max_mod_time, TIMESTAMP_TOLERANCE_SECS)
        {
            let delete_from: Vec<String> = present_peers.iter().map(|(n, _, _, _)| n.clone()).collect();
            if is_dir_decision {
                return Decision::SyncDir {
                    winner_peer: String::new(),
                    winner_mod_time: String::new(),
                    create_on: Vec::new(),
                    delete_from,
                    recurse_peers: Vec::new(),
                };
            } else {
                return Decision::SyncFile {
                    winner_peer: String::new(),
                    winner_mod_time: max_mod_time,
                    winner_size: 0,
                    push_to: Vec::new(),
                    delete_from,
                };
            }
        }
    }

    // Find winner: within tolerance of max, then by rule 5/6
    let mut winners: Vec<&(String, String, i64, bool)> = present_peers
        .iter()
        .filter(|(_, mt, _, _)| timestamp::within_tolerance(mt, &max_mod_time, TIMESTAMP_TOLERANCE_SECS))
        .collect();

    // Rule 5/6: files beat dirs, larger beats smaller
    winners.sort_by(|a, b| {
        let a_file = !a.3;
        let b_file = !b.3;
        if a_file != b_file {
            return b_file.cmp(&a_file);
        }
        b.2.cmp(&a.2)
    });

    let winner = winners.first().unwrap();
    let winner_peer = winner.0.clone();
    let winner_mod_time = winner.1.clone();
    let winner_size = winner.2;
    let winner_is_dir = winner.3;

    // Check if all peers are unchanged
    let all_unchanged = classifications.values().all(|c| matches!(c, EntryClass::Unchanged));
    if all_unchanged && deletion_estimates.is_empty() {
        return Decision::NoAction;
    }

    let mut push_to = Vec::new();
    let mut delete_from_type_conflict = Vec::new();

    for peer in active_peers {
        if peer.name == winner_peer {
            continue;
        }
        match states.get(&peer.name).and_then(|e| e.as_ref()) {
            Some(entry) => {
                if entry.is_dir != winner_is_dir {
                    delete_from_type_conflict.push(peer.name.clone());
                    push_to.push(peer.name.clone());
                } else if !timestamp::within_tolerance(&entry.mod_time, &winner_mod_time, TIMESTAMP_TOLERANCE_SECS)
                    || entry.byte_size != winner_size
                {
                    push_to.push(peer.name.clone());
                }
            }
            None => {
                // Absent peer — push unless it's a deletion that already won (handled above)
                push_to.push(peer.name.clone());
            }
        }
    }

    if winner_is_dir {
        let recurse_peers: Vec<String> = active_peers
            .iter()
            .filter(|p| !delete_from_type_conflict.contains(&p.name))
            .map(|p| p.name.clone())
            .collect();

        Decision::SyncDir {
            winner_peer,
            winner_mod_time,
            create_on: push_to,
            delete_from: delete_from_type_conflict,
            recurse_peers,
        }
    } else {
        Decision::SyncFile {
            winner_peer,
            winner_mod_time,
            winner_size,
            push_to,
            delete_from: delete_from_type_conflict,
        }
    }
}

fn decide_canon(
    entry_path: &str,
    canon_name: &str,
    states: &HashMap<String, Option<DirEntry>>,
    active_peers: &[&Arc<ConnectedPeer>],
    db: &Database,
    sync_stamp: &str,
) -> Decision {
    let canon_entry = states.get(canon_name).and_then(|e| e.as_ref());

    match canon_entry {
        Some(entry) if entry.is_dir => {
            let create_on: Vec<String> = active_peers
                .iter()
                .filter(|p| {
                    p.name != canon_name
                        && !states.get(&p.name).and_then(|e| e.as_ref()).map(|e| e.is_dir).unwrap_or(false)
                })
                .map(|p| p.name.clone())
                .collect();
            let delete_from: Vec<String> = active_peers
                .iter()
                .filter(|p| {
                    p.name != canon_name
                        && states.get(&p.name).and_then(|e| e.as_ref()).map(|e| !e.is_dir).unwrap_or(false)
                })
                .map(|p| p.name.clone())
                .collect();
            let recurse_peers: Vec<String> = active_peers
                .iter()
                .filter(|p| !delete_from.contains(&p.name))
                .map(|p| p.name.clone())
                .collect();
            Decision::SyncDir {
                winner_peer: canon_name.to_string(),
                winner_mod_time: entry.mod_time.clone(),
                create_on,
                delete_from,
                recurse_peers,
            }
        }
        Some(entry) => {
            let push_to: Vec<String> = active_peers
                .iter()
                .filter(|p| {
                    if p.name == canon_name { return false; }
                    match states.get(&p.name).and_then(|e| e.as_ref()) {
                        Some(existing) => {
                            existing.is_dir
                                || !timestamp::within_tolerance(&existing.mod_time, &entry.mod_time, TIMESTAMP_TOLERANCE_SECS)
                                || existing.byte_size != entry.byte_size
                        }
                        None => true,
                    }
                })
                .map(|p| p.name.clone())
                .collect();
            let delete_from: Vec<String> = active_peers
                .iter()
                .filter(|p| {
                    p.name != canon_name
                        && states.get(&p.name).and_then(|e| e.as_ref()).map(|e| e.is_dir).unwrap_or(false)
                })
                .map(|p| p.name.clone())
                .collect();
            Decision::SyncFile {
                winner_peer: canon_name.to_string(),
                winner_mod_time: entry.mod_time.clone(),
                winner_size: entry.byte_size,
                push_to,
                delete_from,
            }
        }
        None => {
            // Canon lacks the entry — delete everywhere else
            let delete_from: Vec<String> = active_peers
                .iter()
                .filter(|p| p.name != canon_name && states.get(&p.name).and_then(|e| e.as_ref()).is_some())
                .map(|p| p.name.clone())
                .collect();
            if delete_from.is_empty() {
                Decision::NoAction
            } else {
                Decision::SyncFile {
                    winner_peer: String::new(),
                    winner_mod_time: String::new(),
                    winner_size: 0,
                    push_to: Vec::new(),
                    delete_from,
                }
            }
        }
    }
}

fn cleanup_back_xfer(
    active_peers: &[&Arc<ConnectedPeer>],
    path: &str,
    config: &crate::config::Config,
    logger: &Logger,
) {
    for peer in active_peers {
        let ks_path = if path.is_empty() {
            ".kitchensync".to_string()
        } else {
            format!("{}/.kitchensync", path)
        };

        if let Ok(Some(st)) = peer.listing_conn.stat(&ks_path) {
            if !st.is_dir { continue; }

            // Clean BACK/
            let back_path = format!("{}/BACK", ks_path);
            if let Ok(entries) = peer.listing_conn.list_dir(&back_path) {
                for entry in entries {
                    if entry.is_dir {
                        if let Some(age) = timestamp::age_days(&entry.name) {
                            if age > config.back_retention_days as f64 {
                                let full = format!("{}/{}", back_path, entry.name);
                                // Remove children then dir
                                if let Ok(children) = peer.listing_conn.list_dir(&full) {
                                    for child in children {
                                        let cp = format!("{}/{}", full, child.name);
                                        if child.is_dir {
                                            let _ = peer.listing_conn.delete_dir(&cp);
                                        } else {
                                            let _ = peer.listing_conn.delete_file(&cp);
                                        }
                                    }
                                }
                                let _ = peer.listing_conn.delete_dir(&full);
                            }
                        }
                    }
                }
            }

            // Clean XFER/
            let xfer_path = format!("{}/XFER", ks_path);
            if let Ok(entries) = peer.listing_conn.list_dir(&xfer_path) {
                for entry in entries {
                    if entry.is_dir {
                        if let Some(age) = timestamp::age_days(&entry.name) {
                            if age > config.xfer_cleanup_days as f64 {
                                let full = format!("{}/{}", xfer_path, entry.name);
                                if let Ok(uuid_dirs) = peer.listing_conn.list_dir(&full) {
                                    for uuid_dir in uuid_dirs {
                                        let up = format!("{}/{}", full, uuid_dir.name);
                                        if uuid_dir.is_dir {
                                            if let Ok(files) = peer.listing_conn.list_dir(&up) {
                                                for f in files {
                                                    let _ = peer.listing_conn.delete_file(&format!("{}/{}", up, f.name));
                                                }
                                            }
                                            let _ = peer.listing_conn.delete_dir(&up);
                                        } else {
                                            let _ = peer.listing_conn.delete_file(&up);
                                        }
                                    }
                                }
                                let _ = peer.listing_conn.delete_dir(&full);
                            }
                        }
                    }
                }
            }
        }
    }
}
