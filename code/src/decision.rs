use crate::database::SnapshotEntry;
use crate::peer::DirEntry;
use crate::timestamp;

const TIMESTAMP_TOLERANCE_SECS: i64 = 5;

#[derive(Debug, Clone)]
pub struct PeerState {
    pub peer_name: String,
    pub entry: Option<DirEntry>, // None = absent on this peer
}

#[derive(Debug, Clone)]
pub enum DecisionType {
    NoAction,
    File,
    Directory,
    Delete,
    RemoveTombstone,
}

#[derive(Debug, Clone)]
pub struct Decision {
    pub decision_type: DecisionType,
    pub src_peer: Option<String>,
    pub mod_time: String,
    pub byte_size: i64,
    pub is_dir: bool,
    pub peers_needing_copy: Vec<String>,
    pub peers_needing_delete: Vec<String>,
}

#[derive(Debug, PartialEq)]
enum Classification {
    Unchanged,
    Modified,
    New,
    Deleted,
    Absent,
}

fn classify(peer_entry: &Option<DirEntry>, snap: &Option<&SnapshotEntry>) -> Classification {
    match (peer_entry, snap) {
        (Some(entry), Some(snap_entry)) => {
            if snap_entry.del_time.is_some() {
                Classification::New
            } else if timestamp::is_within_tolerance(&entry.mod_time, &snap_entry.mod_time, TIMESTAMP_TOLERANCE_SECS) {
                Classification::Unchanged
            } else {
                Classification::Modified
            }
        }
        (Some(_), None) => Classification::New,
        (None, Some(snap_entry)) => {
            if snap_entry.del_time.is_some() {
                Classification::Absent
            } else {
                Classification::Deleted
            }
        }
        (None, None) => Classification::Absent,
    }
}

pub fn decide(
    states: &[PeerState],
    snap: Option<&SnapshotEntry>,
    canon_peer: Option<&str>,
) -> Decision {
    if let Some(canon) = canon_peer {
        return decide_canon(states, canon);
    }
    decide_normal(states, snap)
}

fn decide_canon(states: &[PeerState], canon: &str) -> Decision {
    let canon_state = states.iter().find(|s| s.peer_name == canon);
    let canon_entry = canon_state.and_then(|s| s.entry.as_ref());

    match canon_entry {
        Some(entry) => {
            let peers_needing_copy: Vec<String> = states
                .iter()
                .filter(|s| s.peer_name != canon)
                .filter(|s| {
                    match &s.entry {
                        None => true,
                        Some(e) => {
                            e.is_dir != entry.is_dir
                                || !timestamp::is_within_tolerance(&e.mod_time, &entry.mod_time, TIMESTAMP_TOLERANCE_SECS)
                                || e.byte_size != entry.byte_size
                        }
                    }
                })
                .map(|s| s.peer_name.clone())
                .collect();

            let peers_needing_delete: Vec<String> = states
                .iter()
                .filter(|s| s.peer_name != canon)
                .filter(|s| {
                    s.entry.as_ref().map_or(false, |e| e.is_dir != entry.is_dir)
                })
                .map(|s| s.peer_name.clone())
                .collect();

            Decision {
                decision_type: if entry.is_dir {
                    DecisionType::Directory
                } else {
                    DecisionType::File
                },
                src_peer: Some(canon.to_string()),
                mod_time: entry.mod_time.clone(),
                byte_size: entry.byte_size,
                is_dir: entry.is_dir,
                peers_needing_copy,
                peers_needing_delete,
            }
        }
        None => {
            // Canon lacks file → delete everywhere
            let peers_needing_delete: Vec<String> = states
                .iter()
                .filter(|s| s.entry.is_some())
                .map(|s| s.peer_name.clone())
                .collect();

            Decision {
                decision_type: DecisionType::Delete,
                src_peer: None,
                mod_time: String::new(),
                byte_size: 0,
                is_dir: false,
                peers_needing_copy: Vec::new(),
                peers_needing_delete,
            }
        }
    }
}

fn decide_normal(states: &[PeerState], snap: Option<&SnapshotEntry>) -> Decision {
    let classifications: Vec<(&PeerState, Classification)> = states
        .iter()
        .map(|s| (s, classify(&s.entry, &snap)))
        .collect();

    let has_modified = classifications.iter().any(|(_, c)| *c == Classification::Modified);
    let has_new = classifications.iter().any(|(_, c)| *c == Classification::New);
    let has_deleted = classifications.iter().any(|(_, c)| *c == Classification::Deleted);
    let all_unchanged = classifications.iter().all(|(_, c)| *c == Classification::Unchanged || *c == Classification::Absent);
    let all_absent = classifications.iter().all(|(_, c)| *c == Classification::Absent);

    // Tombstone in snapshot, absent everywhere → remove tombstone
    if all_absent {
        return Decision {
            decision_type: DecisionType::RemoveTombstone,
            src_peer: None,
            mod_time: String::new(),
            byte_size: 0,
            is_dir: false,
            peers_needing_copy: Vec::new(),
            peers_needing_delete: Vec::new(),
        };
    }

    // All unchanged → no action
    if all_unchanged {
        return Decision {
            decision_type: DecisionType::NoAction,
            src_peer: None,
            mod_time: snap.map(|s| s.mod_time.clone()).unwrap_or_default(),
            byte_size: snap.and_then(|s| s.byte_size).unwrap_or(0),
            is_dir: false,
            peers_needing_copy: Vec::new(),
            peers_needing_delete: Vec::new(),
        };
    }

    // Find the winner among live entries
    let live_entries: Vec<&PeerState> = states
        .iter()
        .filter(|s| s.entry.is_some())
        .collect();

    if live_entries.is_empty() {
        // All deleted
        return Decision {
            decision_type: DecisionType::Delete,
            src_peer: None,
            mod_time: String::new(),
            byte_size: 0,
            is_dir: false,
            peers_needing_copy: Vec::new(),
            peers_needing_delete: Vec::new(),
        };
    }

    // Deleted + modified → modification wins
    // Deleted + unchanged → deletion wins
    if has_deleted && !has_modified && !has_new {
        // Only unchanged and deleted → deletion wins
        let peers_needing_delete: Vec<String> = states
            .iter()
            .filter(|s| s.entry.is_some())
            .map(|s| s.peer_name.clone())
            .collect();

        return Decision {
            decision_type: DecisionType::Delete,
            src_peer: None,
            mod_time: String::new(),
            byte_size: 0,
            is_dir: false,
            peers_needing_copy: Vec::new(),
            peers_needing_delete,
        };
    }

    // Find newest entry (winner)
    let winner = find_winner(&live_entries);
    let winner_entry = winner.entry.as_ref().unwrap();

    let peers_needing_copy: Vec<String> = states
        .iter()
        .filter(|s| s.peer_name != winner.peer_name)
        .filter(|s| match &s.entry {
            None => true,
            Some(e) => {
                e.is_dir != winner_entry.is_dir
                    || !timestamp::is_within_tolerance(&e.mod_time, &winner_entry.mod_time, TIMESTAMP_TOLERANCE_SECS)
                    || e.byte_size != winner_entry.byte_size
            }
        })
        .map(|s| s.peer_name.clone())
        .collect();

    // Type conflicts: losers with different type need deletion
    let peers_needing_delete: Vec<String> = states
        .iter()
        .filter(|s| s.peer_name != winner.peer_name)
        .filter(|s| {
            s.entry
                .as_ref()
                .map_or(false, |e| e.is_dir != winner_entry.is_dir)
        })
        .map(|s| s.peer_name.clone())
        .collect();

    Decision {
        decision_type: if winner_entry.is_dir {
            DecisionType::Directory
        } else {
            DecisionType::File
        },
        src_peer: Some(winner.peer_name.clone()),
        mod_time: winner_entry.mod_time.clone(),
        byte_size: winner_entry.byte_size,
        is_dir: winner_entry.is_dir,
        peers_needing_copy,
        peers_needing_delete,
    }
}

fn find_winner<'a>(live: &[&'a PeerState]) -> &'a PeerState {
    let mut best = live[0];
    for state in &live[1..] {
        let best_entry = best.entry.as_ref().unwrap();
        let curr_entry = state.entry.as_ref().unwrap();

        if timestamp::is_within_tolerance(&best_entry.mod_time, &curr_entry.mod_time, TIMESTAMP_TOLERANCE_SECS) {
            // Same mod_time within tolerance → larger file wins (rule 6)
            // Directories have byte_size -1, so files win over dirs (type conflict rule)
            if curr_entry.byte_size > best_entry.byte_size {
                best = state;
            }
        } else {
            // Newer mod_time wins
            if curr_entry.mod_time > best_entry.mod_time {
                best = state;
            }
        }
    }
    best
}
