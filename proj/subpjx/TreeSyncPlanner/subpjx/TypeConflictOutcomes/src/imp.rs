use std::sync::Arc;
use crate::api::*;

struct TypeConflictOutcomesImpl;

impl TypeConflictOutcomes for TypeConflictOutcomesImpl {
    fn decide_type_conflict(&self, request: TypeConflictRequest) -> TypeConflictResult {
        if let Some(reason) = validate_request(&request) {
            return invalid_input(request.relative_path, reason);
        }

        let outcome = match request.canon_peer_identity.as_deref() {
            Some(canon_identity) => select_canon_outcome(&request, canon_identity),
            None => select_contributing_outcome(&request),
        };

        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(reason) => return invalid_input(request.relative_path, reason),
        };

        TypeConflictResult::Decision(build_decision(request, outcome))
    }
}

pub fn new() -> std::sync::Arc<dyn TypeConflictOutcomes> {
    Arc::new(TypeConflictOutcomesImpl)
}

enum SelectedOutcome {
    File(TypeConflictSyncSource),
    Directory(TypeConflictSyncSource),
    Absent,
}

fn validate_request(request: &TypeConflictRequest) -> Option<TypeConflictInvalidReason> {
    if request.active_peers.is_empty() {
        return Some(TypeConflictInvalidReason::EmptyPeerSet);
    }

    for (index, peer) in request.active_peers.iter().enumerate() {
        if request.active_peers[index + 1..]
            .iter()
            .any(|other| other.peer_identity == peer.peer_identity)
        {
            return Some(TypeConflictInvalidReason::DuplicatePeerIdentity(
                peer.peer_identity.clone(),
            ));
        }
    }

    if let Some(canon_identity) = &request.canon_peer_identity {
        if !request
            .active_peers
            .iter()
            .any(|peer| peer.peer_identity == *canon_identity)
        {
            return Some(TypeConflictInvalidReason::CanonPeerNotActive(
                canon_identity.clone(),
            ));
        }
    }

    let has_file = request
        .active_peers
        .iter()
        .any(|peer| matches!(&peer.live_entry, TypeConflictLiveEntry::File { .. }));
    let has_directory = request
        .active_peers
        .iter()
        .any(|peer| matches!(&peer.live_entry, TypeConflictLiveEntry::Directory { .. }));

    if !has_file || !has_directory {
        return Some(TypeConflictInvalidReason::NotOneMixedFileDirectoryPath);
    }

    None
}

fn select_canon_outcome(
    request: &TypeConflictRequest,
    canon_identity: &str,
) -> Result<SelectedOutcome, TypeConflictInvalidReason> {
    let Some(canon_peer) = request
        .active_peers
        .iter()
        .find(|peer| peer.peer_identity == canon_identity)
    else {
        return Err(TypeConflictInvalidReason::CanonPeerNotActive(
            canon_identity.to_string(),
        ));
    };

    match &canon_peer.live_entry {
        TypeConflictLiveEntry::File {
            source_relative_path,
        } => {
            if source_relative_path.is_empty() {
                Err(TypeConflictInvalidReason::MissingCanonSource(
                    canon_peer.peer_identity.clone(),
                ))
            } else {
                Ok(SelectedOutcome::File(TypeConflictSyncSource {
                    peer_identity: canon_peer.peer_identity.clone(),
                    source_relative_path: source_relative_path.clone(),
                }))
            }
        }
        TypeConflictLiveEntry::Directory {
            source_relative_path,
        } => {
            if source_relative_path.is_empty() {
                Err(TypeConflictInvalidReason::MissingCanonSource(
                    canon_peer.peer_identity.clone(),
                ))
            } else {
                Ok(SelectedOutcome::Directory(TypeConflictSyncSource {
                    peer_identity: canon_peer.peer_identity.clone(),
                    source_relative_path: source_relative_path.clone(),
                }))
            }
        }
        TypeConflictLiveEntry::Missing => Ok(SelectedOutcome::Absent),
    }
}

fn select_contributing_outcome(
    request: &TypeConflictRequest,
) -> Result<SelectedOutcome, TypeConflictInvalidReason> {
    let has_contributing_file = request.active_peers.iter().any(|peer| {
        peer.role == TypeConflictPeerRole::Contributing
            && matches!(&peer.live_entry, TypeConflictLiveEntry::File { .. })
    });

    if has_contributing_file {
        return request
            .active_peers
            .iter()
            .filter(|peer| {
                peer.role == TypeConflictPeerRole::Contributing
                    && matches!(&peer.live_entry, TypeConflictLiveEntry::File { .. })
            })
            .find_map(source_from_peer)
            .map(SelectedOutcome::File)
            .ok_or(TypeConflictInvalidReason::MissingEligibleContributingSource);
    }

    let has_contributing_directory = request.active_peers.iter().any(|peer| {
        peer.role == TypeConflictPeerRole::Contributing
            && matches!(&peer.live_entry, TypeConflictLiveEntry::Directory { .. })
    });

    if has_contributing_directory {
        return request
            .active_peers
            .iter()
            .filter(|peer| {
                peer.role == TypeConflictPeerRole::Contributing
                    && matches!(&peer.live_entry, TypeConflictLiveEntry::Directory { .. })
            })
            .find_map(source_from_peer)
            .map(SelectedOutcome::Directory)
            .ok_or(TypeConflictInvalidReason::MissingEligibleContributingSource);
    }

    Err(TypeConflictInvalidReason::NoContributingWinningType)
}

fn source_from_peer(peer: &TypeConflictPeerInput) -> Option<TypeConflictSyncSource> {
    match &peer.live_entry {
        TypeConflictLiveEntry::File {
            source_relative_path,
        }
        | TypeConflictLiveEntry::Directory {
            source_relative_path,
        } => {
            if source_relative_path.is_empty() {
                None
            } else {
                Some(TypeConflictSyncSource {
                    peer_identity: peer.peer_identity.clone(),
                    source_relative_path: source_relative_path.clone(),
                })
            }
        }
        TypeConflictLiveEntry::Missing => None,
    }
}

fn build_decision(request: TypeConflictRequest, outcome: SelectedOutcome) -> TypeConflictDecision {
    let relative_path = request.relative_path;
    let mut peer_decisions = Vec::with_capacity(request.active_peers.len());
    let mut displacement_intents = Vec::new();
    let mut replacement_intents = Vec::new();
    let mut directory_recursion_peers = Vec::new();

    let group_outcome = match &outcome {
        SelectedOutcome::File(source) => TypeConflictGroupOutcome::File {
            source: source.clone(),
        },
        SelectedOutcome::Directory(source) => TypeConflictGroupOutcome::Directory {
            source: source.clone(),
        },
        SelectedOutcome::Absent => TypeConflictGroupOutcome::Absent,
    };

    for peer in request.active_peers {
        let disposition = disposition_for_peer(&peer.live_entry, &outcome);

        if peer.is_active_target {
            add_intents_for_peer(
                &relative_path,
                &peer,
                &outcome,
                &mut displacement_intents,
                &mut replacement_intents,
            );

            if matches!(
                disposition,
                TypeConflictPeerDisposition::KeepsWinningDirectory
                    | TypeConflictPeerDisposition::ReceivesWinningDirectory
                    | TypeConflictPeerDisposition::DisplacesFileThenReceivesDirectory
            ) {
                directory_recursion_peers.push(peer.peer_identity.clone());
            }
        }

        peer_decisions.push(TypeConflictPeerDecision {
            peer_identity: peer.peer_identity,
            role: peer.role,
            live_entry: peer.live_entry,
            disposition,
        });
    }

    let directory_recursion = match outcome {
        SelectedOutcome::Directory(_) => Some(TypeConflictDirectoryRecursion {
            relative_path: relative_path.clone(),
            peer_identities: directory_recursion_peers,
        }),
        SelectedOutcome::File(_) | SelectedOutcome::Absent => None,
    };

    TypeConflictDecision {
        relative_path,
        group_outcome,
        peer_decisions,
        displacement_intents,
        replacement_intents,
        directory_recursion,
    }
}

fn disposition_for_peer(
    live_entry: &TypeConflictLiveEntry,
    outcome: &SelectedOutcome,
) -> TypeConflictPeerDisposition {
    match outcome {
        SelectedOutcome::File(_) => match live_entry {
            TypeConflictLiveEntry::File { .. } => TypeConflictPeerDisposition::KeepsWinningFile,
            TypeConflictLiveEntry::Directory { .. } => {
                TypeConflictPeerDisposition::DisplacesDirectoryThenReceivesFile
            }
            TypeConflictLiveEntry::Missing => TypeConflictPeerDisposition::ReceivesWinningFile,
        },
        SelectedOutcome::Directory(_) => match live_entry {
            TypeConflictLiveEntry::File { .. } => {
                TypeConflictPeerDisposition::DisplacesFileThenReceivesDirectory
            }
            TypeConflictLiveEntry::Directory { .. } => {
                TypeConflictPeerDisposition::KeepsWinningDirectory
            }
            TypeConflictLiveEntry::Missing => TypeConflictPeerDisposition::ReceivesWinningDirectory,
        },
        SelectedOutcome::Absent => match live_entry {
            TypeConflictLiveEntry::File { .. } => {
                TypeConflictPeerDisposition::DisplacesFileForAbsence
            }
            TypeConflictLiveEntry::Directory { .. } => {
                TypeConflictPeerDisposition::DisplacesDirectoryForAbsence
            }
            TypeConflictLiveEntry::Missing => TypeConflictPeerDisposition::AlreadyAbsent,
        },
    }
}

fn add_intents_for_peer(
    relative_path: &str,
    peer: &TypeConflictPeerInput,
    outcome: &SelectedOutcome,
    displacement_intents: &mut Vec<TypeConflictDisplacementIntent>,
    replacement_intents: &mut Vec<TypeConflictReplacementIntent>,
) {
    match outcome {
        SelectedOutcome::File(source) => match &peer.live_entry {
            TypeConflictLiveEntry::File { .. } => {}
            TypeConflictLiveEntry::Directory { .. } => {
                displacement_intents.push(TypeConflictDisplacementIntent {
                    peer_identity: peer.peer_identity.clone(),
                    relative_path: relative_path.to_string(),
                    kind: TypeConflictDisplacementKind::DirectoryWholeSubtree,
                });
                push_file_replacement(relative_path, peer, source, replacement_intents);
            }
            TypeConflictLiveEntry::Missing => {
                push_file_replacement(relative_path, peer, source, replacement_intents);
            }
        },
        SelectedOutcome::Directory(source) => match &peer.live_entry {
            TypeConflictLiveEntry::File { .. } => {
                displacement_intents.push(TypeConflictDisplacementIntent {
                    peer_identity: peer.peer_identity.clone(),
                    relative_path: relative_path.to_string(),
                    kind: TypeConflictDisplacementKind::File,
                });
                push_directory_replacement(relative_path, peer, source, replacement_intents);
            }
            TypeConflictLiveEntry::Directory { .. } => {}
            TypeConflictLiveEntry::Missing => {
                push_directory_replacement(relative_path, peer, source, replacement_intents);
            }
        },
        SelectedOutcome::Absent => match &peer.live_entry {
            TypeConflictLiveEntry::File { .. } => {
                displacement_intents.push(TypeConflictDisplacementIntent {
                    peer_identity: peer.peer_identity.clone(),
                    relative_path: relative_path.to_string(),
                    kind: TypeConflictDisplacementKind::File,
                });
            }
            TypeConflictLiveEntry::Directory { .. } => {
                displacement_intents.push(TypeConflictDisplacementIntent {
                    peer_identity: peer.peer_identity.clone(),
                    relative_path: relative_path.to_string(),
                    kind: TypeConflictDisplacementKind::DirectoryWholeSubtree,
                });
            }
            TypeConflictLiveEntry::Missing => {}
        },
    }
}

fn push_file_replacement(
    relative_path: &str,
    peer: &TypeConflictPeerInput,
    source: &TypeConflictSyncSource,
    replacement_intents: &mut Vec<TypeConflictReplacementIntent>,
) {
    replacement_intents.push(TypeConflictReplacementIntent::SyncFile {
        source_peer_identity: source.peer_identity.clone(),
        source_relative_path: source.source_relative_path.clone(),
        destination_peer_identity: peer.peer_identity.clone(),
        destination_relative_path: relative_path.to_string(),
    });
}

fn push_directory_replacement(
    relative_path: &str,
    peer: &TypeConflictPeerInput,
    source: &TypeConflictSyncSource,
    replacement_intents: &mut Vec<TypeConflictReplacementIntent>,
) {
    replacement_intents.push(TypeConflictReplacementIntent::SyncDirectory {
        source_peer_identity: source.peer_identity.clone(),
        source_relative_path: source.source_relative_path.clone(),
        destination_peer_identity: peer.peer_identity.clone(),
        destination_relative_path: relative_path.to_string(),
    });
}

fn invalid_input(
    relative_path: String,
    reason: TypeConflictInvalidReason,
) -> TypeConflictResult {
    TypeConflictResult::InvalidInput(TypeConflictInvalidInput {
        relative_path,
        reason,
    })
}
