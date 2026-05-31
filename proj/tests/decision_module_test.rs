#[path = "../sync/classify/mod.rs"]
#[allow(dead_code)]
mod classify;
#[path = "../sync/decision/mod.rs"]
#[allow(dead_code)]
mod decision;

use classify::{
    AbsentDirectoryHistory, AbsentUnconfirmedFile, CanonObservation, ClassifiedCandidate,
    ContributingObservation, ContributingState, LiveDirectoryObservation, LiveFileObservation,
    LiveFileSnapshotState, SnapshotDirectoryFacts, SnapshotFileFacts, SubordinateTarget,
    TombstoneDeletionVote,
};
use decision::{
    AbsenceDecision, ClassifiedDecisionInput, DecisionOutcome, DecisionSkipReason,
    DirectoryDecision, FileDecision, InvalidDecisionInput,
};
use kitchensync::{EntryKind, EntryMeta, PeerId, RelPath, Timestamp};

fn test_path(value: &str) -> RelPath {
    RelPath::new(value).expect("test path")
}

fn file_meta(name: &str, mod_time: &str, byte_size: i64) -> EntryMeta {
    EntryMeta {
        name: name.to_string(),
        kind: EntryKind::File,
        mod_time: Timestamp(mod_time.to_string()),
        byte_size,
    }
}

fn dir_meta(name: &str, mod_time: &str) -> EntryMeta {
    EntryMeta {
        name: name.to_string(),
        kind: EntryKind::Directory,
        mod_time: Timestamp(mod_time.to_string()),
        byte_size: -1,
    }
}

fn mk_canon_observation(peer_id: PeerId, state: ContributingState) -> CanonObservation {
    CanonObservation { peer_id, state }
}

fn mk_contributor(peer_id: PeerId, state: ContributingState) -> ContributingObservation {
    ContributingObservation { peer_id, state }
}

fn mk_live_file(
    peer_id: PeerId,
    name: &str,
    mod_time: &str,
    byte_size: i64,
) -> ContributingObservation {
    mk_contributor(
        peer_id,
        ContributingState::LiveFile(LiveFileObservation {
            meta: file_meta(name, mod_time, byte_size),
            snapshot: LiveFileSnapshotState::New,
        }),
    )
}

fn mk_live_directory(peer_id: PeerId, name: &str, mod_time: &str) -> ContributingObservation {
    mk_contributor(
        peer_id,
        ContributingState::LiveDirectory(LiveDirectoryObservation {
            meta: dir_meta(name, mod_time),
            previous: None,
        }),
    )
}

fn mk_tombstone(peer_id: PeerId, deleted_time: &str) -> ContributingObservation {
    mk_contributor(
        peer_id,
        ContributingState::TombstoneDeletionVote(TombstoneDeletionVote {
            deleted_time: Timestamp(deleted_time.to_string()),
        }),
    )
}

fn mk_absent_unconfirmed(peer_id: PeerId, last_seen: &str) -> ContributingObservation {
    mk_contributor(
        peer_id,
        ContributingState::AbsentUnconfirmedFile(AbsentUnconfirmedFile {
            previous: SnapshotFileFacts {
                size: 10,
                modified_time: Timestamp("2020-01-01_00-00-00_000000Z".to_string()),
                last_seen: Timestamp(last_seen.to_string()),
            },
        }),
    )
}

fn mk_absent_directory(peer_id: PeerId, last_seen: &str) -> ContributingObservation {
    mk_contributor(
        peer_id,
        ContributingState::AbsentDirectoryHistory(AbsentDirectoryHistory {
            previous: SnapshotDirectoryFacts {
                modified_time: Some(Timestamp("2020-01-01_00-00-00_000000Z".to_string())),
                last_seen: Timestamp(last_seen.to_string()),
            },
        }),
    )
}

fn mk_no_vote(peer_id: PeerId) -> ContributingObservation {
    mk_contributor(peer_id, ContributingState::NoVote)
}

fn mk_subordinate_live_file(
    peer_id: PeerId,
    name: &str,
    mod_time: &str,
    byte_size: i64,
) -> SubordinateTarget {
    SubordinateTarget {
        peer_id,
        live: Some(file_meta(name, mod_time, byte_size)),
        snapshot: None,
    }
}

fn mk_candidate(
    path: &str,
    canon: Option<CanonObservation>,
    contributors: Vec<ContributingObservation>,
    subordinates: Vec<SubordinateTarget>,
) -> ClassifiedCandidate {
    let path = test_path(path);
    ClassifiedCandidate {
        path: path.clone(),
        basename: path
            .as_str()
            .split('/')
            .next_back()
            .unwrap_or(path.as_str())
            .to_string(),
        canon,
        contributors,
        subordinates,
        summary: Default::default(),
    }
}

fn mk_input(
    candidate: ClassifiedCandidate,
    active_canon_count: usize,
    canon_required: bool,
    skip: Option<DecisionSkipReason>,
) -> ClassifiedDecisionInput {
    ClassifiedDecisionInput {
        candidate,
        active_canon_count,
        canon_required,
        skip,
    }
}

#[test]
fn decide_path_prefers_active_canon_file_over_non_canon_votes() {
    let candidate = mk_candidate(
        "project.bin",
        Some(mk_canon_observation(
            10,
            ContributingState::LiveFile(LiveFileObservation {
                meta: file_meta("project.bin", "2026-05-30_12-00-00_000000Z", 5),
                snapshot: LiveFileSnapshotState::New,
            }),
        )),
        vec![
            mk_live_file(11, "project.bin", "2026-05-30_11-59-55_000000Z", 99),
            mk_live_directory(12, "project.bin", "2026-05-30_12-00-10_000000Z"),
            mk_tombstone(13, "2026-05-30_12-00-20_000000Z"),
        ],
        Vec::new(),
    );

    let result = decision::decide_path(mk_input(candidate, 1, false, None));

    match result {
        DecisionOutcome::CanonFile(FileDecision {
            source_peer_id,
            path,
            winning_meta,
            reason: decision::FileDecisionReason::Canon,
        }) => {
            assert_eq!(source_peer_id, 10);
            assert_eq!(path, test_path("project.bin"));
            assert_eq!(winning_meta.byte_size, 5);
        }
        _ => panic!("expected canon file outcome"),
    }
}

#[test]
fn decide_path_prefers_active_canon_directory_over_non_canon_votes() {
    let candidate = mk_candidate(
        "dir",
        Some(mk_canon_observation(
            8,
            ContributingState::LiveDirectory(LiveDirectoryObservation {
                meta: dir_meta("dir", "2026-05-30_12-00-00_000000Z"),
                previous: None,
            }),
        )),
        vec![
            mk_live_directory(1, "dir", "2026-05-30_12-00-01_000000Z"),
            mk_live_file(2, "dir", "2026-05-30_12-00-05_000000Z", 10),
        ],
        Vec::new(),
    );

    let result = decision::decide_path(mk_input(candidate, 1, false, None));

    match result {
        DecisionOutcome::CanonDirectory(DirectoryDecision {
            path,
            reason: decision::DirectoryDecisionReason::Canon,
        }) => assert_eq!(path, test_path("dir")),
        _ => panic!("expected canon directory outcome"),
    }
}

#[test]
fn decide_path_prefers_active_canon_absence_when_canon_absent() {
    let candidate = mk_candidate(
        "missing.txt",
        Some(mk_canon_observation(7, ContributingState::NoVote)),
        vec![mk_live_file(1, "other", "2026-05-30_12-00-00_000000Z", 5)],
        Vec::new(),
    );

    let result = decision::decide_path(mk_input(candidate, 1, false, None));

    match result {
        DecisionOutcome::CanonAbsence(AbsenceDecision {
            path,
            reason: decision::AbsenceDecisionReason::Canon,
        }) => assert_eq!(path, test_path("missing.txt")),
        _ => panic!("expected canon absence outcome"),
    }
}

#[test]
fn decide_path_uses_type_conflict_file_when_live_directory_and_live_file_compete_without_canon() {
    let candidate = mk_candidate(
        "entry.txt",
        None,
        vec![
            mk_live_file(1, "entry.txt", "2026-05-30_12-00-00_000000Z", 8),
            mk_live_directory(2, "entry.txt", "2026-05-30_12-00-01_000000Z"),
        ],
        Vec::new(),
    );

    let result = decision::decide_path(mk_input(candidate, 0, false, None));

    match result {
        DecisionOutcome::TypeConflictFile(FileDecision {
            source_peer_id,
            winning_meta,
            reason: decision::FileDecisionReason::TypeConflictFilePreferred,
            ..
        }) => {
            assert_eq!(source_peer_id, 1);
            assert_eq!(winning_meta.byte_size, 8);
        }
        _ => panic!("expected type-conflict file outcome"),
    }
}

#[test]
fn decide_path_prefers_newest_live_file_with_tolerance_and_size_tiebreak() {
    let candidate = mk_candidate(
        "payload.bin",
        None,
        vec![
            mk_live_file(1, "payload.bin", "2026-05-30_12-00-00_000000Z", 200),
            mk_live_file(2, "payload.bin", "2026-05-30_12-00-03_000000Z", 150),
            mk_live_file(3, "payload.bin", "2026-05-30_12-00-03_000000Z", 300),
        ],
        Vec::new(),
    );

    let result = decision::decide_path(mk_input(candidate, 0, false, None));

    match result {
        DecisionOutcome::File(FileDecision {
            source_peer_id,
            winning_meta,
            reason: decision::FileDecisionReason::NewestLiveFile,
            ..
        }) => {
            assert_eq!(source_peer_id, 3);
            assert_eq!(winning_meta.byte_size, 300);
            assert_eq!(winning_meta.kind, EntryKind::File);
        }
        _ => panic!("expected file outcome"),
    }
}

#[test]
fn decide_path_prefers_file_when_deletion_vote_is_not_newer_than_tolerance() {
    let candidate = mk_candidate(
        "notes.txt",
        None,
        vec![
            mk_live_file(1, "notes.txt", "2026-05-30_12-00-00_000000Z", 8),
            mk_absent_unconfirmed(2, "2026-05-30_12-00-05_000000Z"),
        ],
        Vec::new(),
    );

    let result = decision::decide_path(mk_input(candidate, 0, false, None));

    match result {
        DecisionOutcome::File(_) => {}
        _ => panic!("expected file outcome when deletion is within tolerance"),
    }
}

#[test]
fn decide_path_prefers_deterministic_live_file_winner_when_newest_files_tie_in_size() {
    let make_input = || {
        mk_input(
            mk_candidate(
                "tie.bin",
                None,
                vec![
                    mk_live_file(1, "tie.bin", "2026-05-30_12-00-00_000000Z", 321),
                    mk_live_file(2, "tie.bin", "2026-05-30_12-00-00_000000Z", 321),
                ],
                Vec::new(),
            ),
            0,
            false,
            None,
        )
    };

    let first = decision::decide_path(make_input());
    let second = decision::decide_path(make_input());

    assert_eq!(first, second);

    match first {
        DecisionOutcome::File(FileDecision {
            source_peer_id,
            path,
            winning_meta,
            reason: decision::FileDecisionReason::NewestLiveFile,
            ..
        }) => {
            assert_eq!(path, test_path("tie.bin"));
            assert_eq!(winning_meta.byte_size, 321);
            assert_eq!(winning_meta.kind, EntryKind::File);
            assert!(matches!(source_peer_id, 1 | 2));
        }
        _ => panic!("expected file outcome for tied live files"),
    }
}

#[test]
fn decide_path_prefers_absence_from_absent_unconfirmed_history_without_live_files() {
    let candidate = mk_candidate(
        "orphaned.bin",
        None,
        vec![mk_absent_unconfirmed(1, "2026-05-30_12-00-06_000000Z")],
        Vec::new(),
    );

    let result = decision::decide_path(mk_input(candidate, 0, false, None));

    assert!(matches!(
        result,
        DecisionOutcome::Absence(AbsenceDecision { .. })
    ));
}

#[test]
fn decide_path_prefers_deletion_when_vote_is_newer_than_tolerance() {
    let candidate = mk_candidate(
        "notes.txt",
        None,
        vec![
            mk_live_file(1, "notes.txt", "2026-05-30_12-00-00_000000Z", 8),
            mk_absent_unconfirmed(2, "2026-05-30_12-00-06_000000Z"),
        ],
        Vec::new(),
    );

    let result = decision::decide_path(mk_input(candidate, 0, false, None));

    match result {
        DecisionOutcome::Absence(AbsenceDecision {
            path,
            reason: decision::AbsenceDecisionReason::DeletionEstimate,
        }) => assert_eq!(path, test_path("notes.txt")),
        _ => panic!("expected absence when deletion vote is newer than tolerance"),
    }
}

#[test]
fn decide_path_prefers_directory_when_no_live_files_exist() {
    let candidate = mk_candidate(
        "folder",
        None,
        vec![mk_live_directory(
            1,
            "folder",
            "2026-05-30_12-00-00_000000Z",
        )],
        Vec::new(),
    );

    let result = decision::decide_path(mk_input(candidate, 0, false, None));

    match result {
        DecisionOutcome::Directory(DirectoryDecision {
            path,
            reason: decision::DirectoryDecisionReason::ContributingLiveDirectory,
        }) => assert_eq!(path, test_path("folder")),
        _ => panic!("expected directory outcome"),
    }
}

#[test]
fn decide_path_prefers_absence_when_deletion_or_absence_history_is_present() {
    let candidate = mk_candidate(
        "stale.txt",
        None,
        vec![
            mk_tombstone(1, "2026-05-30_12-00-06_000000Z"),
            mk_absent_directory(2, "2026-05-30_11-00-00_000000Z"),
        ],
        Vec::new(),
    );

    let result = decision::decide_path(mk_input(candidate, 0, false, None));

    match result {
        DecisionOutcome::Absence(AbsenceDecision {
            reason: decision::AbsenceDecisionReason::DeletionOrSnapshotHistory,
            ..
        }) => {}
        _ => panic!("expected absence outcome from deletion or history"),
    }
}

#[test]
fn decide_path_prefers_absence_when_newest_deletion_estimate_is_late() {
    let candidate = mk_candidate(
        "rollback.log",
        None,
        vec![
            mk_live_file(1, "rollback.log", "2026-05-30_12-00-00_000000Z", 10),
            mk_absent_unconfirmed(2, "2026-05-30_11-59-55_000000Z"),
            mk_tombstone(3, "2026-05-30_12-00-08_000000Z"),
        ],
        Vec::new(),
    );

    let result = decision::decide_path(mk_input(candidate, 0, false, None));

    match result {
        DecisionOutcome::Absence(AbsenceDecision {
            reason: decision::AbsenceDecisionReason::DeletionEstimate,
            ..
        }) => {}
        _ => panic!("expected absence from most recent deletion estimate"),
    }
}

#[test]
fn decide_path_prefers_no_vote_absence_when_only_no_vote_rows_exist() {
    let candidate = mk_candidate("orphan.txt", None, vec![mk_no_vote(1)], Vec::new());

    let result = decision::decide_path(mk_input(candidate, 0, false, None));

    match result {
        DecisionOutcome::NoVoteAbsence(AbsenceDecision {
            path,
            reason: decision::AbsenceDecisionReason::NoVote,
        }) => assert_eq!(path, test_path("orphan.txt")),
        _ => panic!("expected no-vote absence"),
    }
}

#[test]
fn decide_path_preserves_winning_file_metadata_as_input() {
    let expected_meta = file_meta("exact.txt", "2026-05-30_12-00-03_000000Z", 12345);
    let candidate = mk_candidate(
        "exact.txt",
        None,
        vec![
            mk_live_file(1, "wrong.txt", "2026-05-30_11-00-00_000000Z", 5),
            mk_live_file(
                2,
                "exact.txt",
                &expected_meta.mod_time.0,
                expected_meta.byte_size,
            ),
        ],
        Vec::new(),
    );

    let result = decision::decide_path(mk_input(candidate, 0, false, None));

    match result {
        DecisionOutcome::File(FileDecision {
            winning_meta,
            source_peer_id,
            path,
            ..
        }) => {
            assert_eq!(source_peer_id, 2);
            assert_eq!(path, test_path("exact.txt"));
            assert_eq!(winning_meta, expected_meta);
        }
        _ => panic!("expected file outcome with preserved metadata"),
    }
}

#[test]
fn decide_path_ignores_subordinate_votes_for_outcome() {
    let candidate = mk_candidate(
        "shared-dir",
        None,
        vec![mk_no_vote(1)],
        vec![mk_subordinate_live_file(
            99,
            "shared-dir",
            "2026-05-30_12-00-00_000000Z",
            33,
        )],
    );

    let result = decision::decide_path(mk_input(candidate, 0, false, None));

    assert!(matches!(
        result,
        DecisionOutcome::NoVoteAbsence(AbsenceDecision {
            reason: decision::AbsenceDecisionReason::NoVote,
            ..
        })
    ));
}

#[test]
fn decide_path_reports_invalid_for_multiple_active_canon_peers() {
    let candidate = mk_candidate(
        "x",
        Some(mk_canon_observation(1, ContributingState::NoVote)),
        vec![mk_no_vote(1)],
        Vec::new(),
    );

    let result = decision::decide_path(mk_input(candidate, 2, false, None));

    match result {
        DecisionOutcome::InvalidInput {
            reason: InvalidDecisionInput::MultipleCanonPeers,
            ..
        } => {}
        _ => panic!("expected invalid input for multiple canon peers"),
    }
}

#[test]
fn decide_path_reports_invalid_for_canon_control_without_canon_state() {
    let candidate = mk_candidate("x", None, vec![mk_no_vote(1)], Vec::new());

    let result = decision::decide_path(mk_input(candidate, 1, true, None));

    match result {
        DecisionOutcome::InvalidInput {
            reason: InvalidDecisionInput::CanonControlWithoutCanonState,
            ..
        } => {}
        _ => panic!("expected invalid input for missing canon state"),
    }
}

#[test]
fn decide_path_reports_invalid_when_canon_state_present_without_active_canon() {
    let candidate = mk_candidate(
        "x",
        Some(mk_canon_observation(9, ContributingState::NoVote)),
        vec![mk_no_vote(9)],
        Vec::new(),
    );

    let result = decision::decide_path(mk_input(candidate, 0, false, None));

    match result {
        DecisionOutcome::InvalidInput {
            reason: InvalidDecisionInput::CanonStateWithoutActiveCanon,
            ..
        } => {}
        _ => panic!("expected invalid input when canon state exists without active canon"),
    }
}

#[test]
fn decide_path_reports_invalid_when_no_contributors_and_no_canon_state() {
    let candidate = mk_candidate("x", None, Vec::new(), Vec::new());

    let result = decision::decide_path(mk_input(candidate, 0, false, None));

    match result {
        DecisionOutcome::InvalidInput {
            reason: InvalidDecisionInput::NoActiveContributingPeer,
            ..
        } => {}
        _ => panic!("expected invalid input for missing contributors"),
    }
}

#[test]
fn decide_path_reports_invalid_for_bad_live_file_candidate_metadata() {
    let candidate = mk_candidate(
        "broken.bin",
        None,
        vec![mk_contributor(
            1,
            ContributingState::LiveFile(LiveFileObservation {
                meta: EntryMeta {
                    name: "broken.bin".to_string(),
                    kind: EntryKind::Directory,
                    mod_time: Timestamp("2026-05-30_12-00-00_000000Z".to_string()),
                    byte_size: -1,
                },
                snapshot: LiveFileSnapshotState::New,
            }),
        )],
        Vec::new(),
    );

    let result = decision::decide_path(mk_input(candidate, 0, false, None));

    match result {
        DecisionOutcome::InvalidInput {
            reason: InvalidDecisionInput::FileCandidateMissingMetadata { peer_id },
            ..
        } => assert_eq!(peer_id, 1),
        _ => panic!("expected invalid input for bad live file metadata"),
    }
}

#[test]
fn decide_path_returns_skipped_when_skip_reason_is_set() {
    let candidate = mk_candidate("x", None, Vec::new(), Vec::new());

    let result = decision::decide_path(ClassifiedDecisionInput {
        candidate,
        active_canon_count: 0,
        canon_required: false,
        skip: Some(DecisionSkipReason::TraversalPolicy),
    });

    match result {
        DecisionOutcome::Skipped {
            reason: DecisionSkipReason::TraversalPolicy,
            ..
        } => {}
        _ => panic!("expected skipped decision"),
    }
}

#[test]
fn decide_path_returns_skipped_when_classification_is_unavailable() {
    let candidate = mk_candidate("x", None, Vec::new(), Vec::new());

    let result = decision::decide_path(ClassifiedDecisionInput {
        candidate,
        active_canon_count: 0,
        canon_required: false,
        skip: Some(DecisionSkipReason::ClassificationUnavailable),
    });

    match result {
        DecisionOutcome::Skipped {
            reason: DecisionSkipReason::ClassificationUnavailable,
            ..
        } => {}
        _ => panic!("expected skipped classification-unavailable decision"),
    }
}

#[test]
fn decide_path_is_deterministic_for_identical_input() {
    let candidate = mk_candidate(
        "repeat.txt",
        None,
        vec![
            mk_live_file(1, "repeat.txt", "2026-05-30_12-00-00_000000Z", 1),
            mk_live_directory(2, "repeat.txt", "2026-05-30_12-00-01_000000Z"),
            mk_tombstone(3, "2026-05-30_12-00-06_000000Z"),
        ],
        Vec::new(),
    );

    let first = decision::decide_path(mk_input(candidate, 0, false, None));
    let second = decision::decide_path(mk_input(
        mk_candidate(
            "repeat.txt",
            None,
            vec![
                mk_live_file(1, "repeat.txt", "2026-05-30_12-00-00_000000Z", 1),
                mk_live_directory(2, "repeat.txt", "2026-05-30_12-00-01_000000Z"),
                mk_tombstone(3, "2026-05-30_12-00-06_000000Z"),
            ],
            Vec::new(),
        ),
        0,
        false,
        None,
    ));

    assert_eq!(first, second);
}
