use std::sync::Arc;

use treesyncplanner_typeconflictoutcomes::{
    new, TypeConflictDecision, TypeConflictDisplacementIntent,
    TypeConflictDisplacementKind, TypeConflictGroupOutcome, TypeConflictLiveEntry,
    TypeConflictInvalidReason, TypeConflictOutcomes, TypeConflictPeerDecision,
    TypeConflictPeerDisposition, TypeConflictPeerInput, TypeConflictPeerRole,
    TypeConflictReplacementIntent, TypeConflictRequest, TypeConflictResult,
    TypeConflictSyncSource,
};

fn subject() -> Arc<dyn TypeConflictOutcomes> {
    new()
}

fn file(source_relative_path: &str) -> TypeConflictLiveEntry {
    TypeConflictLiveEntry::File {
        source_relative_path: source_relative_path.to_string(),
    }
}

fn directory(source_relative_path: &str) -> TypeConflictLiveEntry {
    TypeConflictLiveEntry::Directory {
        source_relative_path: source_relative_path.to_string(),
    }
}

fn contributing(peer_identity: &str, live_entry: TypeConflictLiveEntry) -> TypeConflictPeerInput {
    peer(
        peer_identity,
        TypeConflictPeerRole::Contributing,
        true,
        live_entry,
    )
}

fn subordinate(peer_identity: &str, live_entry: TypeConflictLiveEntry) -> TypeConflictPeerInput {
    peer(
        peer_identity,
        TypeConflictPeerRole::Subordinate,
        true,
        live_entry,
    )
}

fn peer(
    peer_identity: &str,
    role: TypeConflictPeerRole,
    is_active_target: bool,
    live_entry: TypeConflictLiveEntry,
) -> TypeConflictPeerInput {
    TypeConflictPeerInput {
        peer_identity: peer_identity.to_string(),
        role,
        is_active_target,
        live_entry,
    }
}

fn request(
    active_peers: Vec<TypeConflictPeerInput>,
    canon_peer_identity: Option<&str>,
) -> TypeConflictRequest {
    TypeConflictRequest {
        relative_path: "Recipes/Menu".to_string(),
        active_peers,
        canon_peer_identity: canon_peer_identity.map(str::to_string),
    }
}

fn decision(result: TypeConflictResult) -> TypeConflictDecision {
    match result {
        TypeConflictResult::Decision(decision) => decision,
        other => panic!("expected type conflict decision, got {other:?}"),
    }
}

fn assert_invalid_reason(result: TypeConflictResult, expected_reason: TypeConflictInvalidReason) {
    match result {
        TypeConflictResult::InvalidInput(invalid) => {
            assert_eq!(invalid.relative_path, "Recipes/Menu");
            assert!(
                std::mem::discriminant(&invalid.reason)
                    == std::mem::discriminant(&expected_reason),
                "expected invalid reason {expected_reason:?}, got {:?}",
                invalid.reason
            );
        }
        TypeConflictResult::Decision(decision) => {
            panic!("expected invalid input {expected_reason:?}, got {decision:?}")
        }
    }
}

fn peer_decision<'a>(
    decision: &'a TypeConflictDecision,
    peer_identity: &str,
) -> &'a TypeConflictPeerDecision {
    decision
        .peer_decisions
        .iter()
        .find(|peer_decision| peer_decision.peer_identity == peer_identity)
        .unwrap_or_else(|| panic!("missing peer decision for {peer_identity}"))
}

fn assert_displacement_intents(
    decision: &TypeConflictDecision,
    expected: &[(&str, TypeConflictDisplacementKind)],
) {
    let mut actual = decision
        .displacement_intents
        .iter()
        .map(displacement_key)
        .collect::<Vec<_>>();
    let mut expected = expected
        .iter()
        .map(|(peer_identity, kind)| (peer_identity.to_string(), *kind))
        .collect::<Vec<_>>();

    actual.sort_by_key(displacement_sort_key);
    expected.sort_by_key(displacement_sort_key);
    assert_eq!(actual, expected);
}

fn displacement_key(
    intent: &TypeConflictDisplacementIntent,
) -> (String, TypeConflictDisplacementKind) {
    assert_eq!(intent.relative_path, "Recipes/Menu");
    (intent.peer_identity.clone(), intent.kind)
}

fn displacement_sort_key(
    (peer_identity, kind): &(String, TypeConflictDisplacementKind),
) -> (String, u8) {
    let kind_order = match kind {
        TypeConflictDisplacementKind::File => 0,
        TypeConflictDisplacementKind::DirectoryWholeSubtree => 1,
    };
    (peer_identity.clone(), kind_order)
}

fn assert_replacement_intents(
    decision: &TypeConflictDecision,
    expected: &[TypeConflictReplacementIntent],
) {
    let mut actual = decision.replacement_intents.clone();
    let mut expected = expected.to_vec();

    actual.sort_by_key(replacement_key);
    expected.sort_by_key(replacement_key);
    assert_eq!(actual, expected);
}

fn assert_directory_recursion(
    decision: &TypeConflictDecision,
    expected_peer_identities: &[&str],
) {
    let recursion = decision
        .directory_recursion
        .as_ref()
        .expect("expected directory recursion");
    assert_eq!(recursion.relative_path, "Recipes/Menu");

    let mut actual = recursion.peer_identities.clone();
    let mut expected = expected_peer_identities
        .iter()
        .map(|peer_identity| peer_identity.to_string())
        .collect::<Vec<_>>();

    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
}

fn replacement_key(intent: &TypeConflictReplacementIntent) -> (String, String, String, String) {
    match intent {
        TypeConflictReplacementIntent::SyncFile {
            source_peer_identity,
            source_relative_path,
            destination_peer_identity,
            destination_relative_path,
        }
        | TypeConflictReplacementIntent::SyncDirectory {
            source_peer_identity,
            source_relative_path,
            destination_peer_identity,
            destination_relative_path,
        } => (
            source_peer_identity.clone(),
            source_relative_path.clone(),
            destination_peer_identity.clone(),
            destination_relative_path.clone(),
        ),
    }
}

fn sync_file(
    source_peer_identity: &str,
    source_relative_path: &str,
    destination_peer_identity: &str,
) -> TypeConflictReplacementIntent {
    TypeConflictReplacementIntent::SyncFile {
        source_peer_identity: source_peer_identity.to_string(),
        source_relative_path: source_relative_path.to_string(),
        destination_peer_identity: destination_peer_identity.to_string(),
        destination_relative_path: "Recipes/Menu".to_string(),
    }
}

fn sync_directory(
    source_peer_identity: &str,
    source_relative_path: &str,
    destination_peer_identity: &str,
) -> TypeConflictReplacementIntent {
    TypeConflictReplacementIntent::SyncDirectory {
        source_peer_identity: source_peer_identity.to_string(),
        source_relative_path: source_relative_path.to_string(),
        destination_peer_identity: destination_peer_identity.to_string(),
        destination_relative_path: "Recipes/Menu".to_string(),
    }
}

fn source(peer_identity: &str, source_relative_path: &str) -> TypeConflictSyncSource {
    TypeConflictSyncSource {
        peer_identity: peer_identity.to_string(),
        source_relative_path: source_relative_path.to_string(),
    }
}

#[test]
fn canon_file_displaces_directories_and_syncs_exact_case_file_to_targets() {
    let result = subject().decide_type_conflict(request(
        vec![
            contributing("canon", file("Recipes/MENU.File")),
            contributing("directory_peer", directory("Recipes/menu")),
            subordinate("missing_subordinate", TypeConflictLiveEntry::Missing),
            peer(
                "inactive_directory",
                TypeConflictPeerRole::Subordinate,
                false,
                directory("Recipes/menu"),
            ),
        ],
        Some("canon"),
    ));

    let decision = decision(result);
    assert_eq!(decision.relative_path, "Recipes/Menu");
    assert_eq!(
        decision.group_outcome,
        TypeConflictGroupOutcome::File {
            source: source("canon", "Recipes/MENU.File")
        }
    );
    assert_eq!(
        peer_decision(&decision, "canon").disposition,
        TypeConflictPeerDisposition::KeepsWinningFile
    );
    assert_eq!(
        peer_decision(&decision, "directory_peer").disposition,
        TypeConflictPeerDisposition::DisplacesDirectoryThenReceivesFile
    );
    assert_eq!(
        peer_decision(&decision, "missing_subordinate").disposition,
        TypeConflictPeerDisposition::ReceivesWinningFile
    );
    assert_displacement_intents(
        &decision,
        &[("directory_peer", TypeConflictDisplacementKind::DirectoryWholeSubtree)],
    );
    assert_replacement_intents(
        &decision,
        &[
            sync_file("canon", "Recipes/MENU.File", "directory_peer"),
            sync_file("canon", "Recipes/MENU.File", "missing_subordinate"),
        ],
    );
    assert!(decision.directory_recursion.is_none());
}

#[test]
fn canon_directory_displaces_files_and_syncs_exact_case_directory_to_targets() {
    let result = subject().decide_type_conflict(request(
        vec![
            contributing("canon", directory("Recipes/MENU.Directory")),
            contributing("file_peer", file("Recipes/menu")),
            subordinate("missing_subordinate", TypeConflictLiveEntry::Missing),
        ],
        Some("canon"),
    ));

    let decision = decision(result);
    assert_eq!(
        decision.group_outcome,
        TypeConflictGroupOutcome::Directory {
            source: source("canon", "Recipes/MENU.Directory")
        }
    );
    assert_eq!(
        peer_decision(&decision, "canon").disposition,
        TypeConflictPeerDisposition::KeepsWinningDirectory
    );
    assert_eq!(
        peer_decision(&decision, "file_peer").disposition,
        TypeConflictPeerDisposition::DisplacesFileThenReceivesDirectory
    );
    assert_eq!(
        peer_decision(&decision, "missing_subordinate").disposition,
        TypeConflictPeerDisposition::ReceivesWinningDirectory
    );
    assert_displacement_intents(
        &decision,
        &[("file_peer", TypeConflictDisplacementKind::File)],
    );
    assert_replacement_intents(
        &decision,
        &[
            sync_directory("canon", "Recipes/MENU.Directory", "file_peer"),
            sync_directory("canon", "Recipes/MENU.Directory", "missing_subordinate"),
        ],
    );
    assert_directory_recursion(
        &decision,
        &["canon", "file_peer", "missing_subordinate"],
    );
}

#[test]
fn missing_canon_path_displaces_live_entries_without_replacement_or_recursion() {
    let result = subject().decide_type_conflict(request(
        vec![
            contributing("canon", TypeConflictLiveEntry::Missing),
            contributing("file_peer", file("Recipes/Menu")),
            subordinate("directory_subordinate", directory("Recipes/Menu")),
            contributing("already_missing", TypeConflictLiveEntry::Missing),
        ],
        Some("canon"),
    ));

    let decision = decision(result);
    assert_eq!(decision.group_outcome, TypeConflictGroupOutcome::Absent);
    assert_eq!(
        peer_decision(&decision, "file_peer").disposition,
        TypeConflictPeerDisposition::DisplacesFileForAbsence
    );
    assert_eq!(
        peer_decision(&decision, "directory_subordinate").disposition,
        TypeConflictPeerDisposition::DisplacesDirectoryForAbsence
    );
    assert_eq!(
        peer_decision(&decision, "already_missing").disposition,
        TypeConflictPeerDisposition::AlreadyAbsent
    );
    assert_displacement_intents(
        &decision,
        &[
            ("file_peer", TypeConflictDisplacementKind::File),
            (
                "directory_subordinate",
                TypeConflictDisplacementKind::DirectoryWholeSubtree,
            ),
        ],
    );
    assert!(decision.replacement_intents.is_empty());
    assert!(decision.directory_recursion.is_none());
}

#[test]
fn non_canon_contributing_file_wins_and_subordinate_losing_type_is_replaced() {
    let result = subject().decide_type_conflict(request(
        vec![
            subordinate("subordinate_file", file("Recipes/SubordinateFILE")),
            contributing("contributing_directory", directory("Recipes/Dir")),
            contributing("contributing_file", file("Recipes/WinningFILE")),
            subordinate("subordinate_directory", directory("Recipes/SubDir")),
        ],
        None,
    ));

    let decision = decision(result);
    assert_eq!(
        decision.group_outcome,
        TypeConflictGroupOutcome::File {
            source: source("contributing_file", "Recipes/WinningFILE")
        }
    );
    assert_eq!(
        peer_decision(&decision, "subordinate_file").disposition,
        TypeConflictPeerDisposition::KeepsWinningFile
    );
    assert_eq!(
        peer_decision(&decision, "contributing_directory").disposition,
        TypeConflictPeerDisposition::DisplacesDirectoryThenReceivesFile
    );
    assert_eq!(
        peer_decision(&decision, "subordinate_directory").disposition,
        TypeConflictPeerDisposition::DisplacesDirectoryThenReceivesFile
    );
    assert_displacement_intents(
        &decision,
        &[
            (
                "contributing_directory",
                TypeConflictDisplacementKind::DirectoryWholeSubtree,
            ),
            (
                "subordinate_directory",
                TypeConflictDisplacementKind::DirectoryWholeSubtree,
            ),
        ],
    );
    assert_replacement_intents(
        &decision,
        &[
            sync_file(
                "contributing_file",
                "Recipes/WinningFILE",
                "contributing_directory",
            ),
            sync_file("contributing_file", "Recipes/WinningFILE", "subordinate_directory"),
        ],
    );
    assert!(decision.directory_recursion.is_none());
}

#[test]
fn subordinate_file_does_not_beat_contributing_directory_without_canon_peer() {
    let result = subject().decide_type_conflict(request(
        vec![
            subordinate("subordinate_file", file("Recipes/SubordinateFILE")),
            contributing("contributing_directory", directory("Recipes/WinningDIR")),
        ],
        None,
    ));

    let decision = decision(result);
    assert_eq!(
        decision.group_outcome,
        TypeConflictGroupOutcome::Directory {
            source: source("contributing_directory", "Recipes/WinningDIR")
        }
    );
    assert_eq!(
        peer_decision(&decision, "subordinate_file").disposition,
        TypeConflictPeerDisposition::DisplacesFileThenReceivesDirectory
    );
    assert_displacement_intents(
        &decision,
        &[("subordinate_file", TypeConflictDisplacementKind::File)],
    );
    assert_replacement_intents(
        &decision,
        &[sync_directory(
            "contributing_directory",
            "Recipes/WinningDIR",
            "subordinate_file",
        )],
    );
    assert_directory_recursion(
        &decision,
        &["subordinate_file", "contributing_directory"],
    );
}

#[test]
fn invalid_inputs_return_structured_invalid_reasons_without_deciding() {
    let cases = vec![
        (request(vec![], None), TypeConflictInvalidReason::EmptyPeerSet),
        (
            request(
                vec![
                    contributing("same", file("Recipes/Menu")),
                    contributing("same", directory("Recipes/Menu")),
                ],
                None,
            ),
            TypeConflictInvalidReason::DuplicatePeerIdentity(String::new()),
        ),
        (
            request(
                vec![
                    contributing("file_peer", file("Recipes/Menu")),
                    contributing("directory_peer", directory("Recipes/Menu")),
                ],
                Some("not_active"),
            ),
            TypeConflictInvalidReason::CanonPeerNotActive(String::new()),
        ),
        (
            request(
                vec![
                    contributing("file_peer", file("Recipes/Menu")),
                    contributing("missing_peer", TypeConflictLiveEntry::Missing),
                ],
                None,
            ),
            TypeConflictInvalidReason::NotOneMixedFileDirectoryPath,
        ),
        (
            request(
                vec![
                    contributing("canon", file("")),
                    contributing("directory_peer", directory("Recipes/Menu")),
                ],
                Some("canon"),
            ),
            TypeConflictInvalidReason::MissingCanonSource(String::new()),
        ),
        (
            request(
                vec![
                    contributing("file_peer", file("")),
                    contributing("directory_peer", directory("Recipes/Menu")),
                ],
                None,
            ),
            TypeConflictInvalidReason::MissingEligibleContributingSource,
        ),
        (
            request(
                vec![
                    subordinate("subordinate_file", file("Recipes/Menu")),
                    subordinate("subordinate_directory", directory("Recipes/Menu")),
                ],
                None,
            ),
            TypeConflictInvalidReason::NoContributingWinningType,
        ),
    ];

    for (request, expected_reason) in cases {
        assert_invalid_reason(subject().decide_type_conflict(request), expected_reason);
    }
}
