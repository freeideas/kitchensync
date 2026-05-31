use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use kitchensync::snapshot;
use kitchensync::{
    EffectivePeerRole, EntryKind, EntryMeta, PeerId, PeerRole, PeerSession, PeerUrl, RelPath,
    Timestamp, TransportRootMode, TransportTimeouts,
};

#[path = "../sync/classify/mod.rs"]
#[allow(dead_code)]
mod classify;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_test_root(test_name: &str) -> PathBuf {
    let seq = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "kitchensync-classify-test-{}-{}",
        test_name.replace(['\\', '/'], "_"),
        seq
    ));

    if path.exists() {
        let _ = std::fs::remove_dir_all(&path);
    }

    path
}

fn make_peer_url(root: &Path) -> PeerUrl {
    let path = root.to_string_lossy().to_string();
    PeerUrl {
        scheme: "file".to_string(),
        username: None,
        password: None,
        host: None,
        port: None,
        path,
        identity: format!("file:///{}", root.to_string_lossy()),
        timeout_conn: None,
        timeout_idle: None,
    }
}

fn make_peer_session(id: PeerId, role: EffectivePeerRole) -> PeerSession {
    let root = next_test_root(&format!("peer-{id}"));
    let selected_url = make_peer_url(&root);
    let normalized_identity = make_peer_url(&root);
    let transport = kitchensync::transport::factory()
        .connect(
            &selected_url,
            TransportTimeouts {
                timeout_conn: 1,
                timeout_idle: 1,
            },
            TransportRootMode::CreateMissing,
        )
        .expect("transport connect succeeds for local test roots");

    let declared_role = match role {
        EffectivePeerRole::Canon => PeerRole::Canon,
        EffectivePeerRole::Contributing => PeerRole::Normal,
        EffectivePeerRole::Subordinate => PeerRole::Subordinate,
    };

    PeerSession {
        id,
        invocation_index: 0,
        normalized_identity,
        selected_url,
        declared_role,
        effective_role: role,
        transport,
        had_startup_snapshot: false,
    }
}

fn timestamp(value: &str) -> Timestamp {
    Timestamp(value.to_string())
}

fn snapshot_file_row(
    path: &str,
    modified: &str,
    size: i64,
    last_seen: Option<&str>,
    deleted_time: Option<&str>,
) -> snapshot::SnapshotRow {
    snapshot::SnapshotRow {
        path: RelPath::new(path).expect("candidate path is valid"),
        kind: snapshot::SnapshotEntryKind::File,
        mod_time: timestamp(modified),
        byte_size: size,
        last_seen: last_seen.map(timestamp),
        deleted_time: deleted_time.map(timestamp),
    }
}

fn snapshot_directory_row(
    path: &str,
    modified: &str,
    last_seen: Option<&str>,
) -> snapshot::SnapshotRow {
    snapshot::SnapshotRow {
        path: RelPath::new(path).expect("candidate path is valid"),
        kind: snapshot::SnapshotEntryKind::Directory,
        mod_time: timestamp(modified),
        byte_size: -1,
        last_seen: last_seen.map(timestamp),
        deleted_time: None,
    }
}

fn file_meta(name: &str, byte_size: i64, mod_time: &str) -> EntryMeta {
    EntryMeta {
        name: name.to_string(),
        kind: EntryKind::File,
        mod_time: timestamp(mod_time),
        byte_size,
    }
}

fn directory_meta(name: &str, mod_time: &str) -> EntryMeta {
    EntryMeta {
        name: name.to_string(),
        kind: EntryKind::Directory,
        mod_time: timestamp(mod_time),
        byte_size: -1,
    }
}

#[test]
fn classify_preserves_input_path_and_basename() {
    let path = RelPath::new("nested/CaseName.TXT").expect("valid relpath");
    let input = classify::ClassificationInput {
        path: path.clone(),
        basename: "TRANSPORT_CASE.txt".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(1, EffectivePeerRole::Contributing),
            live: Some(file_meta("CaseName.TXT", 5, "2025-01-01_00-00-00_000000Z")),
            snapshot: classify::SnapshotLookup::Missing,
        }],
    };

    let result = classify::classify_candidate(input).expect("classification succeeds");

    assert_eq!(result.path, path);
    assert_eq!(result.basename, "TRANSPORT_CASE.txt");
}

#[test]
fn classify_live_file_new_when_no_snapshot_exists() {
    let input = classify::ClassificationInput {
        path: RelPath::new("candidate.txt").expect("valid relpath"),
        basename: "candidate.txt".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(1000, EffectivePeerRole::Contributing),
            live: Some(file_meta(
                "candidate.txt",
                11,
                "2025-07-01_01-01-01_000000Z",
            )),
            snapshot: classify::SnapshotLookup::Missing,
        }],
    };

    let result = classify::classify_candidate(input).expect("classification succeeds");
    let state = &result.contributors[0].state;

    assert_eq!(result.summary.has_live_file, true);
    assert_eq!(
        *state,
        classify::ContributingState::LiveFile(classify::LiveFileObservation {
            meta: file_meta("candidate.txt", 11, "2025-07-01_01-01-01_000000Z"),
            snapshot: classify::LiveFileSnapshotState::New,
        })
    );
}

#[test]
fn classify_live_file_tracks_unchanged_with_matching_snapshot_within_tolerance() {
    let input = classify::ClassificationInput {
        path: RelPath::new("sync.txt").expect("valid relpath"),
        basename: "sync.txt".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(1001, EffectivePeerRole::Contributing),
            live: Some(file_meta("sync.txt", 12, "2025-07-01_01-01-04_000000Z")),
            snapshot: classify::SnapshotLookup::Present(snapshot_file_row(
                "sync.txt",
                "2025-07-01_01-01-00_000000Z",
                12,
                Some("2025-06-30_23-59-59_999000Z"),
                None,
            )),
        }],
    };

    let result = classify::classify_candidate(input).expect("classification succeeds");
    let state = &result.contributors[0].state;

    assert_eq!(
        *state,
        classify::ContributingState::LiveFile(classify::LiveFileObservation {
            meta: file_meta("sync.txt", 12, "2025-07-01_01-01-04_000000Z"),
            snapshot: classify::LiveFileSnapshotState::Unchanged {
                previous: classify::SnapshotFileFacts {
                    size: 12,
                    modified_time: timestamp("2025-07-01_01-01-00_000000Z"),
                    last_seen: timestamp("2025-06-30_23-59-59_999000Z"),
                },
            },
        })
    );
}

#[test]
fn classify_live_file_is_modified_when_snapshot_mismatch_exceeds_tolerance() {
    let input = classify::ClassificationInput {
        path: RelPath::new("sync.txt").expect("valid relpath"),
        basename: "sync.txt".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(1002, EffectivePeerRole::Contributing),
            live: Some(file_meta("sync.txt", 13, "2025-07-01_01-01-10_000000Z")),
            snapshot: classify::SnapshotLookup::Present(snapshot_file_row(
                "sync.txt",
                "2025-07-01_01-01-00_000000Z",
                12,
                Some("2025-06-30_23-59-59_998000Z"),
                None,
            )),
        }],
    };

    let result = classify::classify_candidate(input).expect("classification succeeds");
    let state = &result.contributors[0].state;

    assert!(matches!(
        state,
        classify::ContributingState::LiveFile(classify::LiveFileObservation {
            snapshot: classify::LiveFileSnapshotState::Modified {
                previous: classify::SnapshotKnownFacts::File(_),
            },
            ..
        })
    ));
}

#[test]
fn classify_live_file_reports_modified_when_snapshot_is_directory() {
    let input = classify::ClassificationInput {
        path: RelPath::new("changed-kind.txt").expect("valid relpath"),
        basename: "changed-kind.txt".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(1002, EffectivePeerRole::Contributing),
            live: Some(file_meta(
                "changed-kind.txt",
                1,
                "2025-07-01_01-01-10_000000Z",
            )),
            snapshot: classify::SnapshotLookup::Present(snapshot_directory_row(
                "changed-kind.txt",
                "2025-07-01_00-00-00_000000Z",
                Some("2025-06-30_23-59-59_000000Z"),
            )),
        }],
    };

    let result = classify::classify_candidate(input).expect("classification succeeds");
    let state = &result.contributors[0].state;

    assert!(matches!(
        state,
        classify::ContributingState::LiveFile(classify::LiveFileObservation {
            snapshot: classify::LiveFileSnapshotState::Modified {
                previous: classify::SnapshotKnownFacts::Directory(_),
            },
            ..
        })
    ));
}

#[test]
fn classify_live_file_reports_resurrected_from_tombstone_snapshot() {
    let input = classify::ClassificationInput {
        path: RelPath::new("resurrected.txt").expect("valid relpath"),
        basename: "resurrected.txt".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(1003, EffectivePeerRole::Contributing),
            live: Some(file_meta(
                "resurrected.txt",
                2,
                "2025-07-01_02-00-00_000000Z",
            )),
            snapshot: classify::SnapshotLookup::Present(snapshot_file_row(
                "resurrected.txt",
                "2025-06-01_01-00-00_000000Z",
                1,
                Some("2025-06-01_01-01-00_000000Z"),
                Some("2025-06-15_11-00-00_000000Z"),
            )),
        }],
    };

    let result = classify::classify_candidate(input).expect("classification succeeds");
    let state = &result.contributors[0].state;

    assert_eq!(
        state,
        &classify::ContributingState::LiveFile(classify::LiveFileObservation {
            meta: file_meta("resurrected.txt", 2, "2025-07-01_02-00-00_000000Z"),
            snapshot: classify::LiveFileSnapshotState::Resurrected {
                tombstone: classify::SnapshotTombstoneFacts {
                    previous_kind: Some(snapshot::SnapshotEntryKind::File),
                    deleted_time: timestamp("2025-06-15_11-00-00_000000Z"),
                    last_seen: Some(timestamp("2025-06-01_01-01-00_000000Z")),
                },
            },
        })
    );
}

#[test]
fn classify_live_directory_keeps_recent_timestamp_and_previous_snapshot_when_available() {
    let input = classify::ClassificationInput {
        path: RelPath::new("folder").expect("valid relpath"),
        basename: "folder".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(1004, EffectivePeerRole::Contributing),
            live: Some(directory_meta("folder", "2025-07-01_03-03-03_000000Z")),
            snapshot: classify::SnapshotLookup::Present(snapshot_directory_row(
                "folder",
                "2025-06-30_00-00-00_000000Z",
                Some("2025-06-30_00-00-10_000000Z"),
            )),
        }],
    };

    let result = classify::classify_candidate(input).expect("classification succeeds");
    let state = &result.contributors[0].state;

    assert_eq!(result.summary.has_live_directory, true);
    assert_eq!(
        state,
        &classify::ContributingState::LiveDirectory(classify::LiveDirectoryObservation {
            meta: directory_meta("folder", "2025-07-01_03-03-03_000000Z"),
            previous: Some(classify::SnapshotDirectoryFacts {
                modified_time: Some(timestamp("2025-06-30_00-00-00_000000Z")),
                last_seen: timestamp("2025-06-30_00-00-10_000000Z"),
            }),
        })
    );
}

#[test]
fn classify_live_directory_with_non_directory_snapshot_keeps_no_previous_snapshot_facts() {
    let input = classify::ClassificationInput {
        path: RelPath::new("folder").expect("valid relpath"),
        basename: "folder".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(1005, EffectivePeerRole::Contributing),
            live: Some(directory_meta("folder", "2025-07-01_03-03-03_000000Z")),
            snapshot: classify::SnapshotLookup::Present(snapshot_file_row(
                "folder",
                "2025-06-30_00-00-00_000000Z",
                1,
                Some("2025-06-30_00-00-10_000000Z"),
                None,
            )),
        }],
    };

    let result = classify::classify_candidate(input).expect("classification succeeds");

    assert!(matches!(
        result.contributors[0].state,
        classify::ContributingState::LiveDirectory(classify::LiveDirectoryObservation {
            previous: None,
            ..
        })
    ));
}

#[test]
fn classify_absent_contributing_peer_classifies_tombstone_as_deletion_vote() {
    let input = classify::ClassificationInput {
        path: RelPath::new("gone.txt").expect("valid relpath"),
        basename: "gone.txt".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(1006, EffectivePeerRole::Contributing),
            live: None,
            snapshot: classify::SnapshotLookup::Present(snapshot_file_row(
                "gone.txt",
                "2025-06-01_01-01-01_000000Z",
                9,
                Some("2025-06-01_01-01-02_000000Z"),
                Some("2025-07-01_00-00-00_000000Z"),
            )),
        }],
    };

    let result = classify::classify_candidate(input).expect("classification succeeds");

    assert_eq!(result.summary.has_deletion_vote, true);
    assert_eq!(
        result.contributors[0].state,
        classify::ContributingState::TombstoneDeletionVote(classify::TombstoneDeletionVote {
            deleted_time: timestamp("2025-07-01_00-00-00_000000Z"),
        })
    );
}

#[test]
fn classify_absent_contributing_peer_with_directory_history_is_directory_history() {
    let input = classify::ClassificationInput {
        path: RelPath::new("old-dir").expect("valid relpath"),
        basename: "old-dir".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(1007, EffectivePeerRole::Contributing),
            live: None,
            snapshot: classify::SnapshotLookup::Present(snapshot_directory_row(
                "old-dir",
                "2025-06-30_00-00-00_000000Z",
                Some("2025-06-30_00-00-10_000000Z"),
            )),
        }],
    };

    let result = classify::classify_candidate(input).expect("classification succeeds");

    assert_eq!(
        result.contributors[0].state,
        classify::ContributingState::AbsentDirectoryHistory(classify::AbsentDirectoryHistory {
            previous: classify::SnapshotDirectoryFacts {
                modified_time: Some(timestamp("2025-06-30_00-00-00_000000Z")),
                last_seen: timestamp("2025-06-30_00-00-10_000000Z"),
            },
        })
    );
    assert_eq!(result.summary.has_unconfirmed_absence, false);
    assert_eq!(result.summary.has_deletion_vote, false);
}

#[test]
fn classify_absent_contributing_peer_classifies_file_history_as_unconfirmed_absence() {
    let input = classify::ClassificationInput {
        path: RelPath::new("ghost.txt").expect("valid relpath"),
        basename: "ghost.txt".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(1008, EffectivePeerRole::Contributing),
            live: None,
            snapshot: classify::SnapshotLookup::Present(snapshot_file_row(
                "ghost.txt",
                "2025-06-01_01-01-01_000000Z",
                9,
                Some("2025-06-01_01-01-02_000000Z"),
                None,
            )),
        }],
    };

    let result = classify::classify_candidate(input).expect("classification succeeds");

    assert_eq!(result.summary.has_unconfirmed_absence, true);
    assert_eq!(
        result.contributors[0].state,
        classify::ContributingState::AbsentUnconfirmedFile(classify::AbsentUnconfirmedFile {
            previous: classify::SnapshotFileFacts {
                size: 9,
                modified_time: timestamp("2025-06-01_01-01-01_000000Z"),
                last_seen: timestamp("2025-06-01_01-01-02_000000Z"),
            },
        })
    );
}

#[test]
fn classify_absent_contributing_peer_without_snapshot_is_no_vote() {
    let input = classify::ClassificationInput {
        path: RelPath::new("new.txt").expect("valid relpath"),
        basename: "new.txt".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(1009, EffectivePeerRole::Contributing),
            live: None,
            snapshot: classify::SnapshotLookup::Missing,
        }],
    };

    let result = classify::classify_candidate(input).expect("classification succeeds");

    assert_eq!(
        result.contributors[0].state,
        classify::ContributingState::NoVote
    );
}

#[test]
fn classify_subordinate_peers_are_recorded_only_as_subordinate_targets() {
    let input = classify::ClassificationInput {
        path: RelPath::new("shared.txt").expect("valid relpath"),
        basename: "shared.txt".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(1010, EffectivePeerRole::Subordinate),
            live: Some(file_meta("shared.txt", 4, "2025-07-01_01-01-01_000000Z")),
            snapshot: classify::SnapshotLookup::Present(snapshot_file_row(
                "shared.txt",
                "2025-06-20_10-00-00_000000Z",
                3,
                Some("2025-06-20_10-00-01_000000Z"),
                None,
            )),
        }],
    };

    let result = classify::classify_candidate(input).expect("classification succeeds");

    assert!(result.contributors.is_empty());
    assert_eq!(result.subordinates.len(), 1);
    assert_eq!(result.summary, classify::ClassificationSummary::default());
    assert_eq!(
        result.subordinates[0],
        classify::SubordinateTarget {
            peer_id: 1010,
            live: Some(file_meta("shared.txt", 4, "2025-07-01_01-01-01_000000Z")),
            snapshot: Some(classify::SubordinateSnapshotFacts::File(
                classify::SnapshotFileFacts {
                    size: 3,
                    modified_time: timestamp("2025-06-20_10-00-00_000000Z"),
                    last_seen: timestamp("2025-06-20_10-00-01_000000Z"),
                }
            )),
        }
    );
}

#[test]
fn classify_subordinate_not_looked_up_snapshot_becomes_none() {
    let input = classify::ClassificationInput {
        path: RelPath::new("sub-only").expect("valid relpath"),
        basename: "sub-only".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(1011, EffectivePeerRole::Subordinate),
            live: Some(directory_meta("sub-only", "2025-07-01_00-00-00_000000Z")),
            snapshot: classify::SnapshotLookup::NotLookedUp,
        }],
    };

    let result = classify::classify_candidate(input).expect("classification succeeds");

    assert!(result.contributors.is_empty());
    assert_eq!(result.subordinates.len(), 1);
    assert!(matches!(
        result.subordinates[0],
        classify::SubordinateTarget { snapshot: None, .. }
    ));
}

#[test]
fn classify_preserves_canon_as_separate_observation() {
    let input = classify::ClassificationInput {
        path: RelPath::new("canon.txt").expect("valid relpath"),
        basename: "canon.txt".to_string(),
        peers: vec![
            classify::PeerCandidateInput {
                session: make_peer_session(1012, EffectivePeerRole::Contributing),
                live: Some(file_meta("canon.txt", 1, "2025-07-01_01-00-00_000000Z")),
                snapshot: classify::SnapshotLookup::Missing,
            },
            classify::PeerCandidateInput {
                session: make_peer_session(1013, EffectivePeerRole::Canon),
                live: Some(file_meta("canon.txt", 2, "2025-07-01_01-00-01_000000Z")),
                snapshot: classify::SnapshotLookup::Missing,
            },
        ],
    };

    let result = classify::classify_candidate(input).expect("classification succeeds");

    assert!(result.canon.is_some());
    let canon = result.canon.expect("canon peer present");

    assert_eq!(canon.peer_id, 1013);
    assert_eq!(result.contributors[1].state, canon.state);
}

#[test]
fn classify_output_order_tracks_input_run_order() {
    let input = classify::ClassificationInput {
        path: RelPath::new("ordered.txt").expect("valid relpath"),
        basename: "ordered.txt".to_string(),
        peers: vec![
            classify::PeerCandidateInput {
                session: make_peer_session(1014, EffectivePeerRole::Subordinate),
                live: Some(file_meta("ordered.txt", 1, "2025-01-01_00-00-00_000000Z")),
                snapshot: classify::SnapshotLookup::Present(snapshot_file_row(
                    "ordered.txt",
                    "2025-01-01_00-00-00_000000Z",
                    1,
                    Some("2025-01-01_00-00-00_000000Z"),
                    None,
                )),
            },
            classify::PeerCandidateInput {
                session: make_peer_session(1015, EffectivePeerRole::Contributing),
                live: Some(file_meta("ordered.txt", 1, "2025-01-01_00-00-01_000000Z")),
                snapshot: classify::SnapshotLookup::Present(snapshot_file_row(
                    "ordered.txt",
                    "2025-01-01_00-00-01_000000Z",
                    1,
                    Some("2025-01-01_00-00-01_000000Z"),
                    None,
                )),
            },
            classify::PeerCandidateInput {
                session: make_peer_session(1016, EffectivePeerRole::Contributing),
                live: None,
                snapshot: classify::SnapshotLookup::Missing,
            },
        ],
    };

    let result = classify::classify_candidate(input).expect("classification succeeds");

    assert_eq!(result.subordinates[0].peer_id, 1014);
    assert_eq!(result.contributors[0].peer_id, 1015);
    assert_eq!(result.contributors[1].peer_id, 1016);
    assert_eq!(result.summary.has_live_file, true);
}

#[test]
fn classify_returns_duplicate_peer_error() {
    let input = classify::ClassificationInput {
        path: RelPath::new("dup.txt").expect("valid relpath"),
        basename: "dup.txt".to_string(),
        peers: vec![
            classify::PeerCandidateInput {
                session: make_peer_session(2000, EffectivePeerRole::Contributing),
                live: Some(file_meta("dup.txt", 1, "2025-01-01_00-00-00_000000Z")),
                snapshot: classify::SnapshotLookup::Missing,
            },
            classify::PeerCandidateInput {
                session: make_peer_session(2000, EffectivePeerRole::Contributing),
                live: Some(file_meta("dup.txt", 2, "2025-01-01_00-00-01_000000Z")),
                snapshot: classify::SnapshotLookup::Missing,
            },
        ],
    };

    let err = classify::classify_candidate(input).unwrap_err();

    assert_eq!(
        err,
        classify::ClassificationError::DuplicatePeer { peer_id: 2000 }
    );
}

#[test]
fn classify_returns_multiple_canon_peers_error() {
    let input = classify::ClassificationInput {
        path: RelPath::new("multi-canon.txt").expect("valid relpath"),
        basename: "multi-canon.txt".to_string(),
        peers: vec![
            classify::PeerCandidateInput {
                session: make_peer_session(2001, EffectivePeerRole::Canon),
                live: Some(file_meta(
                    "multi-canon.txt",
                    1,
                    "2025-01-01_00-00-00_000000Z",
                )),
                snapshot: classify::SnapshotLookup::Missing,
            },
            classify::PeerCandidateInput {
                session: make_peer_session(2002, EffectivePeerRole::Canon),
                live: Some(file_meta(
                    "multi-canon.txt",
                    1,
                    "2025-01-01_00-00-00_000000Z",
                )),
                snapshot: classify::SnapshotLookup::Missing,
            },
        ],
    };

    let err = classify::classify_candidate(input).unwrap_err();

    assert_eq!(err, classify::ClassificationError::MultipleCanonPeers);
}

#[test]
fn classify_rejects_live_file_without_size() {
    let input = classify::ClassificationInput {
        path: RelPath::new("bad-file.txt").expect("valid relpath"),
        basename: "bad-file.txt".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(3000, EffectivePeerRole::Contributing),
            live: Some(file_meta("bad-file.txt", -1, "2025-01-01_00-00-00_000000Z")),
            snapshot: classify::SnapshotLookup::Missing,
        }],
    };

    let err = classify::classify_candidate(input).unwrap_err();

    assert_eq!(
        err,
        classify::ClassificationError::InvalidLiveMetadata {
            peer_id: 3000,
            reason: classify::InvalidLiveMetadata::FileWithoutSize,
        }
    );
}

#[test]
fn classify_rejects_directory_with_file_size_in_live_metadata() {
    let input = classify::ClassificationInput {
        path: RelPath::new("bad-dir").expect("valid relpath"),
        basename: "bad-dir".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(3001, EffectivePeerRole::Contributing),
            live: Some(EntryMeta {
                name: "bad-dir".to_string(),
                kind: EntryKind::Directory,
                mod_time: timestamp("2025-01-01_00-00-00_000000Z"),
                byte_size: 5,
            }),
            snapshot: classify::SnapshotLookup::Missing,
        }],
    };

    let err = classify::classify_candidate(input).unwrap_err();

    assert_eq!(
        err,
        classify::ClassificationError::InvalidLiveMetadata {
            peer_id: 3001,
            reason: classify::InvalidLiveMetadata::DirectoryWithFileSize,
        }
    );
}

#[test]
fn classify_rejects_snapshot_row_with_file_kind_facts_mismatch() {
    let input = classify::ClassificationInput {
        path: RelPath::new("mismatch.txt").expect("valid relpath"),
        basename: "mismatch.txt".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(3002, EffectivePeerRole::Contributing),
            live: Some(file_meta("mismatch.txt", 1, "2025-01-01_00-00-00_000000Z")),
            snapshot: classify::SnapshotLookup::Present(snapshot::SnapshotRow {
                path: RelPath::new("mismatch.txt").expect("valid relpath"),
                kind: snapshot::SnapshotEntryKind::File,
                mod_time: timestamp("2025-01-01_00-00-00_000000Z"),
                byte_size: -1,
                last_seen: Some(timestamp("2025-01-01_00-00-01_000000Z")),
                deleted_time: None,
            }),
        }],
    };

    let err = classify::classify_candidate(input).unwrap_err();

    assert_eq!(
        err,
        classify::ClassificationError::InvalidSnapshotState {
            peer_id: 3002,
            reason: classify::InvalidSnapshotState::KindFactsMismatch,
        }
    );
}

#[test]
fn classify_rejects_snapshot_row_with_directory_kind_facts_mismatch() {
    let input = classify::ClassificationInput {
        path: RelPath::new("dir-mismatch").expect("valid relpath"),
        basename: "dir-mismatch".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(3003, EffectivePeerRole::Contributing),
            live: Some(directory_meta(
                "dir-mismatch",
                "2025-01-01_00-00-00_000000Z",
            )),
            snapshot: classify::SnapshotLookup::Present(snapshot::SnapshotRow {
                path: RelPath::new("dir-mismatch").expect("valid relpath"),
                kind: snapshot::SnapshotEntryKind::Directory,
                mod_time: timestamp("2025-01-01_00-00-00_000000Z"),
                byte_size: 1,
                last_seen: Some(timestamp("2025-01-01_00-00-01_000000Z")),
                deleted_time: None,
            }),
        }],
    };

    let err = classify::classify_candidate(input).unwrap_err();

    assert_eq!(
        err,
        classify::ClassificationError::InvalidSnapshotState {
            peer_id: 3003,
            reason: classify::InvalidSnapshotState::KindFactsMismatch,
        }
    );
}

#[test]
fn classify_requires_snapshot_for_no_lookup_candidate() {
    let input = classify::ClassificationInput {
        path: RelPath::new("requires-lookups.txt").expect("valid relpath"),
        basename: "requires-lookups.txt".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(3004, EffectivePeerRole::Contributing),
            live: Some(file_meta(
                "requires-lookups.txt",
                1,
                "2025-01-01_00-00-00_000000Z",
            )),
            snapshot: classify::SnapshotLookup::NotLookedUp,
        }],
    };

    let err = classify::classify_candidate(input).unwrap_err();

    assert_eq!(
        err,
        classify::ClassificationError::MissingRequiredSnapshot { peer_id: 3004 }
    );
}

#[test]
fn classify_requires_snapshot_for_absence_candidate_when_not_looked_up() {
    let input = classify::ClassificationInput {
        path: RelPath::new("absent-missing-lookup.txt").expect("valid relpath"),
        basename: "absent-missing-lookup.txt".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(3005, EffectivePeerRole::Contributing),
            live: None,
            snapshot: classify::SnapshotLookup::NotLookedUp,
        }],
    };

    let err = classify::classify_candidate(input).unwrap_err();

    assert_eq!(
        err,
        classify::ClassificationError::MissingRequiredSnapshot { peer_id: 3005 }
    );
}

#[test]
fn classify_allows_live_directory_without_snapshot_lookup() {
    let input = classify::ClassificationInput {
        path: RelPath::new("nolookup-dir").expect("valid relpath"),
        basename: "nolookup-dir".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(3006, EffectivePeerRole::Contributing),
            live: Some(directory_meta(
                "nolookup-dir",
                "2025-07-01_00-00-00_000000Z",
            )),
            snapshot: classify::SnapshotLookup::NotLookedUp,
        }],
    };

    let result = classify::classify_candidate(input).expect("classification succeeds");

    assert_eq!(
        result.summary,
        classify::ClassificationSummary {
            has_live_file: false,
            has_live_directory: true,
            has_deletion_vote: false,
            has_unconfirmed_absence: false,
        }
    );
    assert_eq!(
        result.contributors[0].state,
        classify::ContributingState::LiveDirectory(classify::LiveDirectoryObservation {
            meta: directory_meta("nolookup-dir", "2025-07-01_00-00-00_000000Z"),
            previous: None,
        })
    );
}

#[test]
fn classify_rejects_file_snapshot_row_with_directory_facts() {
    let input = classify::ClassificationInput {
        path: RelPath::new("bad-file-facts").expect("valid relpath"),
        basename: "bad-file-facts".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(3007, EffectivePeerRole::Contributing),
            live: Some(file_meta(
                "bad-file-facts",
                1,
                "2025-07-01_00-00-00_000000Z",
            )),
            snapshot: classify::SnapshotLookup::Present(snapshot::SnapshotRow {
                path: RelPath::new("bad-file-facts").expect("valid relpath"),
                kind: snapshot::SnapshotEntryKind::File,
                mod_time: timestamp("2025-06-30_00-00-00_000000Z"),
                byte_size: -1,
                last_seen: Some(timestamp("2025-07-01_00-00-01_000000Z")),
                deleted_time: None,
            }),
        }],
    };

    let err = classify::classify_candidate(input).unwrap_err();

    assert_eq!(
        err,
        classify::ClassificationError::InvalidSnapshotState {
            peer_id: 3007,
            reason: classify::InvalidSnapshotState::KindFactsMismatch,
        }
    );
}

#[test]
fn classify_rejects_tombstone_snapshot_without_deleted_time() {
    let input = classify::ClassificationInput {
        path: RelPath::new("tombstone-without-time").expect("valid relpath"),
        basename: "tombstone-without-time".to_string(),
        peers: vec![classify::PeerCandidateInput {
            session: make_peer_session(4000, EffectivePeerRole::Contributing),
            live: None,
            snapshot: classify::SnapshotLookup::Present(snapshot::SnapshotRow {
                path: RelPath::new("tombstone-without-time").expect("valid relpath"),
                kind: snapshot::SnapshotEntryKind::Tombstone,
                mod_time: timestamp("2025-07-01_00-00-00_000000Z"),
                byte_size: -1,
                last_seen: Some(timestamp("2025-07-01_00-00-01_000000Z")),
                deleted_time: None,
            }),
        }],
    };

    let err = classify::classify_candidate(input).unwrap_err();

    assert_eq!(
        err,
        classify::ClassificationError::InvalidSnapshotState {
            peer_id: 4000,
            reason: classify::InvalidSnapshotState::TombstoneWithoutDeletedTime,
        }
    );
}
