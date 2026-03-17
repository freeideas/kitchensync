use crate::database::{LocalDatabase, PeerDatabase, SnapshotEntry};
use crate::timestamp;

/// Reconciliation decision for a path.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    NoAction,
    PushFile,
    PullFile,
    PushDelete,
    PullDelete,
    CreateDirOnPeer,
    CreateDirLocally,
    DeleteDirOnPeer,
}

/// Determine what action to take for a path.
pub fn decide_action(
    local_entry: Option<&SnapshotEntry>,
    peer_entry: Option<&SnapshotEntry>,
) -> Action {
    match (local_entry, peer_entry) {
        // Both Unknown - should not happen, but handle gracefully
        (None, None) => Action::NoAction,

        // Local Unknown, Peer Live -> Pull
        (None, Some(peer)) if !peer.is_deleted() => {
            if peer.is_dir() {
                Action::CreateDirLocally
            } else {
                Action::PullFile
            }
        }

        // Local Unknown, Peer Deleted -> No action
        (None, Some(_)) => Action::NoAction,

        // Local Live, Peer Unknown -> Push
        (Some(local), None) if !local.is_deleted() => {
            if local.is_dir() {
                Action::CreateDirOnPeer
            } else {
                Action::PushFile
            }
        }

        // Local Deleted, Peer Unknown -> No action
        (Some(_), None) => Action::NoAction,

        // Both exist
        (Some(local), Some(peer)) => {
            let local_deleted = local.is_deleted();
            let peer_deleted = peer.is_deleted();
            let local_is_dir = local.is_dir();
            let peer_is_dir = peer.is_dir();

            // Handle directories
            if local_is_dir || peer_is_dir {
                return decide_dir_action(local, peer);
            }

            match (local_deleted, peer_deleted) {
                // Both deleted -> No action
                (true, true) => Action::NoAction,

                // Local live, Peer deleted
                (false, true) => {
                    let peer_del = peer.del_time.as_ref().unwrap();
                    let local_mod = local.mod_time.as_ref().map(|s| s.as_str()).unwrap_or("");

                    // Compare mod_time vs del_time
                    match timestamp::compare_timestamps(local_mod, peer_del) {
                        1 => Action::PushFile, // Local newer, push
                        -1 => Action::PullDelete, // Deletion newer, propagate
                        0 => Action::PushFile, // Tie goes to keeping data
                    }
                }

                // Local deleted, Peer live
                (true, false) => {
                    let local_del = local.del_time.as_ref().unwrap();
                    let peer_mod = peer.mod_time.as_ref().map(|s| s.as_str()).unwrap_or("");

                    // Compare del_time vs mod_time
                    match timestamp::compare_timestamps(local_del, peer_mod) {
                        1 => Action::PushDelete, // Deletion newer, propagate
                        -1 => Action::PullFile, // Peer newer, pull
                        0 => Action::PullFile, // Tie goes to keeping data
                    }
                }

                // Both live
                (false, false) => {
                    let local_mod = local.mod_time.as_ref().map(|s| s.as_str()).unwrap_or("");
                    let peer_mod = peer.mod_time.as_ref().map(|s| s.as_str()).unwrap_or("");

                    match timestamp::compare_timestamps(local_mod, peer_mod) {
                        1 => Action::PushFile, // Local newer
                        -1 => Action::PullFile, // Peer newer
                        0 => {
                            // Same time - compare sizes
                            let local_size = local.byte_size.unwrap_or(0);
                            let peer_size = peer.byte_size.unwrap_or(0);
                            if local_size > peer_size {
                                Action::PushFile // Larger wins
                            } else if peer_size > local_size {
                                Action::PullFile // Larger wins
                            } else {
                                Action::NoAction // Same size too
                            }
                        }
                    }
                }
            }
        }
    }
}

fn decide_dir_action(local: &SnapshotEntry, peer: &SnapshotEntry) -> Action {
    let local_deleted = local.is_deleted();
    let peer_deleted = peer.is_deleted();
    let local_is_dir = local.is_dir();
    let peer_is_dir = peer.is_dir();

    // Mismatch handling: treat file as dominant
    if local_is_dir != peer_is_dir {
        if local_is_dir {
            // Local is dir, peer is file
            if peer_deleted {
                Action::CreateDirOnPeer
            } else {
                Action::NoAction // File takes precedence, complex case
            }
        } else {
            // Local is file, peer is dir
            if local_deleted {
                Action::CreateDirLocally
            } else {
                Action::NoAction // File takes precedence
            }
        }
    } else {
        // Both directories
        match (local_deleted, peer_deleted) {
            (false, false) => Action::NoAction,
            (false, true) => Action::CreateDirOnPeer,
            (true, false) => Action::DeleteDirOnPeer,
            (true, true) => Action::NoAction,
        }
    }
}
