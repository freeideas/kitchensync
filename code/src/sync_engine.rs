use crate::backup;
use crate::decision::{compute_actions, SyncAction};
use crate::peer::{PeerRole, PeerSpec, Scheme};
use crate::snapshot::Snapshot;
use crate::staging;
use crate::syncignore::IgnoreRules;
use crate::transport::Transport;
use crate::local_transport::LocalTransport;
use crate::sftp_transport::SftpTransport;
use chrono::Utc;
use log::{debug, info, trace};
use std::collections::HashMap;
use std::io;
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

/// Connection pool for a single URL, limiting concurrent connections via a counting semaphore.
struct ConnectionPool {
    url_label: String,
    state: Mutex<usize>,
    condvar: Condvar,
    max: usize,
}

impl ConnectionPool {
    fn new(url_label: String, max: usize) -> Self {
        Self {
            url_label,
            state: Mutex::new(0),
            condvar: Condvar::new(),
            max,
        }
    }

    fn acquire(&self) {
        let mut active = self.state.lock().unwrap();
        while *active >= self.max {
            active = self.condvar.wait(active).unwrap();
        }
        *active += 1;
        trace!("pool acquire url={} connections={}/{}", self.url_label, *active, self.max);
    }

    fn release(&self) {
        let mut active = self.state.lock().unwrap();
        *active -= 1;
        trace!("pool release url={} connections={}/{}", self.url_label, *active, self.max);
        self.condvar.notify_one();
    }
}

struct ConnectedPeer {
    spec: PeerSpec,
    listing_transport: Box<dyn Transport>,
    connection_pool: Arc<ConnectionPool>,
    transport: Box<dyn Transport>,
    snapshot: Snapshot,
}

/// Run the full sync across all peers.
pub fn run_sync(
    peers: &[PeerSpec],
    global_max_connections: usize,
    global_connect_timeout: u64,
    staging_expiry_days: u64,
    backup_expiry_days: u64,
    tombstone_expiry_days: u64,
) -> io::Result<()> {
    info!("Connecting to {} peers...", peers.len());

    // Step 1: Connect to all peers in parallel
    let connected = connect_all_peers(peers, global_max_connections, global_connect_timeout)?;

    // Startup purge: remove old tombstones and stale rows (REQ_DB_024, REQ_DB_025)
    {
        let now = Utc::now().timestamp();
        for peer in &connected {
            let cutoff = now - (tombstone_expiry_days as i64 * 86400);
            peer.snapshot.purge_old_tombstones(cutoff)?;
            peer.snapshot.purge_stale_rows(cutoff)?;
        }
    }

    info!("All peers connected. Listing files...");

    // Step 2: Walk the combined directory tree (concurrently across peers)
    let peer_entries = list_all_peers(&connected)?;

    // Step 3: Load snapshots
    let peer_snapshots: Vec<HashMap<String, crate::snapshot::SnapshotEntry>> = connected
        .iter()
        .map(|p| p.snapshot.all_entries())
        .collect::<io::Result<Vec<_>>>()?;

    let peer_roles: Vec<PeerRole> = connected.iter().map(|p| p.spec.role).collect();

    // REQ_MTS_044: Canon required on first sync (no peer has snapshot history)
    let has_canon = peer_roles.iter().any(|r| *r == PeerRole::Canon);
    let any_has_snapshot = connected.iter().any(|p| p.snapshot.has_history);
    if !has_canon && !any_has_snapshot {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Canon (+) is required on first sync when no peer has snapshot history\nFirst sync? Mark the authoritative peer with a leading +",
        ));
    }

    // REQ_ERR_011: After auto-subordination of snapshotless peers, at least one
    // contributing (non-subordinate) peer must be reachable.
    let has_contributing = connected.iter().any(|p| {
        p.spec.role != PeerRole::Subordinate && (p.snapshot.has_history || p.spec.role == PeerRole::Canon)
    });
    if !has_contributing {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "No contributing peer reachable",
        ));
    }

    // Step 4: Compute sync actions
    info!("Computing sync actions...");
    let peer_has_history: Vec<bool> = connected.iter().map(|p| p.snapshot.has_history).collect();
    let actions = compute_actions(&peer_roles, &peer_entries, &peer_snapshots, &peer_has_history);

    if actions.is_empty() {
        info!("All peers are in sync. Nothing to do.");
    } else {
        info!("{} actions to perform.", actions.len());
    }

    // Step 5: Execute actions
    execute_actions(&actions, &connected)?;

    // Step 6: Update snapshots
    info!("Updating snapshots...");
    let now = Utc::now().timestamp();
    update_snapshots(&connected, &peer_entries, &actions, now)?;

    // REQ_MTS_036: After copies complete, set last_seen on destination
    finalize_copies(&connected, &actions, now)?;

    // Step 7: Upload snapshots back to peers
    upload_snapshots(&connected)?;

    // Step 8: Purge old backups and staging at all directory levels (REQ_SYNCOP_024)
    {
        let mut dirs_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        dirs_seen.insert(String::new()); // root
        for entries in peer_entries.iter() {
            for (rel_path, (is_dir, _, _)) in entries {
                if *is_dir {
                    dirs_seen.insert(rel_path.clone());
                }
                // Also add the parent directory
                if let Some(pos) = rel_path.rfind('/') {
                    dirs_seen.insert(rel_path[..pos].to_string());
                }
            }
        }
        for peer in &connected {
            for dir in &dirs_seen {
                let _ = backup::purge_old_backups(peer.transport.as_ref(), dir, backup_expiry_days);
                let _ = staging::purge_old_staging(peer.transport.as_ref(), dir, staging_expiry_days);
            }
        }
    }

    info!("Sync complete.");
    Ok(())
}

fn connect_all_peers(
    peers: &[PeerSpec],
    global_max_connections: usize,
    global_timeout: u64,
) -> io::Result<Vec<ConnectedPeer>> {
    let results: Arc<Mutex<Vec<(usize, io::Result<ConnectedPeer>)>>> =
        Arc::new(Mutex::new(Vec::new()));

    let mut handles = Vec::new();

    for (i, spec) in peers.iter().enumerate() {
        let spec = spec.clone();
        let results = Arc::clone(&results);

        let handle = thread::spawn(move || {
            let result = connect_peer(&spec, global_max_connections, global_timeout);
            results.lock().unwrap().push((i, result));
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "Thread panicked during connection")
        })?;
    }

    let mut results = results.lock().unwrap();
    results.sort_by_key(|(i, _)| *i);

    let mut connected = Vec::new();
    for (i, result) in results.drain(..) {
        match result {
            Ok(peer) => connected.push(peer),
            Err(e) => {
                // REQ_MTS_017: Canon peer unreachable → fatal error
                if peers[i].role == PeerRole::Canon {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionRefused,
                        format!("Canon peer {} is unreachable: {}", i, e),
                    ));
                }
                // REQ_MTS_043: Non-canon unreachable peers are excluded
                log::warn!("Peer {} unreachable, excluding: {}", i, e);
            }
        }
    }

    if connected.len() < 2 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Not enough reachable peers (need at least 2)",
        ));
    }

    Ok(connected)
}

fn connect_peer(spec: &PeerSpec, global_max_connections: usize, global_timeout: u64) -> io::Result<ConnectedPeer> {
    let mut last_err = None;

    for url in &spec.urls {
        let timeout = url.connect_timeout.unwrap_or(global_timeout);
        let max_conn = url.max_connections.unwrap_or(global_max_connections);

        let transport_result: io::Result<Box<dyn Transport>> = match url.scheme {
            Scheme::Local => {
                LocalTransport::new(&url.path).map(|t| Box::new(t) as Box<dyn Transport>)
            }
            Scheme::Sftp => {
                let host = url.host.as_deref().unwrap_or("localhost");
                SftpTransport::connect(
                    host,
                    url.port,
                    url.username.as_deref(),
                    url.password.as_deref(),
                    timeout,
                    &url.path,
                )
                .map(|t| Box::new(t) as Box<dyn Transport>)
            }
        };

        match transport_result {
            Ok(transport) => {
                let snapshot = load_snapshot(transport.as_ref())?;

                // Create a separate listing transport (dedicated connection outside pool)
                let listing_transport: Box<dyn Transport> = match url.scheme {
                    Scheme::Local => {
                        Box::new(LocalTransport::new(&url.path)?)
                    }
                    Scheme::Sftp => {
                        let host = url.host.as_deref().unwrap_or("localhost");
                        Box::new(SftpTransport::connect(
                            host,
                            url.port,
                            url.username.as_deref(),
                            url.password.as_deref(),
                            timeout,
                            &url.path,
                        )?)
                    }
                };

                let url_label = if url.scheme == Scheme::Sftp {
                    format!("sftp://{}:{}{}", url.host.as_deref().unwrap_or("localhost"), url.port, url.path)
                } else {
                    format!("file://{}", url.path)
                };

                let pool = Arc::new(ConnectionPool::new(url_label, max_conn));

                return Ok(ConnectedPeer {
                    spec: spec.clone(),
                    listing_transport,
                    connection_pool: pool,
                    transport,
                    snapshot,
                });
            }
            Err(e) => {
                debug!("Failed to connect via {}: {}", url.path, e);
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| {
        io::Error::new(io::ErrorKind::NotConnected, "No URLs to try")
    }))
}

fn load_snapshot(transport: &dyn Transport) -> io::Result<Snapshot> {
    match transport.read_file(".kitchensync/snapshot.db") {
        Ok(data) => Snapshot::from_bytes(&data),
        Err(_) => Snapshot::new_empty(),
    }
}

fn list_all_peers(
    peers: &[ConnectedPeer],
) -> io::Result<Vec<HashMap<String, (bool, i64, u64)>>> {
    // Issue directory listings concurrently across all peers (REQ_CONC_017, REQ_CONC_019)
    // Extract listing transports so we only pass Send+Sync references to threads.
    let transports: Vec<&dyn Transport> = peers.iter().map(|p| p.listing_transport.as_ref()).collect();
    let results: Arc<Mutex<Vec<(usize, io::Result<HashMap<String, (bool, i64, u64)>>)>>> =
        Arc::new(Mutex::new(Vec::new()));

    thread::scope(|s| {
        for (i, transport) in transports.iter().enumerate() {
            let results = Arc::clone(&results);
            let transport = *transport;
            s.spawn(move || {
                let entries = walk_peer_tree(transport);
                results.lock().unwrap().push((i, entries));
            });
        }
    });

    let mut results = results.lock().unwrap();
    results.sort_by_key(|(i, _)| *i);

    let mut all_entries = Vec::new();
    for (_i, result) in results.drain(..) {
        all_entries.push(result?);
    }

    Ok(all_entries)
}

fn walk_peer_tree(
    transport: &dyn Transport,
) -> io::Result<HashMap<String, (bool, i64, u64)>> {
    let mut entries = HashMap::new();
    let default_rules = IgnoreRules::default_rules();
    let mut dirs_to_visit: Vec<(String, IgnoreRules)> = vec![(String::new(), default_rules)];

    while let Some((dir, parent_rules)) = dirs_to_visit.pop() {
        let listing = transport.list_dir(if dir.is_empty() { "." } else { &dir })?;

        // REQ_IGN_003: Resolve .syncignore before other entries
        let current_rules = if listing.iter().any(|e| e.name == ".syncignore") {
            let syncignore_path = if dir.is_empty() {
                ".syncignore".to_string()
            } else {
                format!("{}/.syncignore", dir)
            };
            match transport.read_file(&syncignore_path) {
                Ok(data) => {
                    if let Ok(content) = String::from_utf8(data) {
                        parent_rules.with_syncignore(if dir.is_empty() { "" } else { &dir }, &content)
                    } else {
                        // REQ_IGN_011: read failure -> warn and use parent rules
                        log::warn!("Failed to parse .syncignore at {}: not valid UTF-8", syncignore_path);
                        parent_rules
                    }
                }
                Err(e) => {
                    // REQ_IGN_011: read failure -> warn and use parent rules
                    log::warn!("Failed to read .syncignore at {}: {}", syncignore_path, e);
                    parent_rules
                }
            }
        } else {
            // REQ_IGN_004: No .syncignore at this level, only parent rules apply
            parent_rules
        };

        let gitignore = current_rules.build();

        for entry in listing {
            let rel_path = if dir.is_empty() {
                entry.name.clone()
            } else {
                format!("{}/{}", dir, entry.name)
            };

            // .syncignore is always included (REQ_IGN_002: synced like any other file)
            // Other entries are filtered through ignore rules
            if entry.name != ".syncignore"
                && current_rules.is_ignored(&gitignore, &rel_path, entry.is_dir)
            {
                continue;
            }

            entries.insert(rel_path.clone(), (entry.is_dir, entry.mod_time, entry.size));

            if entry.is_dir {
                dirs_to_visit.push((rel_path, current_rules.clone()));
            }
        }
    }

    Ok(entries)
}

fn execute_actions(
    actions: &[SyncAction],
    peers: &[ConnectedPeer],
) -> io::Result<()> {
    // Track which paths have been logged (REQ_SYNCOP_021/022: once per decision, not per peer)
    let mut copy_logged: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut del_logged: std::collections::HashSet<String> = std::collections::HashSet::new();

    for action in actions {
        match action {
            SyncAction::CreateDir {
                rel_path,
                target_peer,
            } => {
                debug!("  mkdir {} on peer {}", rel_path, target_peer);
                peers[*target_peer].transport.mkdir(rel_path)?;
            }
            SyncAction::CopyFile {
                rel_path,
                source_peer,
                target_peer,
                mod_time,
                ..
            } => {
                if copy_logged.insert(rel_path.clone()) {
                    info!("C {}", rel_path);
                }

                // Acquire connections from both pools (REQ_CONC_005)
                peers[*source_peer].connection_pool.acquire();
                peers[*target_peer].connection_pool.acquire();

                // Pipelined transfer: reader task -> bounded channel -> writer task (REQ_CONC_020)
                let (tx, rx) = sync_channel::<Vec<u8>>(4);

                let src_transport = &peers[*source_peer].transport;
                let dst_transport = &peers[*target_peer].transport;
                let rel_path_clone = rel_path.clone();
                let mod_time_val = *mod_time;

                // Back up existing file if present
                if let Ok(Some(_)) = dst_transport.stat(rel_path) {
                    let _ = backup::backup_file(dst_transport.as_ref(), rel_path);
                }

                // Use thread::scope for pipelined read/write
                let result: io::Result<()> = thread::scope(|s| {
                    let reader = s.spawn(|| -> io::Result<()> {
                        let data = src_transport.read_file(&rel_path_clone)?;
                        tx.send(data).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                        Ok(())
                    });

                    let rel_path_w = rel_path.clone();
                    let writer = s.spawn(move || -> io::Result<()> {
                        let data = rx.recv().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                        staging::write_via_staging(dst_transport.as_ref(), &rel_path_w, &data, mod_time_val)?;
                        Ok(())
                    });

                    reader.join().map_err(|_| io::Error::new(io::ErrorKind::Other, "reader thread panicked"))??;
                    writer.join().map_err(|_| io::Error::new(io::ErrorKind::Other, "writer thread panicked"))??;
                    Ok(())
                });

                // Release connections back to pools (REQ_CONC_006)
                peers[*source_peer].connection_pool.release();
                peers[*target_peer].connection_pool.release();

                result?;
            }
            SyncAction::DeleteFile {
                rel_path,
                target_peer,
            } => {
                if del_logged.insert(rel_path.clone()) {
                    info!("X {}", rel_path);
                }
                let _ = backup::backup_file(peers[*target_peer].transport.as_ref(), rel_path);
            }
            SyncAction::RemoveDir {
                rel_path,
                target_peer,
            } => {
                if del_logged.insert(rel_path.clone()) {
                    info!("X {}", rel_path);
                }
                let _ = backup::backup_file(peers[*target_peer].transport.as_ref(), rel_path);
            }
        }
    }

    Ok(())
}

fn update_snapshots(
    peers: &[ConnectedPeer],
    peer_entries: &[HashMap<String, (bool, i64, u64)>],
    actions: &[SyncAction],
    now: i64,
) -> io::Result<()> {
    for (i, peer) in peers.iter().enumerate() {
        // REQ_MTS_032: Confirmed present entries get last_seen = now
        for (rel_path, (is_dir, mod_time, size)) in &peer_entries[i] {
            peer.snapshot.upsert(rel_path, *is_dir, *mod_time, *size, now)?;
        }

        // REQ_MTS_033: Entries in snapshot that are absent from disk → mark deleted
        let snap_entries = peer.snapshot.all_entries()?;
        for (rel_path, entry) in &snap_entries {
            if entry.deleted_at == 0 && !peer_entries[i].contains_key(rel_path) {
                peer.snapshot.mark_deleted(rel_path)?;
            }
        }

        for action in actions {
            match action {
                SyncAction::CopyFile {
                    rel_path,
                    target_peer,
                    mod_time,
                    size,
                    ..
                } if *target_peer == i => {
                    // REQ_MTS_035: Push to peer (don't set last_seen yet)
                    peer.snapshot.upsert_push(rel_path, false, *mod_time, *size)?;
                }
                SyncAction::CreateDir {
                    rel_path,
                    target_peer,
                } if *target_peer == i => {
                    // REQ_MTS_037: Dir creation → set last_seen
                    peer.snapshot.upsert(rel_path, true, now, 0, now)?;
                }
                SyncAction::DeleteFile {
                    rel_path,
                    target_peer,
                } if *target_peer == i => {
                    peer.snapshot.mark_deleted(rel_path)?;
                }
                SyncAction::RemoveDir {
                    rel_path,
                    target_peer,
                } if *target_peer == i => {
                    // REQ_MTS_038: Cascade deletion to descendants
                    peer.snapshot.cascade_delete(rel_path)?;
                    peer.snapshot.mark_deleted(rel_path)?;
                }
                _ => {}
            }
        }

    }

    Ok(())
}

/// After copies complete, update last_seen on destination peers (REQ_MTS_036).
fn finalize_copies(
    peers: &[ConnectedPeer],
    actions: &[SyncAction],
    now: i64,
) -> io::Result<()> {
    for action in actions {
        if let SyncAction::CopyFile { rel_path, target_peer, .. } = action {
            peers[*target_peer].snapshot.set_last_seen(rel_path, now)?;
        }
    }
    Ok(())
}

fn upload_snapshots(peers: &[ConnectedPeer]) -> io::Result<()> {
    for peer in peers {
        let data = peer.snapshot.to_bytes()?;

        // REQ_SYNCOP_004: Upload as snapshot-new.db, then atomic rename to snapshot.db
        peer.transport.mkdir(".kitchensync")?;
        peer.transport.write_file(".kitchensync/snapshot-new.db", &data)?;
        peer.transport.rename(".kitchensync/snapshot-new.db", ".kitchensync/snapshot.db")?;
    }

    Ok(())
}
