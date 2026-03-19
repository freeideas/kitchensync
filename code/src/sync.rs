use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use rusqlite::Connection;

use crate::database;
use crate::decision::{self, DecisionType, PeerState};
use crate::hash;
use crate::ignore::IgnoreRules;
use crate::peer::{DirEntry, Peer};
use crate::timestamp;
use crate::worker::{CopyJob, WorkerPool};

pub fn run_sync(
    conn: &Connection,
    peers: &Arc<HashMap<String, Box<dyn Peer>>>,
    pool: &WorkerPool,
    canon_peer: Option<&str>,
) {
    sync_directory(conn, peers, pool, canon_peer, "", &IgnoreRules::default_rules());
}

fn sync_directory(
    conn: &Connection,
    peers: &Arc<HashMap<String, Box<dyn Peer>>>,
    pool: &WorkerPool,
    canon_peer: Option<&str>,
    path: &str,
    parent_ignore: &IgnoreRules,
) {
    // Phase 1: List all peers in parallel
    let peer_names: Vec<String> = peers.keys().cloned().collect();
    let listings: HashMap<String, Vec<DirEntry>> = {
        let mut map = HashMap::new();
        // Use threads for parallel listing
        let handles: Vec<_> = peer_names
            .iter()
            .map(|name| {
                let name = name.clone();
                let peer = peers.get(&name).unwrap();
                let list_path = if path.is_empty() { ".".to_string() } else { path.to_string() };
                let result = peer.list_dir(&list_path);
                (name, result)
            })
            .collect();

        for (name, result) in handles {
            match result {
                Ok(entries) => {
                    map.insert(name, entries);
                }
                Err(e) => {
                    eprintln!("Warning: cannot list {}:{}: {}", name, path, e);
                    map.insert(name, Vec::new());
                }
            }
        }
        map
    };

    // Phase 2: Union entry names from listings + snapshot children
    let parent_hash_path = if path.is_empty() {
        "/".to_string()
    } else {
        format!("{}/", path)
    };
    let parent_id = hash::path_id(&parent_hash_path);
    let snap_children = database::snapshot_children(conn, &parent_id);

    let mut all_names: HashSet<String> = HashSet::new();
    for entries in listings.values() {
        for entry in entries {
            all_names.insert(entry.name.clone());
        }
    }
    for snap_entry in &snap_children {
        all_names.insert(snap_entry.basename.clone());
    }

    // Filter built-in excludes
    all_names.retain(|name| !is_builtin_excluded(name));

    // Phase 2b: Resolve .syncignore before other entries
    let has_syncignore = all_names.contains(".syncignore");

    let active_ignore = if has_syncignore {
        // Decide and sync .syncignore like any other file
        let syncignore_states = gather_states(&peer_names, &listings, ".syncignore");
        let snap_path = hash::snapshot_path(path, ".syncignore", false);
        let snap_id = hash::path_id(&snap_path);
        let snap_entry = database::snapshot_lookup(conn, &snap_id);
        let decision = decision::decide(&syncignore_states, snap_entry.as_ref(), canon_peer);

        // If there's a winner, sync it and load its rules
        match decision.decision_type {
            DecisionType::File => {
                update_snapshot_from_decision(conn, path, ".syncignore", false, &decision);
                for dst in &decision.peers_needing_copy {
                    if let Some(src) = &decision.src_peer {
                        let file_path = if path.is_empty() {
                            ".syncignore".to_string()
                        } else {
                            format!("{}/{}", path, ".syncignore")
                        };
                        pool.enqueue(CopyJob {
                            src_peer_name: src.clone(),
                            src_path: file_path.clone(),
                            dst_peer_name: dst.clone(),
                            dst_path: file_path,
                        });
                    }
                }
                // Load winning .syncignore content
                if let Some(src_name) = &decision.src_peer {
                    if let Some(src_peer) = peers.get(src_name) {
                        let file_path = if path.is_empty() {
                            ".syncignore".to_string()
                        } else {
                            format!("{}/{}", path, ".syncignore")
                        };
                        if let Ok(mut reader) = src_peer.read_file(&file_path) {
                            let mut content = String::new();
                            if std::io::Read::read_to_string(&mut reader, &mut content).is_ok() {
                                IgnoreRules::from_content(&content, Some(parent_ignore))
                            } else {
                                parent_ignore.clone_rules()
                            }
                        } else {
                            parent_ignore.clone_rules()
                        }
                    } else {
                        parent_ignore.clone_rules()
                    }
                } else {
                    parent_ignore.clone_rules()
                }
            }
            _ => parent_ignore.clone_rules(),
        }
    } else {
        parent_ignore.clone_rules()
    };

    // Remove .syncignore and ignored names
    all_names.remove(".syncignore");
    all_names.retain(|name| {
        let is_dir = listings.values().any(|entries| {
            entries.iter().any(|e| e.name == *name && e.is_dir)
        });
        !active_ignore.is_ignored(name, is_dir)
    });

    // Phase 3: Decide and act on each entry
    let mut sorted_names: Vec<String> = all_names.into_iter().collect();
    sorted_names.sort();

    for name in &sorted_names {
        let states = gather_states(&peer_names, &listings, name);
        let snap_is_dir = snap_children.iter().any(|s| s.basename == *name && s.byte_size == Some(-1));
        let live_is_dir = states.iter().any(|s| s.entry.as_ref().map_or(false, |e| e.is_dir));
        let is_dir = live_is_dir || snap_is_dir;

        let snap_path = hash::snapshot_path(path, name, is_dir);
        let snap_id = hash::path_id(&snap_path);
        let snap_entry = database::snapshot_lookup(conn, &snap_id);

        let d = decision::decide(&states, snap_entry.as_ref(), canon_peer);

        match d.decision_type {
            DecisionType::NoAction => {
                // If this was a directory, still recurse for contents
                if is_dir && live_is_dir {
                    let child_path = if path.is_empty() {
                        name.clone()
                    } else {
                        format!("{}/{}", path, name)
                    };
                    sync_directory(conn, peers, pool, canon_peer, &child_path, &active_ignore);
                }
            }
            DecisionType::Directory => {
                // Create dirs where needed
                for peer_name in &d.peers_needing_delete {
                    if let Some(peer) = peers.get(peer_name) {
                        let file_path = if path.is_empty() {
                            name.clone()
                        } else {
                            format!("{}/{}", path, name)
                        };
                        displace_to_back(peer.as_ref(), &file_path);
                    }
                }
                for peer_name in &d.peers_needing_copy {
                    if let Some(peer) = peers.get(peer_name) {
                        let dir_path = if path.is_empty() {
                            name.clone()
                        } else {
                            format!("{}/{}", path, name)
                        };
                        peer.create_dir(&dir_path).ok();
                    }
                }

                update_snapshot_from_decision(conn, path, name, true, &d);

                // Recurse
                let child_path = if path.is_empty() {
                    name.clone()
                } else {
                    format!("{}/{}", path, name)
                };
                sync_directory(conn, peers, pool, canon_peer, &child_path, &active_ignore);
            }
            DecisionType::File => {
                update_snapshot_from_decision(conn, path, name, false, &d);

                // Displace type conflicts
                for peer_name in &d.peers_needing_delete {
                    if let Some(peer) = peers.get(peer_name) {
                        let file_path = if path.is_empty() {
                            name.clone()
                        } else {
                            format!("{}/{}", path, name)
                        };
                        displace_to_back(peer.as_ref(), &file_path);
                    }
                }

                // Enqueue copies
                for dst in &d.peers_needing_copy {
                    if let Some(src) = &d.src_peer {
                        let file_path = if path.is_empty() {
                            name.clone()
                        } else {
                            format!("{}/{}", path, name)
                        };
                        pool.enqueue(CopyJob {
                            src_peer_name: src.clone(),
                            src_path: file_path.clone(),
                            dst_peer_name: dst.clone(),
                            dst_path: file_path,
                        });
                    }
                }
            }
            DecisionType::Delete => {
                // Displace files/directories on peers that have them
                for peer_name in &d.peers_needing_delete {
                    if let Some(peer) = peers.get(peer_name) {
                        let file_path = if path.is_empty() {
                            name.clone()
                        } else {
                            format!("{}/{}", path, name)
                        };
                        displace_to_back(peer.as_ref(), &file_path);
                    }
                }

                // Set tombstone in snapshot
                let snap_path = hash::snapshot_path(path, name, is_dir);
                let snap_id = hash::path_id(&snap_path);
                let parent_hash = if path.is_empty() {
                    "/".to_string()
                } else {
                    format!("{}/", path)
                };
                let pid = hash::path_id(&parent_hash);
                let del_stamp = timestamp::now();
                database::snapshot_upsert(
                    conn,
                    &snap_id,
                    &pid,
                    name,
                    &snap_entry.map(|s| s.mod_time).unwrap_or_default(),
                    Some(if is_dir { -1 } else { 0 }),
                    Some(&del_stamp),
                )
                .ok();
            }
            DecisionType::RemoveTombstone => {
                // Remove orphaned tombstone from snapshot
                let snap_path = hash::snapshot_path(path, name, is_dir);
                let snap_id = hash::path_id(&snap_path);
                database::snapshot_delete(conn, &snap_id).ok();
            }
        }
    }
}

fn gather_states(
    peer_names: &[String],
    listings: &HashMap<String, Vec<DirEntry>>,
    name: &str,
) -> Vec<PeerState> {
    peer_names
        .iter()
        .map(|pn| {
            let entry = listings
                .get(pn)
                .and_then(|entries| entries.iter().find(|e| e.name == name))
                .cloned();
            PeerState {
                peer_name: pn.clone(),
                entry,
            }
        })
        .collect()
}

fn update_snapshot_from_decision(
    conn: &Connection,
    path: &str,
    name: &str,
    is_dir: bool,
    d: &decision::Decision,
) {
    let snap_path = hash::snapshot_path(path, name, is_dir);
    let snap_id = hash::path_id(&snap_path);
    let parent_hash = if path.is_empty() {
        "/".to_string()
    } else {
        format!("{}/", path)
    };
    let pid = hash::path_id(&parent_hash);

    database::snapshot_upsert(
        conn,
        &snap_id,
        &pid,
        name,
        &d.mod_time,
        Some(d.byte_size),
        None,
    )
    .ok();
}

fn displace_to_back(peer: &dyn Peer, file_path: &str) {
    let stamp = timestamp::now();
    let basename = std::path::Path::new(file_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.to_string());
    let parent = match file_path.rfind('/') {
        Some(pos) => &file_path[..pos],
        None => ".",
    };
    let back_path = format!("{}/.kitchensync/BACK/{}/{}", parent, stamp, basename);
    peer.rename(file_path, &back_path).ok();
}

fn is_builtin_excluded(name: &str) -> bool {
    name == ".kitchensync"
}

