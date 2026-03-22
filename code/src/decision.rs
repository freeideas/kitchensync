use crate::peer::PeerRole;
use crate::snapshot::SnapshotEntry;
use std::collections::{HashMap, HashSet};

/// Timestamp tolerance in seconds (REQ_MTS_026).
const TOLERANCE: i64 = 5;

/// What we know about one entry on one peer.
#[derive(Debug, Clone)]
pub enum PeerState {
    /// File/dir exists on this peer right now.
    Exists {
        is_dir: bool,
        mod_time: i64,
        size: u64,
    },
    /// File/dir was in the snapshot but is now missing (deleted by the user).
    /// `deletion_estimate` is the best guess of when deletion happened.
    Deleted {
        /// The mod_time the file had in the snapshot (last known alive).
        last_mod_time: i64,
        /// Estimated deletion time (snapshot mod_time used as proxy).
        deletion_estimate: i64,
    },
    /// Absent-unconfirmed: file was in snapshot (deleted_at=0) but missing from disk.
    /// Could be a user deletion or an incomplete copy.
    AbsentUnconfirmed {
        /// mod_time from the snapshot row.
        snap_mod_time: i64,
        /// last_seen from the snapshot row (when confirmed present).
        last_seen: i64,
    },
    /// Never existed on this peer (not in snapshot, not on disk).
    Unknown,
}

/// The decided authoritative state for an entry.
#[derive(Debug, Clone)]
pub enum AuthoritativeState {
    /// The entry should exist with these attributes.
    Exists {
        is_dir: bool,
        mod_time: i64,
        size: u64,
        /// Index of the peer that has the authoritative copy.
        source_peer: usize,
    },
    /// The entry should be deleted.
    Deleted,
}

/// An action to bring a peer in line with the authoritative state.
#[derive(Debug, Clone)]
pub enum SyncAction {
    /// Copy file from source_peer to this peer.
    CopyFile {
        rel_path: String,
        source_peer: usize,
        target_peer: usize,
        mod_time: i64,
        size: u64,
    },
    /// Create directory on this peer.
    CreateDir {
        rel_path: String,
        target_peer: usize,
    },
    /// Delete file from this peer (after backing up).
    DeleteFile {
        rel_path: String,
        target_peer: usize,
    },
    /// Remove directory from this peer.
    RemoveDir {
        rel_path: String,
        target_peer: usize,
    },
}

/// Determine the authoritative state for a single entry across all peers.
pub fn decide(
    _rel_path: &str,
    peer_states: &[(PeerState, PeerRole)],
) -> AuthoritativeState {
    // If any canon peer has a definitive state, use it
    for (i, (state, role)) in peer_states.iter().enumerate() {
        if *role == PeerRole::Canon {
            match state {
                PeerState::Exists {
                    is_dir,
                    mod_time,
                    size,
                } => {
                    return AuthoritativeState::Exists {
                        is_dir: *is_dir,
                        mod_time: *mod_time,
                        size: *size,
                        source_peer: i,
                    };
                }
                PeerState::Deleted { .. } => {
                    return AuthoritativeState::Deleted;
                }
                PeerState::AbsentUnconfirmed { .. } | PeerState::Unknown => {
                    // Canon peer doesn't know about this file — treat as delete
                    return AuthoritativeState::Deleted;
                }
            }
        }
    }

    // No canon peer. Find the newest state among non-subordinate peers.
    // Subordinate peers don't influence decisions (REQ_MTS_004).
    let mut best_exists: Option<(i64, usize, bool, u64)> = None; // (mod_time, peer_idx, is_dir, size)
    let mut has_file = false;
    let mut has_dir = false;
    let mut any_deletion = false;
    let mut newest_delete_estimate: i64 = 0;

    for (i, (state, role)) in peer_states.iter().enumerate() {
        if *role == PeerRole::Subordinate {
            continue;
        }
        match state {
            PeerState::Exists {
                is_dir,
                mod_time,
                size,
            } => {
                if *is_dir { has_dir = true; } else { has_file = true; }
                let dominated = if let Some((best_mt, _, best_is_dir, best_sz)) = best_exists {
                    if has_file && has_dir {
                        // REQ_MTS_030: type conflict without canon → file wins
                        *is_dir && !best_is_dir
                    } else if (*mod_time - best_mt).abs() <= TOLERANCE {
                        // Within tolerance: larger file wins (REQ_MTS_023, REQ_MTS_026)
                        *size <= best_sz
                    } else {
                        *mod_time < best_mt
                    }
                } else {
                    false
                };
                if !dominated {
                    best_exists = Some((*mod_time, i, *is_dir, *size));
                }
            }
            PeerState::Deleted { deletion_estimate, .. } => {
                any_deletion = true;
                if *deletion_estimate > newest_delete_estimate {
                    newest_delete_estimate = *deletion_estimate;
                }
            }
            PeerState::AbsentUnconfirmed { .. } => {
                // REQ_MTS_022: Will resolve after we know max existing mod_time
                // Collected but not counted as deletion yet
            }
            PeerState::Unknown => {}
        }
    }

    // REQ_MTS_022: Resolve absent-unconfirmed peers.
    // If last_seen > max_mod_time of existing peers (by > tolerance): real deletion
    // If last_seen <= max_mod_time (or is 0): failed copy → re-enqueue
    let max_existing_mod_time = best_exists.map(|(mt, _, _, _)| mt);
    for (_i, (state, role)) in peer_states.iter().enumerate() {
        if *role == PeerRole::Subordinate {
            continue;
        }
        if let PeerState::AbsentUnconfirmed { last_seen, .. } = state {
            if *last_seen == 0 {
                // Never confirmed present → failed copy, not a deletion
                continue;
            }
            if let Some(max_mt) = max_existing_mod_time {
                if *last_seen > max_mt + TOLERANCE {
                    // last_seen > max existing mod_time: real deletion
                    any_deletion = true;
                    if *last_seen > newest_delete_estimate {
                        newest_delete_estimate = *last_seen;
                    }
                }
                // else: failed copy, not a deletion
            } else {
                // No existing peer has it — treat as deletion
                any_deletion = true;
                if *last_seen > newest_delete_estimate {
                    newest_delete_estimate = *last_seen;
                }
            }
        }
    }

    match (best_exists, any_deletion) {
        (Some((mod_time, idx, is_dir, size)), true) => {
            // Conflict: some peers have it, some deleted.
            // REQ_MTS_021: deletion estimate > mod_time → deletion wins
            // REQ_MTS_024: ties favor keeping data
            if newest_delete_estimate > mod_time + TOLERANCE {
                AuthoritativeState::Deleted
            } else {
                AuthoritativeState::Exists {
                    is_dir,
                    mod_time,
                    size,
                    source_peer: idx,
                }
            }
        }
        (Some((mod_time, idx, is_dir, size)), false) => AuthoritativeState::Exists {
            is_dir,
            mod_time,
            size,
            source_peer: idx,
        },
        (None, _) => AuthoritativeState::Deleted,
    }
}

/// Given a set of peers with their current states and snapshots, compute all sync actions.
pub fn compute_actions(
    peer_roles: &[PeerRole],
    peer_entries: &[HashMap<String, (bool, i64, u64)>], // rel_path -> (is_dir, mod_time, size)
    peer_snapshots: &[HashMap<String, SnapshotEntry>],
    peer_has_history: &[bool],
) -> Vec<SyncAction> {
    let num_peers = peer_roles.len();

    // REQ_MTS_002: Only live peer listings drive traversal, not snapshots
    let mut all_paths: HashSet<String> = HashSet::new();
    for entries in peer_entries {
        for path in entries.keys() {
            all_paths.insert(path.clone());
        }
    }

    // Sort paths so directories come before their contents
    let mut sorted_paths: Vec<String> = all_paths.into_iter().collect();
    sorted_paths.sort();

    let mut actions = Vec::new();
    // REQ_MTS_040: Track displaced directories per peer to skip their children
    let mut displaced_dirs: Vec<Vec<String>> = vec![Vec::new(); num_peers];

    for rel_path in &sorted_paths {
        // Build peer states for this entry, skipping peers with displaced parent dirs
        let mut peer_states: Vec<(PeerState, PeerRole)> = Vec::new();

        for i in 0..num_peers {
            // REQ_MTS_040: If this path is under a displaced dir on this peer, treat as absent
            let under_displaced = displaced_dirs[i].iter().any(|d| {
                rel_path.starts_with(d) && rel_path.as_bytes().get(d.len()) == Some(&b'/')
            });

            let current = if under_displaced {
                None
            } else {
                peer_entries[i].get(rel_path)
            };
            let snap = peer_snapshots[i].get(rel_path);

            let state = match (current, snap) {
                (Some((is_dir, mod_time, size)), _) => PeerState::Exists {
                    is_dir: *is_dir,
                    mod_time: *mod_time,
                    size: *size,
                },
                (None, Some(snap_entry)) if snap_entry.deleted_at == 0 && !under_displaced => {
                    // Was in snapshot with no tombstone, now gone -> absent-unconfirmed
                    PeerState::AbsentUnconfirmed {
                        snap_mod_time: snap_entry.mod_time,
                        last_seen: snap_entry.last_seen,
                    }
                }
                (None, Some(snap_entry)) if snap_entry.deleted_at != 0 => {
                    // Tombstone set -> confirmed deletion (REQ_MTS_013)
                    PeerState::Deleted {
                        last_mod_time: snap_entry.mod_time,
                        deletion_estimate: snap_entry.deleted_at,
                    }
                }
                _ => PeerState::Unknown,
            };

            // A peer without any snapshot history is automatically subordinate
            let role = if !peer_has_history[i] && peer_roles[i] == PeerRole::Bidirectional {
                PeerRole::Subordinate
            } else {
                peer_roles[i]
            };

            peer_states.push((state, role));
        }

        let auth = decide(rel_path, &peer_states);

        match auth {
            AuthoritativeState::Exists {
                is_dir,
                mod_time,
                size,
                source_peer,
            } => {
                for i in 0..num_peers {
                    if i == source_peer {
                        continue;
                    }
                    match &peer_states[i].0 {
                        PeerState::Exists {
                            mod_time: mt,
                            size: sz,
                            is_dir: id,
                        } if *id == is_dir
                            && (mt - mod_time).abs() <= TOLERANCE
                            && *sz == size =>
                        {
                            // REQ_MTS_048: Already matches (within tolerance, same size) → skip
                        }
                        PeerState::Exists {
                            is_dir: id,
                            ..
                        } if *id != is_dir => {
                            // Type conflict on target: need to displace first
                            if *id {
                                // Target has dir, auth is file → displace dir, copy file
                                displaced_dirs[i].push(rel_path.clone());
                                actions.push(SyncAction::RemoveDir {
                                    rel_path: rel_path.clone(),
                                    target_peer: i,
                                });
                            } else {
                                // Target has file, auth is dir → displace file, create dir
                                actions.push(SyncAction::DeleteFile {
                                    rel_path: rel_path.clone(),
                                    target_peer: i,
                                });
                            }
                            if is_dir {
                                actions.push(SyncAction::CreateDir {
                                    rel_path: rel_path.clone(),
                                    target_peer: i,
                                });
                            } else {
                                actions.push(SyncAction::CopyFile {
                                    rel_path: rel_path.clone(),
                                    source_peer,
                                    target_peer: i,
                                    mod_time,
                                    size,
                                });
                            }
                        }
                        _ => {
                            if is_dir {
                                actions.push(SyncAction::CreateDir {
                                    rel_path: rel_path.clone(),
                                    target_peer: i,
                                });
                            } else {
                                actions.push(SyncAction::CopyFile {
                                    rel_path: rel_path.clone(),
                                    source_peer,
                                    target_peer: i,
                                    mod_time,
                                    size,
                                });
                            }
                        }
                    }
                }
            }
            AuthoritativeState::Deleted => {
                for i in 0..num_peers {
                    if let PeerState::Exists { is_dir, .. } = &peer_states[i].0 {
                        if *is_dir {
                            displaced_dirs[i].push(rel_path.clone());
                            actions.push(SyncAction::RemoveDir {
                                rel_path: rel_path.clone(),
                                target_peer: i,
                            });
                        } else {
                            actions.push(SyncAction::DeleteFile {
                                rel_path: rel_path.clone(),
                                target_peer: i,
                            });
                        }
                    }
                }
            }
        }
    }

    // Sort: directories created first, files deleted before dirs removed
    // Dir creates sorted by depth (shallowest first)
    // Dir removes sorted by depth (deepest first)
    actions.sort_by(|a, b| action_priority(a).cmp(&action_priority(b)));
    actions
}

fn action_priority(action: &SyncAction) -> (u8, isize) {
    match action {
        // Displacements first (deepest dirs first), then creates/copies
        SyncAction::DeleteFile { .. } => (0, 0),
        SyncAction::RemoveDir { rel_path, .. } => (1, -(path_depth(rel_path))),
        SyncAction::CreateDir { rel_path, .. } => (2, path_depth(rel_path)),
        SyncAction::CopyFile { .. } => (3, 0),
    }
}

fn path_depth(path: &str) -> isize {
    path.chars().filter(|c| *c == '/').count() as isize
}
