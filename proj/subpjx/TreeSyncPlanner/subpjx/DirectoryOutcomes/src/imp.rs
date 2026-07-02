use std::sync::Arc;
use std::time::Duration;

use crate::api::*;

struct DirectoryOutcomesImpl;

const DELETION_TOLERANCE: Duration = Duration::from_secs(5);

impl DirectoryOutcomes for DirectoryOutcomesImpl {
    fn decide_directory(&self, request: DirectoryOutcomeRequest) -> DirectoryOutcomeResult {
        if request.active_peers.is_empty() {
            return invalid(
                request.relative_path.clone(),
                DirectoryOutcomeInvalidReason::EmptyPeerSet,
            );
        }

        for (index, peer) in request.active_peers.iter().enumerate() {
            if request.active_peers[index + 1..]
                .iter()
                .any(|other| other.peer_identity == peer.peer_identity)
            {
                return invalid(
                    request.relative_path.clone(),
                    DirectoryOutcomeInvalidReason::DuplicatePeerIdentity(
                        peer.peer_identity.clone(),
                    ),
                );
            }
        }

        if let Some(canon_peer_identity) = request.canon_peer_identity.as_deref() {
            let Some(canon_peer) = request
                .active_peers
                .iter()
                .find(|peer| peer.peer_identity == canon_peer_identity)
            else {
                return invalid(
                    request.relative_path.clone(),
                    DirectoryOutcomeInvalidReason::CanonPeerNotActive(
                        canon_peer_identity.to_string(),
                    ),
                );
            };

            if request.survival_evidence != DirectorySurvivalEvidence::NotNeeded {
                return invalid(
                    request.relative_path.clone(),
                    DirectoryOutcomeInvalidReason::SurvivalEvidenceSuppliedForNonConflict,
                );
            }

            if canon_peer.has_live_directory {
                return exists_decision(request.relative_path, &request.active_peers);
            }

            return absent_with_active_displacements(request.relative_path, &request.active_peers);
        }

        let contributing: Vec<&DirectoryPeerInput> = request
            .active_peers
            .iter()
            .filter(|peer| peer.role == DirectoryPeerRole::Contributing)
            .collect();
        let live_contributing_count = contributing
            .iter()
            .filter(|peer| peer.has_live_directory)
            .count();
        let snapshot_contributing_count = contributing
            .iter()
            .filter(|peer| peer.snapshot.is_some())
            .count();
        let voting_contributing: Vec<&DirectoryPeerInput> = contributing
            .iter()
            .copied()
            .filter(|peer| peer.has_live_directory || peer.snapshot.is_some())
            .collect();
        let absent_voting: Vec<&DirectoryPeerInput> = voting_contributing
            .iter()
            .copied()
            .filter(|peer| !peer.has_live_directory)
            .collect();

        let is_live_directory_conflict =
            live_contributing_count > 0 && !absent_voting.is_empty();

        if !is_live_directory_conflict
            && request.survival_evidence != DirectorySurvivalEvidence::NotNeeded
        {
            return invalid(
                request.relative_path.clone(),
                DirectoryOutcomeInvalidReason::SurvivalEvidenceSuppliedForNonConflict,
            );
        }

        if !voting_contributing.is_empty() && absent_voting.is_empty() {
            return exists_decision(request.relative_path, &request.active_peers);
        }

        if is_live_directory_conflict {
            if request.survival_evidence == DirectorySurvivalEvidence::NotNeeded {
                return invalid(
                    request.relative_path.clone(),
                    DirectoryOutcomeInvalidReason::SurvivalEvidenceMissingForLiveDirectoryConflict,
                );
            }

            let mut newest_deletion_estimate = None;
            for peer in absent_voting {
                let Some(snapshot) = &peer.snapshot else {
                    return invalid(
                        request.relative_path.clone(),
                        DirectoryOutcomeInvalidReason::MissingContributingPeerDeletionEstimate(
                            peer.peer_identity.clone(),
                        ),
                    );
                };
                let estimate = if let Some(deleted_time) = snapshot.deleted_time {
                    deleted_time
                } else if let Some(last_seen) = snapshot.last_seen {
                    last_seen
                } else {
                    return invalid(
                        request.relative_path.clone(),
                        DirectoryOutcomeInvalidReason::MissingContributingPeerDeletionEstimate(
                            peer.peer_identity.clone(),
                        ),
                    );
                };
                newest_deletion_estimate = newest_deletion_estimate
                    .map(|newest| newest.max(estimate))
                    .or(Some(estimate));
            }

            return match request.survival_evidence {
                DirectorySurvivalEvidence::CollectionFailed { .. } => {
                    DirectoryOutcomeResult::SubtreeBlocked(DirectorySubtreeBlock {
                        relative_path: request.relative_path,
                        blocked_peer_identities: request
                            .active_peers
                            .iter()
                            .map(|peer| peer.peer_identity.clone())
                            .collect(),
                        reason: DirectorySubtreeBlockReason::SurvivalEvidenceCollectionFailed,
                    })
                }
                DirectorySurvivalEvidence::NoLiveFiles => {
                    absent_with_active_displacements(request.relative_path, &request.active_peers)
                }
                DirectorySurvivalEvidence::NewestLiveFile { modification_time } => {
                    let deletion_wins = newest_deletion_estimate
                        .and_then(|estimate| estimate.duration_since(modification_time).ok())
                        .map(|age| age > DELETION_TOLERANCE)
                        .unwrap_or(false);

                    if deletion_wins {
                        absent_with_active_displacements(
                            request.relative_path,
                            &request.active_peers,
                        )
                    } else {
                        exists_decision(request.relative_path, &request.active_peers)
                    }
                }
                DirectorySurvivalEvidence::NotNeeded => unreachable!(),
            };
        }

        if live_contributing_count == 0 && snapshot_contributing_count > 0 {
            return absent_with_active_displacements(request.relative_path, &request.active_peers);
        }

        absent_without_contributing_history(request.relative_path, &request.active_peers)
    }
}

fn invalid(
    relative_path: String,
    reason: DirectoryOutcomeInvalidReason,
) -> DirectoryOutcomeResult {
    DirectoryOutcomeResult::InvalidInput(DirectoryOutcomeInvalidInput {
        relative_path,
        reason,
    })
}

fn exists_decision(
    relative_path: String,
    peers: &[DirectoryPeerInput],
) -> DirectoryOutcomeResult {
    let mut creation_intents = Vec::new();
    let mut recursion_peer_identities = Vec::new();
    let mut peer_outcomes = Vec::new();

    for peer in peers {
        let outcome = if peer.has_live_directory {
            recursion_peer_identities.push(peer.peer_identity.clone());
            DirectoryPeerDirectoryOutcome::KeepsDirectory
        } else if peer.is_active_target {
            creation_intents.push(DirectoryCreationIntent {
                peer_identity: peer.peer_identity.clone(),
                relative_path: relative_path.clone(),
            });
            recursion_peer_identities.push(peer.peer_identity.clone());
            DirectoryPeerDirectoryOutcome::CreateDirectory
        } else {
            DirectoryPeerDirectoryOutcome::DirectoryAbsent
        };

        peer_outcomes.push(DirectoryPeerOutcome {
            peer_identity: peer.peer_identity.clone(),
            outcome,
        });
    }

    DirectoryOutcomeResult::Decision(DirectoryOutcomeDecision {
        relative_path: relative_path.clone(),
        group_outcome: DirectoryGroupOutcome::Exists,
        peer_outcomes,
        creation_intents,
        displacement_intents: Vec::new(),
        recursion: Some(DirectoryRecursion {
            relative_path,
            peer_identities: recursion_peer_identities,
        }),
    })
}

fn absent_with_active_displacements(
    relative_path: String,
    peers: &[DirectoryPeerInput],
) -> DirectoryOutcomeResult {
    absent_decision(relative_path, peers, DisplacementScope::ActiveTargets)
}

fn absent_without_contributing_history(
    relative_path: String,
    peers: &[DirectoryPeerInput],
) -> DirectoryOutcomeResult {
    absent_decision(relative_path, peers, DisplacementScope::SubordinateTargets)
}

enum DisplacementScope {
    ActiveTargets,
    SubordinateTargets,
}

fn absent_decision(
    relative_path: String,
    peers: &[DirectoryPeerInput],
    displacement_scope: DisplacementScope,
) -> DirectoryOutcomeResult {
    let mut displacement_intents = Vec::new();
    let mut peer_outcomes = Vec::new();

    for peer in peers {
        let should_displace = peer.has_live_directory
            && peer.is_active_target
            && match displacement_scope {
                DisplacementScope::ActiveTargets => true,
                DisplacementScope::SubordinateTargets => {
                    peer.role == DirectoryPeerRole::Subordinate
                }
            };

        if should_displace {
            displacement_intents.push(DirectoryDisplacementIntent {
                peer_identity: peer.peer_identity.clone(),
                relative_path: relative_path.clone(),
                ordering: DirectoryDisplacementOrdering::WholeDirectoryPreOrder,
            });
        }

        peer_outcomes.push(DirectoryPeerOutcome {
            peer_identity: peer.peer_identity.clone(),
            outcome: if should_displace {
                DirectoryPeerDirectoryOutcome::DisplaceDirectory
            } else {
                DirectoryPeerDirectoryOutcome::DirectoryAbsent
            },
        });
    }

    DirectoryOutcomeResult::Decision(DirectoryOutcomeDecision {
        relative_path,
        group_outcome: DirectoryGroupOutcome::Absent,
        peer_outcomes,
        creation_intents: Vec::new(),
        displacement_intents,
        recursion: None,
    })
}

pub fn new() -> std::sync::Arc<dyn DirectoryOutcomes> {
    Arc::new(DirectoryOutcomesImpl)
}
