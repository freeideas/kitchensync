use std::time::{Duration, SystemTime};

use treesyncplanner_directoryoutcomes::{
    new, DirectoryDisplacementOrdering, DirectoryGroupOutcome, DirectoryOutcomeDecision,
    DirectoryOutcomeInvalidReason, DirectoryOutcomeRequest, DirectoryOutcomeResult,
    DirectoryOutcomes, DirectoryPeerDirectoryOutcome, DirectoryPeerInput, DirectoryPeerRole,
    DirectorySnapshotFact, DirectorySubtreeBlockReason, DirectorySurvivalEvidence,
};

fn subject() -> std::sync::Arc<dyn DirectoryOutcomes> {
    new()
}

fn at(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

fn snapshot(
    deleted_time: Option<SystemTime>,
    last_seen: Option<SystemTime>,
) -> DirectorySnapshotFact {
    DirectorySnapshotFact {
        deleted_time,
        last_seen,
    }
}

fn peer(
    peer_identity: &str,
    role: DirectoryPeerRole,
    is_active_target: bool,
    has_live_directory: bool,
    snapshot: Option<DirectorySnapshotFact>,
) -> DirectoryPeerInput {
    DirectoryPeerInput {
        peer_identity: peer_identity.to_string(),
        role,
        is_active_target,
        has_live_directory,
        snapshot,
    }
}

fn contributing(
    peer_identity: &str,
    has_live_directory: bool,
    snapshot: Option<DirectorySnapshotFact>,
) -> DirectoryPeerInput {
    peer(
        peer_identity,
        DirectoryPeerRole::Contributing,
        true,
        has_live_directory,
        snapshot,
    )
}

fn inactive_contributing(
    peer_identity: &str,
    has_live_directory: bool,
    snapshot: Option<DirectorySnapshotFact>,
) -> DirectoryPeerInput {
    peer(
        peer_identity,
        DirectoryPeerRole::Contributing,
        false,
        has_live_directory,
        snapshot,
    )
}

fn subordinate(peer_identity: &str, has_live_directory: bool) -> DirectoryPeerInput {
    peer(
        peer_identity,
        DirectoryPeerRole::Subordinate,
        true,
        has_live_directory,
        None,
    )
}

fn request(
    active_peers: Vec<DirectoryPeerInput>,
    canon_peer_identity: Option<&str>,
    survival_evidence: DirectorySurvivalEvidence,
) -> DirectoryOutcomeRequest {
    DirectoryOutcomeRequest {
        relative_path: "recipes".to_string(),
        active_peers,
        canon_peer_identity: canon_peer_identity.map(str::to_string),
        survival_evidence,
    }
}

fn decision(result: DirectoryOutcomeResult) -> DirectoryOutcomeDecision {
    match result {
        DirectoryOutcomeResult::Decision(decision) => decision,
        other => panic!("expected directory decision, got {other:?}"),
    }
}

fn peer_outcome(
    decision: &DirectoryOutcomeDecision,
    peer_identity: &str,
) -> DirectoryPeerDirectoryOutcome {
    decision
        .peer_outcomes
        .iter()
        .find(|outcome| outcome.peer_identity == peer_identity)
        .unwrap_or_else(|| panic!("missing peer outcome for {peer_identity}"))
        .outcome
}

fn creation_peers(decision: &DirectoryOutcomeDecision) -> Vec<String> {
    decision
        .creation_intents
        .iter()
        .map(|intent| intent.peer_identity.clone())
        .collect()
}

fn displacement_peers(decision: &DirectoryOutcomeDecision) -> Vec<String> {
    decision
        .displacement_intents
        .iter()
        .map(|intent| intent.peer_identity.clone())
        .collect()
}

fn recursion_peers(decision: &DirectoryOutcomeDecision) -> Option<Vec<String>> {
    decision
        .recursion
        .as_ref()
        .map(|recursion| recursion.peer_identities.clone())
}

fn invalid_reason(result: DirectoryOutcomeResult) -> DirectoryOutcomeInvalidReason {
    match result {
        DirectoryOutcomeResult::InvalidInput(invalid) => {
            assert_eq!(invalid.relative_path, "recipes");
            invalid.reason
        }
        other => panic!("expected invalid input, got {other:?}"),
    }
}

fn assert_peer_id_set(actual: Vec<String>, expected: &[&str]) {
    let mut actual = actual;
    let mut expected = expected
        .iter()
        .map(|peer_identity| peer_identity.to_string())
        .collect::<Vec<_>>();
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
}

fn assert_optional_peer_id_set(actual: Option<Vec<String>>, expected: &[&str]) {
    assert_peer_id_set(actual.expect("expected recursion peers"), expected);
}

#[test]
fn missing_peer_that_is_not_an_active_target_is_not_created() {
    let result = subject().decide_directory(request(
        vec![
            contributing("live", true, None),
            contributing("active_missing", false, None),
            inactive_contributing("inactive_missing", false, None),
        ],
        None,
        DirectorySurvivalEvidence::NotNeeded,
    ));

    let decision = decision(result);
    assert_eq!(decision.group_outcome, DirectoryGroupOutcome::Exists);
    assert_eq!(
        peer_outcome(&decision, "active_missing"),
        DirectoryPeerDirectoryOutcome::CreateDirectory
    );
    assert_eq!(
        peer_outcome(&decision, "inactive_missing"),
        DirectoryPeerDirectoryOutcome::DirectoryAbsent
    );
    assert_peer_id_set(creation_peers(&decision), &["active_missing"]);
    assert_optional_peer_id_set(recursion_peers(&decision), &["live", "active_missing"]);
}

#[test]
fn canon_live_directory_exists_on_every_active_target() {
    let result = subject().decide_directory(request(
        vec![
            contributing("canon", true, None),
            contributing("missing", false, None),
            subordinate("subordinate", false),
        ],
        Some("canon"),
        DirectorySurvivalEvidence::NotNeeded,
    ));

    let decision = decision(result);
    assert_eq!(decision.group_outcome, DirectoryGroupOutcome::Exists);
    assert_eq!(
        peer_outcome(&decision, "canon"),
        DirectoryPeerDirectoryOutcome::KeepsDirectory
    );
    assert_eq!(
        peer_outcome(&decision, "missing"),
        DirectoryPeerDirectoryOutcome::CreateDirectory
    );
    assert_eq!(
        peer_outcome(&decision, "subordinate"),
        DirectoryPeerDirectoryOutcome::CreateDirectory
    );
    assert_peer_id_set(creation_peers(&decision), &["missing", "subordinate"]);
    assert!(decision.displacement_intents.is_empty());
    assert_optional_peer_id_set(
        recursion_peers(&decision),
        &["canon", "missing", "subordinate"],
    );
}

#[test]
fn canon_missing_path_displaces_live_active_targets_without_recursion() {
    let result = subject().decide_directory(request(
        vec![
            contributing("canon", false, None),
            contributing("live", true, None),
            subordinate("subordinate", true),
            contributing("already_missing", false, None),
        ],
        Some("canon"),
        DirectorySurvivalEvidence::NotNeeded,
    ));

    let decision = decision(result);
    assert_eq!(decision.group_outcome, DirectoryGroupOutcome::Absent);
    assert_eq!(
        peer_outcome(&decision, "live"),
        DirectoryPeerDirectoryOutcome::DisplaceDirectory
    );
    assert_eq!(
        peer_outcome(&decision, "subordinate"),
        DirectoryPeerDirectoryOutcome::DisplaceDirectory
    );
    assert_eq!(
        peer_outcome(&decision, "already_missing"),
        DirectoryPeerDirectoryOutcome::DirectoryAbsent
    );
    assert!(decision.creation_intents.is_empty());
    assert_peer_id_set(displacement_peers(&decision), &["live", "subordinate"]);
    assert!(decision
        .displacement_intents
        .iter()
        .all(|intent| intent.ordering == DirectoryDisplacementOrdering::WholeDirectoryPreOrder));
    assert!(decision.recursion.is_none());
}

#[test]
fn non_canon_all_voting_contributors_live_makes_directory_exist() {
    let result = subject().decide_directory(request(
        vec![
            contributing(
                "live_with_old_snapshot",
                true,
                Some(snapshot(Some(at(200)), Some(at(150)))),
            ),
            contributing("live_without_snapshot", true, None),
            contributing("non_voting_missing", false, None),
            subordinate("subordinate_missing", false),
        ],
        None,
        DirectorySurvivalEvidence::NotNeeded,
    ));

    let decision = decision(result);
    assert_eq!(decision.group_outcome, DirectoryGroupOutcome::Exists);
    assert_eq!(
        peer_outcome(&decision, "live_with_old_snapshot"),
        DirectoryPeerDirectoryOutcome::KeepsDirectory
    );
    assert_eq!(
        peer_outcome(&decision, "non_voting_missing"),
        DirectoryPeerDirectoryOutcome::CreateDirectory
    );
    assert_eq!(
        peer_outcome(&decision, "subordinate_missing"),
        DirectoryPeerDirectoryOutcome::CreateDirectory
    );
    assert_optional_peer_id_set(
        recursion_peers(&decision),
        &[
            "live_with_old_snapshot",
            "live_without_snapshot",
            "non_voting_missing",
            "subordinate_missing",
        ],
    );
}

#[test]
fn live_directory_conflict_uses_deleted_time_before_last_seen() {
    let result = subject().decide_directory(request(
        vec![
            contributing("live", true, None),
            contributing("absent", false, Some(snapshot(Some(at(1_010)), Some(at(2_000))))),
            contributing("target_missing", false, None),
        ],
        None,
        DirectorySurvivalEvidence::NewestLiveFile {
            modification_time: at(1_008),
        },
    ));

    let decision = decision(result);
    assert_eq!(decision.group_outcome, DirectoryGroupOutcome::Exists);
    assert_eq!(
        peer_outcome(&decision, "target_missing"),
        DirectoryPeerDirectoryOutcome::CreateDirectory
    );
    assert_optional_peer_id_set(
        recursion_peers(&decision),
        &["live", "absent", "target_missing"],
    );
    assert!(decision.displacement_intents.is_empty());
}

#[test]
fn live_directory_conflict_uses_last_seen_when_deleted_time_is_absent() {
    let result = subject().decide_directory(request(
        vec![
            contributing("live", true, None),
            contributing("absent", false, Some(snapshot(None, Some(at(1_020))))),
            contributing("also_live", true, None),
        ],
        None,
        DirectorySurvivalEvidence::NewestLiveFile {
            modification_time: at(1_014),
        },
    ));

    let decision = decision(result);
    assert_eq!(decision.group_outcome, DirectoryGroupOutcome::Absent);
    assert_eq!(
        peer_outcome(&decision, "live"),
        DirectoryPeerDirectoryOutcome::DisplaceDirectory
    );
    assert_eq!(
        peer_outcome(&decision, "also_live"),
        DirectoryPeerDirectoryOutcome::DisplaceDirectory
    );
    assert!(decision.creation_intents.is_empty());
    assert!(decision.recursion.is_none());
}

#[test]
fn live_directory_conflict_uses_newest_deletion_estimate() {
    let result = subject().decide_directory(request(
        vec![
            contributing("live", true, None),
            contributing(
                "old_absent",
                false,
                Some(snapshot(Some(at(1_000)), Some(at(1_000)))),
            ),
            contributing("new_absent", false, Some(snapshot(None, Some(at(1_030))))),
            subordinate("subordinate_live", true),
        ],
        None,
        DirectorySurvivalEvidence::NewestLiveFile {
            modification_time: at(1_024),
        },
    ));

    let decision = decision(result);
    assert_eq!(decision.group_outcome, DirectoryGroupOutcome::Absent);
    assert_peer_id_set(
        displacement_peers(&decision),
        &["live", "subordinate_live"],
    );
    assert!(decision.creation_intents.is_empty());
    assert!(decision.recursion.is_none());
}

#[test]
fn surviving_live_directory_conflict_creates_absent_voting_peers() {
    let result = subject().decide_directory(request(
        vec![
            contributing("live", true, None),
            contributing("absent", false, Some(snapshot(Some(at(1_000)), Some(at(900))))),
        ],
        None,
        DirectorySurvivalEvidence::NewestLiveFile {
            modification_time: at(1_000),
        },
    ));

    let decision = decision(result);
    assert_eq!(decision.group_outcome, DirectoryGroupOutcome::Exists);
    assert_eq!(
        peer_outcome(&decision, "absent"),
        DirectoryPeerDirectoryOutcome::CreateDirectory
    );
    assert_peer_id_set(creation_peers(&decision), &["absent"]);
    assert_optional_peer_id_set(recursion_peers(&decision), &["live", "absent"]);
    assert!(decision.displacement_intents.is_empty());
}

#[test]
fn survival_within_five_second_tolerance_keeps_recursion() {
    let result = subject().decide_directory(request(
        vec![
            contributing("live", true, None),
            contributing("absent", false, Some(snapshot(Some(at(1_005)), Some(at(1_005))))),
            contributing("missing_target", false, None),
        ],
        None,
        DirectorySurvivalEvidence::NewestLiveFile {
            modification_time: at(1_000),
        },
    ));

    let decision = decision(result);
    assert_eq!(decision.group_outcome, DirectoryGroupOutcome::Exists);
    assert_eq!(
        peer_outcome(&decision, "missing_target"),
        DirectoryPeerDirectoryOutcome::CreateDirectory
    );
    assert_optional_peer_id_set(
        recursion_peers(&decision),
        &["live", "absent", "missing_target"],
    );
    assert!(decision.displacement_intents.is_empty());
    // Child file decisions are delegated to file rules and are not directly
    // observable through this interface.
}

#[test]
fn no_live_file_survival_evidence_displaces_live_directories() {
    let result = subject().decide_directory(request(
        vec![
            contributing("live", true, None),
            contributing("absent", false, Some(snapshot(Some(at(1_000)), Some(at(900))))),
            contributing("missing_target", false, None),
        ],
        None,
        DirectorySurvivalEvidence::NoLiveFiles,
    ));

    let decision = decision(result);
    assert_eq!(decision.group_outcome, DirectoryGroupOutcome::Absent);
    assert_eq!(
        peer_outcome(&decision, "live"),
        DirectoryPeerDirectoryOutcome::DisplaceDirectory
    );
    assert_eq!(
        peer_outcome(&decision, "missing_target"),
        DirectoryPeerDirectoryOutcome::DirectoryAbsent
    );
    assert!(decision.creation_intents.is_empty());
    assert!(decision.recursion.is_none());
}

#[test]
fn failed_survival_evidence_collection_blocks_the_subtree_without_intents() {
    let result = subject().decide_directory(request(
        vec![
            contributing("live", true, None),
            contributing("absent", false, Some(snapshot(None, Some(at(1_000))))),
            subordinate("subordinate", true),
        ],
        None,
        DirectorySurvivalEvidence::CollectionFailed {
            failed_peer_identities: vec!["live".to_string()],
        },
    ));

    match result {
        DirectoryOutcomeResult::SubtreeBlocked(block) => {
            assert_eq!(block.relative_path, "recipes");
            assert_peer_id_set(
                block.blocked_peer_identities,
                &["live", "absent", "subordinate"],
            );
            assert_eq!(
                block.reason,
                DirectorySubtreeBlockReason::SurvivalEvidenceCollectionFailed
            );
        }
        other => panic!("expected subtree block, got {other:?}"),
    }
}

#[test]
fn absent_snapshot_history_displaces_active_peers_that_still_have_directory() {
    let result = subject().decide_directory(request(
        vec![
            contributing("deleted_peer", false, Some(snapshot(Some(at(1_000)), Some(at(900))))),
            contributing("missing_peer", false, None),
            subordinate("subordinate_live", true),
            subordinate("subordinate_missing", false),
        ],
        None,
        DirectorySurvivalEvidence::NotNeeded,
    ));

    let decision = decision(result);
    assert_eq!(decision.group_outcome, DirectoryGroupOutcome::Absent);
    assert_eq!(
        peer_outcome(&decision, "subordinate_live"),
        DirectoryPeerDirectoryOutcome::DisplaceDirectory
    );
    assert_peer_id_set(displacement_peers(&decision), &["subordinate_live"]);
    assert!(decision.creation_intents.is_empty());
    assert!(decision.recursion.is_none());
}

#[test]
fn no_contributing_votes_displaces_subordinate_live_directories() {
    let result = subject().decide_directory(request(
        vec![
            contributing("contributing_missing", false, None),
            subordinate("subordinate_live", true),
            subordinate("subordinate_missing", false),
        ],
        None,
        DirectorySurvivalEvidence::NotNeeded,
    ));

    let decision = decision(result);
    assert_eq!(decision.group_outcome, DirectoryGroupOutcome::Absent);
    assert_eq!(
        peer_outcome(&decision, "contributing_missing"),
        DirectoryPeerDirectoryOutcome::DirectoryAbsent
    );
    assert_eq!(
        peer_outcome(&decision, "subordinate_live"),
        DirectoryPeerDirectoryOutcome::DisplaceDirectory
    );
    assert_peer_id_set(displacement_peers(&decision), &["subordinate_live"]);
    assert!(decision.creation_intents.is_empty());
    assert!(decision.recursion.is_none());
    assert_eq!(
        decision.displacement_intents[0].ordering,
        DirectoryDisplacementOrdering::WholeDirectoryPreOrder
    );
}

#[test]
fn empty_peer_set_is_invalid_input() {
    let result =
        subject().decide_directory(request(vec![], None, DirectorySurvivalEvidence::NotNeeded));

    assert_eq!(
        invalid_reason(result),
        DirectoryOutcomeInvalidReason::EmptyPeerSet
    );
}

#[test]
fn duplicate_peer_identity_is_invalid_input() {
    let result = subject().decide_directory(request(
        vec![
            contributing("same_peer", true, None),
            contributing("same_peer", false, None),
        ],
        None,
        DirectorySurvivalEvidence::NotNeeded,
    ));

    assert_eq!(
        invalid_reason(result),
        DirectoryOutcomeInvalidReason::DuplicatePeerIdentity("same_peer".to_string())
    );
}

#[test]
fn canon_peer_that_is_not_active_is_invalid_input() {
    let result = subject().decide_directory(request(
        vec![contributing("active_peer", true, None)],
        Some("missing_canon"),
        DirectorySurvivalEvidence::NotNeeded,
    ));

    assert_eq!(
        invalid_reason(result),
        DirectoryOutcomeInvalidReason::CanonPeerNotActive("missing_canon".to_string())
    );
}

#[test]
fn conflict_absent_voter_without_deletion_estimate_is_invalid_input() {
    let result = subject().decide_directory(request(
        vec![
            contributing("live", true, None),
            contributing("absent_without_estimate", false, Some(snapshot(None, None))),
        ],
        None,
        DirectorySurvivalEvidence::NewestLiveFile {
            modification_time: at(1_000),
        },
    ));

    assert_eq!(
        invalid_reason(result),
        DirectoryOutcomeInvalidReason::MissingContributingPeerDeletionEstimate(
            "absent_without_estimate".to_string()
        )
    );
}

#[test]
fn missing_survival_evidence_for_live_directory_conflict_is_invalid_input() {
    let result = subject().decide_directory(request(
        vec![
            contributing("live", true, None),
            contributing("absent", false, Some(snapshot(Some(at(1_000)), None))),
        ],
        None,
        DirectorySurvivalEvidence::NotNeeded,
    ));

    assert_eq!(
        invalid_reason(result),
        DirectoryOutcomeInvalidReason::SurvivalEvidenceMissingForLiveDirectoryConflict
    );
}

#[test]
fn survival_evidence_for_non_conflict_is_invalid_input() {
    let result = subject().decide_directory(request(
        vec![contributing("live", true, None)],
        None,
        DirectorySurvivalEvidence::NoLiveFiles,
    ));

    assert_eq!(
        invalid_reason(result),
        DirectoryOutcomeInvalidReason::SurvivalEvidenceSuppliedForNonConflict
    );
}
