use std::sync::Arc;

use treesyncplanner_fileoutcomes::{
    ClassifiedLiveFile, FileAbsenceIntent, FileGroupOutcome, FileOutcomePeer,
    FileOutcomePeerRole, FileOutcomeRequest, FileOutcomeSource, FileOutcomes, FileSnapshotRow,
    LiveFileFact, PeerFileClassificationRequest, PeerFileDecisionStatus, PeerFilePresenceFact,
    PeerFileState, SyncTimestamp,
};

fn subject() -> Arc<dyn FileOutcomes> {
    treesyncplanner_fileoutcomes::new(
        treesyncplanner_fileoutcomes_groupfiledecision::new(),
        treesyncplanner_fileoutcomes_peerfileclassification::new(),
    )
}

fn ts(unix_seconds: i64) -> SyncTimestamp {
    SyncTimestamp {
        unix_seconds,
        nanoseconds: 0,
    }
}

fn live(byte_size: u64, modified_time: i64, source_relative_path: &str) -> PeerFileState {
    PeerFileState::ModifiedLiveFile(ClassifiedLiveFile {
        byte_size,
        modified_time: ts(modified_time),
        source_relative_path: source_relative_path.to_string(),
    })
}

fn unchanged(byte_size: u64, modified_time: i64, source_relative_path: &str) -> PeerFileState {
    PeerFileState::UnchangedLiveFile(ClassifiedLiveFile {
        byte_size,
        modified_time: ts(modified_time),
        source_relative_path: source_relative_path.to_string(),
    })
}

fn new_live(byte_size: u64, modified_time: i64, source_relative_path: &str) -> PeerFileState {
    PeerFileState::NewLiveFile(ClassifiedLiveFile {
        byte_size,
        modified_time: ts(modified_time),
        source_relative_path: source_relative_path.to_string(),
    })
}

fn peer(peer_id: &str, role: FileOutcomePeerRole, classification: PeerFileState) -> FileOutcomePeer {
    FileOutcomePeer {
        peer_id: peer_id.to_string(),
        role,
        classification,
    }
}

fn decide(file_outcomes: &dyn FileOutcomes, peers: Vec<FileOutcomePeer>) -> treesyncplanner_fileoutcomes::FileOutcomeDecision {
    file_outcomes
        .decide_file_outcome(FileOutcomeRequest {
            relative_path: "CaseName.txt".to_string(),
            peers,
        })
        .expect("file outcome decision should succeed")
}

fn assert_has_status(
    decision: &treesyncplanner_fileoutcomes::FileOutcomeDecision,
    peer_id: &str,
    status: PeerFileDecisionStatus,
) {
    let peer_decision = decision
        .peer_decisions
        .iter()
        .find(|fact| fact.peer_id == peer_id)
        .expect("peer decision should be present");

    assert!(
        peer_decision.statuses.contains(&status),
        "expected {peer_id} to have status {status:?}, got {:?}",
        peer_decision.statuses
    );
}

#[test]
fn classifies_live_and_absent_peer_file_states_with_five_second_tolerance() {
    let file_outcomes = subject();

    let classify = |peer_id: &str,
                    presence: PeerFilePresenceFact,
                    snapshot_row: Option<FileSnapshotRow>,
                    last_seen: Option<SyncTimestamp>| {
        file_outcomes
            .classify_peer_file(PeerFileClassificationRequest {
                peer_id: peer_id.to_string(),
                relative_path: "CaseName.txt".to_string(),
                presence,
                snapshot_row,
                last_seen,
            })
            .expect("classification should succeed")
            .state
    };

    let row = |byte_size, modified_time, deleted_time| FileSnapshotRow {
        byte_size,
        modified_time,
        deleted_time,
    };
    let live_fact = |byte_size, modified_time| {
        PeerFilePresenceFact::LiveFile(LiveFileFact {
            byte_size,
            modified_time: ts(modified_time),
            source_relative_path: "CaseName.txt".to_string(),
        })
    };

    assert_eq!(
        classify(
            "unchanged",
            live_fact(12, 105),
            Some(row(Some(12), Some(ts(100)), None)),
            None,
        ),
        PeerFileState::UnchangedLiveFile(ClassifiedLiveFile {
            byte_size: 12,
            modified_time: ts(105),
            source_relative_path: "CaseName.txt".to_string(),
        })
    );
    assert!(matches!(
        classify(
            "different-size",
            live_fact(13, 100),
            Some(row(Some(12), Some(ts(100)), None)),
            None,
        ),
        PeerFileState::ModifiedLiveFile(_)
    ));
    assert!(matches!(
        classify(
            "different-time",
            live_fact(12, 106),
            Some(row(Some(12), Some(ts(100)), None)),
            None,
        ),
        PeerFileState::ModifiedLiveFile(_)
    ));
    assert!(matches!(
        classify(
            "deleted-row-live-file",
            live_fact(12, 100),
            Some(row(Some(12), Some(ts(100)), Some(ts(90)))),
            None,
        ),
        PeerFileState::ModifiedLiveFile(_)
    ));
    assert!(matches!(
        classify("new-live", live_fact(12, 100), None, None),
        PeerFileState::NewLiveFile(_)
    ));
    assert_eq!(
        classify(
            "deleted",
            PeerFilePresenceFact::Absent,
            Some(row(Some(12), Some(ts(100)), Some(ts(120)))),
            None,
        ),
        PeerFileState::DeletedFile {
            deletion_estimate: ts(120)
        }
    );
    assert_eq!(
        classify(
            "absent-unconfirmed",
            PeerFilePresenceFact::Absent,
            Some(row(Some(12), Some(ts(100)), None)),
            Some(ts(130)),
        ),
        PeerFileState::AbsentUnconfirmed {
            last_seen: Some(ts(130))
        }
    );
    assert_eq!(
        classify("absent-no-row", PeerFilePresenceFact::Absent, None, None),
        PeerFileState::AbsentNoRowNoVote
    );
}

#[test]
fn canon_peer_file_or_absence_selects_outcome_without_non_canon_votes_changing_it() {
    let file_outcomes = subject();

    let canon_file = decide(
        file_outcomes.as_ref(),
        vec![
            peer("canon", FileOutcomePeerRole::Canon, live(12, 100, "CaseName.txt")),
            peer("newer", FileOutcomePeerRole::Contributing, live(99, 200, "CaseName.txt")),
            peer("missing", FileOutcomePeerRole::Subordinate, PeerFileState::AbsentNoRowNoVote),
        ],
    );

    assert_eq!(
        canon_file.group_outcome,
        FileGroupOutcome::ExistingFile {
            byte_size: 12,
            modified_time: ts(100),
        }
    );
    assert_eq!(canon_file.copy_intents.len(), 2);
    assert!(
        canon_file
            .copy_intents
            .iter()
            .all(|intent| intent.source_peer_id == "canon")
    );
    assert_has_status(&canon_file, "canon", PeerFileDecisionStatus::CanonSelectedOutcome);
    assert_has_status(&canon_file, "newer", PeerFileDecisionStatus::DidNotVote);

    let canon_absent = decide(
        file_outcomes.as_ref(),
        vec![
            peer(
                "canon",
                FileOutcomePeerRole::Canon,
                PeerFileState::DeletedFile {
                    deletion_estimate: ts(150),
                },
            ),
            peer("contributor", FileOutcomePeerRole::Contributing, live(12, 200, "CaseName.txt")),
            peer("subordinate", FileOutcomePeerRole::Subordinate, live(12, 210, "CaseName.txt")),
        ],
    );

    assert_eq!(
        canon_absent.group_outcome,
        FileGroupOutcome::Deletion {
            deletion_estimate: Some(ts(150)),
        }
    );
    assert!(canon_absent.copy_intents.is_empty());
    assert_eq!(canon_absent.absence_intents.len(), 2);
    assert!(canon_absent
        .absence_intents
        .contains(&FileAbsenceIntent::DeleteFile {
            peer_id: "contributor".to_string(),
            relative_path: "CaseName.txt".to_string(),
        }));
    assert!(canon_absent
        .absence_intents
        .contains(&FileAbsenceIntent::DeleteFile {
            peer_id: "subordinate".to_string(),
            relative_path: "CaseName.txt".to_string(),
        }));
    assert_has_status(&canon_absent, "canon", PeerFileDecisionStatus::CanonSelectedOutcome);
}

#[test]
fn matching_unchanged_contributors_do_not_copy_between_each_other_but_targets_receive_file() {
    let file_outcomes = subject();

    let decision = decide(
        file_outcomes.as_ref(),
        vec![
            peer("left", FileOutcomePeerRole::Contributing, unchanged(12, 100, "CaseName.txt")),
            peer("right", FileOutcomePeerRole::Contributing, unchanged(12, 103, "CaseName.txt")),
            peer("subordinate", FileOutcomePeerRole::Subordinate, PeerFileState::AbsentNoRowNoVote),
        ],
    );

    assert_eq!(
        decision.group_outcome,
        FileGroupOutcome::ExistingFile {
            byte_size: 12,
            modified_time: ts(103),
        }
    );
    assert_eq!(decision.copy_intents.len(), 1);
    assert_eq!(decision.copy_intents[0].destination_peer_id, "subordinate");
    assert_ne!(decision.copy_intents[0].destination_peer_id, "left");
    assert_ne!(decision.copy_intents[0].destination_peer_id, "right");
    assert_has_status(&decision, "subordinate", PeerFileDecisionStatus::DidNotVote);
    assert_has_status(&decision, "subordinate", PeerFileDecisionStatus::NeedsCopy);
    assert_has_status(&decision, "left", PeerFileDecisionStatus::NotSelectedForCopy);
    assert_has_status(&decision, "right", PeerFileDecisionStatus::NotSelectedForCopy);
}

#[test]
fn live_file_winner_uses_tolerance_then_size_and_copies_from_an_identical_source() {
    let file_outcomes = subject();

    let decision = decide(
        file_outcomes.as_ref(),
        vec![
            peer("old", FileOutcomePeerRole::Contributing, live(999, 94, "CaseName.txt")),
            peer("small", FileOutcomePeerRole::Contributing, live(10, 100, "CaseName.txt")),
            peer("large-a", FileOutcomePeerRole::Contributing, live(20, 104, "CaseName.txt")),
            peer("large-b", FileOutcomePeerRole::Contributing, new_live(20, 105, "CaseName.txt")),
            peer("near-match", FileOutcomePeerRole::Subordinate, live(20, 101, "CaseName.txt")),
            peer("missing", FileOutcomePeerRole::Subordinate, PeerFileState::AbsentNoRowNoVote),
        ],
    );

    assert_eq!(
        decision.group_outcome,
        FileGroupOutcome::ExistingFile {
            byte_size: 20,
            modified_time: ts(105),
        }
    );
    assert_eq!(decision.source_peers.len(), 2);
    assert!(decision.source_peers.contains(&FileOutcomeSource {
        peer_id: "large-a".to_string(),
        source_relative_path: "CaseName.txt".to_string(),
        byte_size: 20,
        modified_time: ts(104),
    }));
    assert!(decision.source_peers.contains(&FileOutcomeSource {
        peer_id: "large-b".to_string(),
        source_relative_path: "CaseName.txt".to_string(),
        byte_size: 20,
        modified_time: ts(105),
    }));
    assert_eq!(decision.copy_intents.len(), 3);
    assert!(
        decision
            .copy_intents
            .iter()
            .any(|intent| intent.destination_peer_id == "small")
    );
    assert!(
        decision
            .copy_intents
            .iter()
            .any(|intent| intent.destination_peer_id == "old")
    );
    assert!(
        decision
            .copy_intents
            .iter()
            .any(|intent| intent.destination_peer_id == "missing")
    );
    assert!(decision.copy_intents.iter().all(|intent| {
        intent.source_peer_id == "large-a" || intent.source_peer_id == "large-b"
    }));
    assert!(
        decision
            .copy_intents
            .iter()
            .all(|intent| intent.source_relative_path == "CaseName.txt")
    );
    assert_has_status(&decision, "small", PeerFileDecisionStatus::VotedForExistingFile);
    assert_has_status(&decision, "large-a", PeerFileDecisionStatus::IdenticalSource);
    assert_has_status(&decision, "large-b", PeerFileDecisionStatus::IdenticalSource);
    assert_has_status(&decision, "near-match", PeerFileDecisionStatus::NotSelectedForCopy);
}

#[test]
fn deletion_votes_use_latest_estimate_and_lose_to_existing_file_within_tolerance() {
    let file_outcomes = subject();

    let file_wins = decide(
        file_outcomes.as_ref(),
        vec![
            peer("file", FileOutcomePeerRole::Contributing, live(12, 100, "CaseName.txt")),
            peer(
                "deleted",
                FileOutcomePeerRole::Contributing,
                PeerFileState::DeletedFile {
                    deletion_estimate: ts(105),
                },
            ),
            peer(
                "older-deleted",
                FileOutcomePeerRole::Contributing,
                PeerFileState::DeletedFile {
                    deletion_estimate: ts(103),
                },
            ),
            peer(
                "unconfirmed-no-last-seen",
                FileOutcomePeerRole::Contributing,
                PeerFileState::AbsentUnconfirmed { last_seen: None },
            ),
            peer(
                "unconfirmed-near-last-seen",
                FileOutcomePeerRole::Contributing,
                PeerFileState::AbsentUnconfirmed {
                    last_seen: Some(ts(105)),
                },
            ),
        ],
    );

    assert_eq!(
        file_wins.group_outcome,
        FileGroupOutcome::ExistingFile {
            byte_size: 12,
            modified_time: ts(100),
        }
    );
    assert_eq!(file_wins.copy_intents.len(), 4);
    assert_has_status(&file_wins, "deleted", PeerFileDecisionStatus::VotedForDeletion);
    assert_has_status(
        &file_wins,
        "unconfirmed-no-last-seen",
        PeerFileDecisionStatus::DidNotVote,
    );
    assert_has_status(
        &file_wins,
        "unconfirmed-near-last-seen",
        PeerFileDecisionStatus::DidNotVote,
    );

    let deletion_wins = decide(
        file_outcomes.as_ref(),
        vec![
            peer("file", FileOutcomePeerRole::Contributing, live(12, 100, "CaseName.txt")),
            peer(
                "deleted",
                FileOutcomePeerRole::Contributing,
                PeerFileState::DeletedFile {
                    deletion_estimate: ts(104),
                },
            ),
            peer(
                "unconfirmed-newer",
                FileOutcomePeerRole::Contributing,
                PeerFileState::AbsentUnconfirmed {
                    last_seen: Some(ts(106)),
                },
            ),
        ],
    );

    assert_eq!(
        deletion_wins.group_outcome,
        FileGroupOutcome::Deletion {
            deletion_estimate: Some(ts(106)),
        }
    );
    assert!(deletion_wins.copy_intents.is_empty());
    assert_eq!(
        deletion_wins.absence_intents,
        vec![FileAbsenceIntent::DeleteFile {
            peer_id: "file".to_string(),
            relative_path: "CaseName.txt".to_string(),
        }]
    );
    assert_has_status(
        &deletion_wins,
        "unconfirmed-newer",
        PeerFileDecisionStatus::VotedForDeletion,
    );

    let exact_tie = decide(
        file_outcomes.as_ref(),
        vec![
            peer("file", FileOutcomePeerRole::Contributing, live(12, 100, "CaseName.txt")),
            peer(
                "deleted",
                FileOutcomePeerRole::Contributing,
                PeerFileState::DeletedFile {
                    deletion_estimate: ts(100),
                },
            ),
        ],
    );

    assert_eq!(
        exact_tie.group_outcome,
        FileGroupOutcome::ExistingFile {
            byte_size: 12,
            modified_time: ts(100),
        }
    );
}

#[test]
fn all_contributors_with_no_snapshot_vote_select_no_file_and_displace_subordinate_files() {
    let file_outcomes = subject();

    let decision = decide(
        file_outcomes.as_ref(),
        vec![
            peer("left", FileOutcomePeerRole::Contributing, PeerFileState::AbsentNoRowNoVote),
            peer("right", FileOutcomePeerRole::Contributing, PeerFileState::AbsentNoRowNoVote),
            peer("subordinate", FileOutcomePeerRole::Subordinate, live(12, 100, "CaseName.txt")),
        ],
    );

    assert_eq!(decision.group_outcome, FileGroupOutcome::NoFile);
    assert!(decision.copy_intents.is_empty());
    assert_eq!(
        decision.absence_intents,
        vec![FileAbsenceIntent::DisplaceFile {
            peer_id: "subordinate".to_string(),
            relative_path: "CaseName.txt".to_string(),
        }]
    );
    assert_has_status(&decision, "left", PeerFileDecisionStatus::DidNotVote);
    assert_has_status(&decision, "subordinate", PeerFileDecisionStatus::NeedsDisplacement);
}
