use treesyncplanner_fileoutcomes_groupfiledecision::{
    new as create_group_file_decision, ClassifiedLiveFile, FileAbsenceIntent,
    FileGroupOutcome, GroupFileDecisionError, GroupFileDecisionOutput,
    GroupFileDecisionPeer,
    GroupFileDecisionPeerRole, GroupFileDecisionRequest, PeerFileDecisionStatus,
    PeerFileState, SyncTimestamp,
};

const PATH: &str = "dir/file.txt";

fn ts(unix_seconds: i64) -> SyncTimestamp {
    SyncTimestamp {
        unix_seconds,
        nanoseconds: 0,
    }
}

fn live_file(byte_size: u64, modified_seconds: i64) -> ClassifiedLiveFile {
    ClassifiedLiveFile {
        byte_size,
        modified_time: ts(modified_seconds),
        source_relative_path: PATH.to_string(),
    }
}

fn peer(
    peer_id: &str,
    role: GroupFileDecisionPeerRole,
    classification: PeerFileState,
) -> GroupFileDecisionPeer {
    GroupFileDecisionPeer {
        peer_id: peer_id.to_string(),
        role,
        classification,
    }
}

fn decide(peers: Vec<GroupFileDecisionPeer>) -> GroupFileDecisionOutput {
    let subject = create_group_file_decision();
    subject
        .decide_group_file(GroupFileDecisionRequest {
            relative_path: PATH.to_string(),
            peers,
        })
        .expect("decision should succeed")
}

fn copy_destinations(output: &GroupFileDecisionOutput) -> Vec<String> {
    let mut destinations = output
        .copy_intents
        .iter()
        .map(|intent| intent.destination_peer_id.clone())
        .collect::<Vec<_>>();
    destinations.sort();
    destinations
}

fn source_ids(output: &GroupFileDecisionOutput) -> Vec<String> {
    let mut ids = output
        .source_peers
        .iter()
        .map(|source| source.peer_id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn has_delete_intent(output: &GroupFileDecisionOutput, peer_id: &str) -> bool {
    output.absence_intents.iter().any(|intent| {
        matches!(
            intent,
            FileAbsenceIntent::DeleteFile {
                peer_id: intent_peer_id,
                relative_path,
            } if intent_peer_id == peer_id && relative_path == PATH
        )
    })
}

fn has_displace_intent(output: &GroupFileDecisionOutput, peer_id: &str) -> bool {
    output.absence_intents.iter().any(|intent| {
        matches!(
            intent,
            FileAbsenceIntent::DisplaceFile {
                peer_id: intent_peer_id,
                relative_path,
            } if intent_peer_id == peer_id && relative_path == PATH
        )
    })
}

fn statuses_for<'a>(
    output: &'a GroupFileDecisionOutput,
    peer_id: &str,
) -> &'a [PeerFileDecisionStatus] {
    output
        .peer_decisions
        .iter()
        .find(|decision| decision.peer_id == peer_id)
        .expect("peer decision should be present")
        .statuses
        .as_slice()
}

fn assert_status(
    output: &GroupFileDecisionOutput,
    peer_id: &str,
    status: PeerFileDecisionStatus,
) {
    assert!(
        statuses_for(output, peer_id).contains(&status),
        "{peer_id} should have status {status:?}"
    );
}

#[test]
fn canon_live_file_selects_outcome_and_non_canon_state_cannot_change_it() {
    let output = decide(vec![
        peer(
            "canon",
            GroupFileDecisionPeerRole::Canon,
            PeerFileState::UnchangedLiveFile(live_file(10, 100)),
        ),
        peer(
            "newer-contributor",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::ModifiedLiveFile(live_file(99, 200)),
        ),
        peer(
            "empty-subordinate",
            GroupFileDecisionPeerRole::Subordinate,
            PeerFileState::AbsentNoRowNoVote,
        ),
    ]);

    assert_eq!(
        output.group_outcome,
        FileGroupOutcome::ExistingFile {
            byte_size: 10,
            modified_time: ts(100),
        }
    );
    assert_eq!(source_ids(&output), vec!["canon"]);
    assert_eq!(
        copy_destinations(&output),
        vec!["empty-subordinate", "newer-contributor"]
    );
    assert!(output.absence_intents.is_empty());
    assert_status(
        &output,
        "canon",
        PeerFileDecisionStatus::CanonSelectedOutcome,
    );
    assert_status(
        &output,
        "newer-contributor",
        PeerFileDecisionStatus::DidNotVote,
    );
}

#[test]
fn canon_without_file_selects_deletion_for_every_live_peer() {
    let output = decide(vec![
        peer(
            "canon",
            GroupFileDecisionPeerRole::Canon,
            PeerFileState::AbsentNoRowNoVote,
        ),
        peer(
            "contributor-live",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::ModifiedLiveFile(live_file(10, 100)),
        ),
        peer(
            "subordinate-live",
            GroupFileDecisionPeerRole::Subordinate,
            PeerFileState::NewLiveFile(live_file(20, 110)),
        ),
        peer(
            "already-absent",
            GroupFileDecisionPeerRole::Subordinate,
            PeerFileState::AbsentUnconfirmed {
                last_seen: Some(ts(300)),
            },
        ),
    ]);

    assert_eq!(
        output.group_outcome,
        FileGroupOutcome::Deletion {
            deletion_estimate: None,
        }
    );
    assert!(output.copy_intents.is_empty());
    assert!(has_delete_intent(&output, "contributor-live"));
    assert!(has_delete_intent(&output, "subordinate-live"));
    assert!(!has_delete_intent(&output, "already-absent"));
    assert!(!has_displace_intent(&output, "subordinate-live"));
    assert_status(
        &output,
        "canon",
        PeerFileDecisionStatus::CanonSelectedOutcome,
    );
}

#[test]
fn unchanged_matching_contributors_select_the_file_without_copying_between_them() {
    let output = decide(vec![
        peer(
            "left",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::UnchangedLiveFile(live_file(12, 100)),
        ),
        peer(
            "right",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::UnchangedLiveFile(live_file(12, 100)),
        ),
        peer(
            "absent-without-last-seen",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::AbsentUnconfirmed { last_seen: None },
        ),
        peer(
            "empty-subordinate",
            GroupFileDecisionPeerRole::Subordinate,
            PeerFileState::AbsentNoRowNoVote,
        ),
    ]);

    assert_eq!(
        output.group_outcome,
        FileGroupOutcome::ExistingFile {
            byte_size: 12,
            modified_time: ts(100),
        }
    );
    assert_eq!(source_ids(&output), vec!["left", "right"]);
    assert_eq!(
        copy_destinations(&output),
        vec!["absent-without-last-seen", "empty-subordinate"]
    );
    assert_status(&output, "left", PeerFileDecisionStatus::MatchedWinner);
    assert_status(&output, "right", PeerFileDecisionStatus::NotSelectedForCopy);
    assert_status(
        &output,
        "absent-without-last-seen",
        PeerFileDecisionStatus::DidNotVote,
    );
}

#[test]
fn subordinate_live_files_do_not_vote_but_are_targets_for_contributor_winners() {
    let output = decide(vec![
        peer(
            "old-contributor",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::ModifiedLiveFile(live_file(10, 100)),
        ),
        peer(
            "new-contributor",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::ModifiedLiveFile(live_file(11, 106)),
        ),
        peer(
            "newer-subordinate",
            GroupFileDecisionPeerRole::Subordinate,
            PeerFileState::ModifiedLiveFile(live_file(99, 500)),
        ),
    ]);

    assert_eq!(
        output.group_outcome,
        FileGroupOutcome::ExistingFile {
            byte_size: 11,
            modified_time: ts(106),
        }
    );
    assert_eq!(source_ids(&output), vec!["new-contributor"]);
    assert_eq!(
        copy_destinations(&output),
        vec!["newer-subordinate", "old-contributor"]
    );
    assert_status(
        &output,
        "newer-subordinate",
        PeerFileDecisionStatus::DidNotVote,
    );
    assert_status(
        &output,
        "newer-subordinate",
        PeerFileDecisionStatus::NeedsCopy,
    );
}

#[test]
fn new_file_winner_propagates_to_peers_with_no_snapshot_row() {
    let output = decide(vec![
        peer(
            "older-new",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::NewLiveFile(live_file(10, 100)),
        ),
        peer(
            "newer-new",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::NewLiveFile(live_file(11, 106)),
        ),
        peer(
            "no-row",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::AbsentNoRowNoVote,
        ),
    ]);

    assert_eq!(
        output.group_outcome,
        FileGroupOutcome::ExistingFile {
            byte_size: 11,
            modified_time: ts(106),
        }
    );
    assert_eq!(source_ids(&output), vec!["newer-new"]);
    assert_eq!(copy_destinations(&output), vec!["no-row", "older-new"]);
}

#[test]
fn live_votes_within_five_seconds_use_byte_size_and_equal_size_sources_are_identical() {
    let output = decide(vec![
        peer(
            "max-time-smaller",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::ModifiedLiveFile(live_file(10, 100)),
        ),
        peer(
            "larger-near-a",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::ModifiedLiveFile(live_file(20, 96)),
        ),
        peer(
            "larger-near-b",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::ModifiedLiveFile(live_file(20, 100)),
        ),
        peer(
            "matching-subordinate",
            GroupFileDecisionPeerRole::Subordinate,
            PeerFileState::ModifiedLiveFile(live_file(20, 97)),
        ),
        peer(
            "empty-target",
            GroupFileDecisionPeerRole::Subordinate,
            PeerFileState::AbsentNoRowNoVote,
        ),
    ]);

    assert_eq!(
        output.group_outcome,
        FileGroupOutcome::ExistingFile {
            byte_size: 20,
            modified_time: ts(100),
        }
    );
    assert_eq!(source_ids(&output), vec!["larger-near-a", "larger-near-b"]);
    assert_eq!(
        copy_destinations(&output),
        vec!["empty-target", "max-time-smaller"]
    );
    assert_status(
        &output,
        "larger-near-a",
        PeerFileDecisionStatus::IdenticalSource,
    );
    assert_status(
        &output,
        "larger-near-b",
        PeerFileDecisionStatus::IdenticalSource,
    );
    assert_status(
        &output,
        "larger-near-a",
        PeerFileDecisionStatus::NotSelectedForCopy,
    );
    assert_status(
        &output,
        "larger-near-b",
        PeerFileDecisionStatus::NotSelectedForCopy,
    );
    assert_status(
        &output,
        "matching-subordinate",
        PeerFileDecisionStatus::NotSelectedForCopy,
    );
    assert!(output
        .copy_intents
        .iter()
        .any(|intent| intent.destination_peer_id == "empty-target"
            && (intent.source_peer_id == "larger-near-a"
                || intent.source_peer_id == "larger-near-b")));
}

#[test]
fn deletion_newer_than_existing_by_more_than_five_seconds_wins() {
    let output = decide(vec![
        peer(
            "live",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::ModifiedLiveFile(live_file(10, 100)),
        ),
        peer(
            "older-delete",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::DeletedFile {
                deletion_estimate: ts(104),
            },
        ),
        peer(
            "newer-delete",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::DeletedFile {
                deletion_estimate: ts(106),
            },
        ),
    ]);

    assert_eq!(
        output.group_outcome,
        FileGroupOutcome::Deletion {
            deletion_estimate: Some(ts(106)),
        }
    );
    assert!(output.copy_intents.is_empty());
    assert!(has_delete_intent(&output, "live"));
    assert_status(
        &output,
        "newer-delete",
        PeerFileDecisionStatus::VotedForDeletion,
    );
}

#[test]
fn existing_file_wins_when_deletion_is_not_more_than_five_seconds_newer() {
    let output = decide(vec![
        peer(
            "live",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::ModifiedLiveFile(live_file(10, 100)),
        ),
        peer(
            "delete-at-tolerance",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::DeletedFile {
                deletion_estimate: ts(105),
            },
        ),
    ]);

    assert_eq!(
        output.group_outcome,
        FileGroupOutcome::ExistingFile {
            byte_size: 10,
            modified_time: ts(100),
        }
    );
    assert_eq!(copy_destinations(&output), vec!["delete-at-tolerance"]);
}

#[test]
fn existing_file_wins_when_deletion_estimate_ties_it() {
    let output = decide(vec![
        peer(
            "live",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::ModifiedLiveFile(live_file(10, 100)),
        ),
        peer(
            "delete-tied",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::DeletedFile {
                deletion_estimate: ts(100),
            },
        ),
    ]);

    assert_eq!(
        output.group_outcome,
        FileGroupOutcome::ExistingFile {
            byte_size: 10,
            modified_time: ts(100),
        }
    );
    assert_eq!(copy_destinations(&output), vec!["delete-tied"]);
}

#[test]
fn absent_unconfirmed_votes_deletion_only_when_last_seen_is_more_than_five_seconds_newer() {
    let deletion_output = decide(vec![
        peer(
            "live",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::ModifiedLiveFile(live_file(10, 100)),
        ),
        peer(
            "absent-newer",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::AbsentUnconfirmed {
                last_seen: Some(ts(106)),
            },
        ),
    ]);

    assert_eq!(
        deletion_output.group_outcome,
        FileGroupOutcome::Deletion {
            deletion_estimate: Some(ts(106)),
        }
    );
    assert_status(
        &deletion_output,
        "absent-newer",
        PeerFileDecisionStatus::VotedForDeletion,
    );

    let existing_output = decide(vec![
        peer(
            "live",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::ModifiedLiveFile(live_file(10, 100)),
        ),
        peer(
            "absent-at-tolerance",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::AbsentUnconfirmed {
                last_seen: Some(ts(105)),
            },
        ),
    ]);

    assert_eq!(
        existing_output.group_outcome,
        FileGroupOutcome::ExistingFile {
            byte_size: 10,
            modified_time: ts(100),
        }
    );
    assert_eq!(copy_destinations(&existing_output), vec!["absent-at-tolerance"]);
    assert_status(
        &existing_output,
        "absent-at-tolerance",
        PeerFileDecisionStatus::DidNotVote,
    );
}

#[test]
fn all_contributors_absent_with_no_row_produces_no_file_and_displaces_subordinate_live_file() {
    let output = decide(vec![
        peer(
            "missing-left",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::AbsentNoRowNoVote,
        ),
        peer(
            "missing-right",
            GroupFileDecisionPeerRole::Contributing,
            PeerFileState::AbsentNoRowNoVote,
        ),
        peer(
            "subordinate-live",
            GroupFileDecisionPeerRole::Subordinate,
            PeerFileState::ModifiedLiveFile(live_file(10, 100)),
        ),
    ]);

    assert_eq!(output.group_outcome, FileGroupOutcome::NoFile);
    assert!(output.copy_intents.is_empty());
    assert!(output.source_peers.is_empty());
    assert!(has_displace_intent(&output, "subordinate-live"));
    assert!(!has_delete_intent(&output, "subordinate-live"));
    assert_status(
        &output,
        "subordinate-live",
        PeerFileDecisionStatus::NeedsDisplacement,
    );
}

#[test]
fn contradictory_canon_role_facts_return_invalid_input() {
    let subject = create_group_file_decision();
    let result = subject.decide_group_file(GroupFileDecisionRequest {
        relative_path: PATH.to_string(),
        peers: vec![
            peer(
                "first-canon",
                GroupFileDecisionPeerRole::Canon,
                PeerFileState::AbsentNoRowNoVote,
            ),
            peer(
                "second-canon",
                GroupFileDecisionPeerRole::Canon,
                PeerFileState::UnchangedLiveFile(live_file(10, 100)),
            ),
        ],
    });

    assert!(matches!(
        result,
        Err(GroupFileDecisionError::InvalidInput(_))
    ));
}
