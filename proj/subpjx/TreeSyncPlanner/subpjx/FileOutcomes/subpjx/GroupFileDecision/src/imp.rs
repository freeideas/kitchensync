use std::sync::Arc;

use crate::api::*;

const TOLERANCE_NANOS: i128 = 5_000_000_000;
const NANOS_PER_SECOND: i128 = 1_000_000_000;

struct GroupFileDecisionImpl;

#[derive(Clone)]
struct LiveCandidate {
    peer_index: usize,
    file: ClassifiedLiveFile,
}

#[derive(Clone)]
struct ExistingWinner {
    byte_size: u64,
    modified_time: SyncTimestamp,
    sources: Vec<usize>,
}

impl GroupFileDecision for GroupFileDecisionImpl {
    fn decide_group_file(
        &self,
        request: GroupFileDecisionRequest,
    ) -> Result<GroupFileDecisionOutput, GroupFileDecisionError> {
        validate_request(&request)?;

        if let Some(canon_index) = canon_peer_index(&request)? {
            return decide_with_canon(request, canon_index);
        }

        decide_without_canon(request)
    }
}

pub fn new() -> std::sync::Arc<dyn GroupFileDecision> {
    Arc::new(GroupFileDecisionImpl)
}

fn validate_request(request: &GroupFileDecisionRequest) -> Result<(), GroupFileDecisionError> {
    if request.relative_path.is_empty() {
        return invalid("relative_path must not be empty");
    }
    if request.peers.is_empty() {
        return invalid("at least one peer is required");
    }

    for peer in &request.peers {
        if peer.peer_id.is_empty() {
            return invalid("peer_id must not be empty");
        }
        validate_classification(&request.relative_path, &peer.classification)?;
    }

    for left in 0..request.peers.len() {
        for right in left + 1..request.peers.len() {
            if request.peers[left].peer_id == request.peers[right].peer_id {
                return invalid("peer_id values must be unique");
            }
        }
    }

    Ok(())
}

fn validate_classification(
    relative_path: &str,
    classification: &PeerFileState,
) -> Result<(), GroupFileDecisionError> {
    match classification {
        PeerFileState::UnchangedLiveFile(file)
        | PeerFileState::ModifiedLiveFile(file)
        | PeerFileState::NewLiveFile(file) => {
            validate_timestamp(file.modified_time)?;
            if file.source_relative_path.is_empty() {
                return invalid("live file source_relative_path must not be empty");
            }
            if file.source_relative_path != relative_path {
                return invalid("live file source_relative_path must match relative_path");
            }
        }
        PeerFileState::DeletedFile {
            deletion_estimate,
        } => validate_timestamp(*deletion_estimate)?,
        PeerFileState::AbsentUnconfirmed { last_seen } => {
            if let Some(last_seen) = last_seen {
                validate_timestamp(*last_seen)?;
            }
        }
        PeerFileState::AbsentNoRowNoVote => {}
    }

    Ok(())
}

fn validate_timestamp(timestamp: SyncTimestamp) -> Result<(), GroupFileDecisionError> {
    if timestamp.nanoseconds >= 1_000_000_000 {
        return invalid("timestamp nanoseconds must be less than 1000000000");
    }

    Ok(())
}

fn canon_peer_index(
    request: &GroupFileDecisionRequest,
) -> Result<Option<usize>, GroupFileDecisionError> {
    let canon_indexes: Vec<usize> = request
        .peers
        .iter()
        .enumerate()
        .filter_map(|(index, peer)| {
            if peer.role == GroupFileDecisionPeerRole::Canon {
                Some(index)
            } else {
                None
            }
        })
        .collect();

    if canon_indexes.len() > 1 {
        return invalid("at most one canon peer is allowed");
    }

    Ok(canon_indexes.first().copied())
}

fn decide_with_canon(
    request: GroupFileDecisionRequest,
    canon_index: usize,
) -> Result<GroupFileDecisionOutput, GroupFileDecisionError> {
    if let Some(file) = live_file(&request.peers[canon_index].classification) {
        let mut sources = vec![canon_index];
        sources.extend(
            matching_live_peer_indexes(&request.peers, file.byte_size, file.modified_time)
                .into_iter()
                .filter(|index| *index != canon_index),
        );

        let winner = ExistingWinner {
            byte_size: file.byte_size,
            modified_time: file.modified_time,
            sources,
        };
        return existing_file_output(request, winner, Some(canon_index));
    }

    let deletion_voters = if matches!(
        request.peers[canon_index].classification,
        PeerFileState::DeletedFile { .. } | PeerFileState::AbsentUnconfirmed { .. }
    ) {
        vec![canon_index]
    } else {
        Vec::new()
    };

    let deletion_estimate = deletion_estimate(&request.peers[canon_index].classification);
    absent_output(
        request,
        FileGroupOutcome::Deletion { deletion_estimate },
        true,
        deletion_voters,
    )
}

fn decide_without_canon(
    request: GroupFileDecisionRequest,
) -> Result<GroupFileDecisionOutput, GroupFileDecisionError> {
    let contributing_indexes: Vec<usize> = request
        .peers
        .iter()
        .enumerate()
        .filter_map(|(index, peer)| {
            if peer.role == GroupFileDecisionPeerRole::Contributing {
                Some(index)
            } else {
                None
            }
        })
        .collect();

    if contributing_indexes.is_empty() {
        return invalid("at least one contributing peer is required without a canon peer");
    }

    let live_candidates = live_candidates(&request.peers, &contributing_indexes);
    let all_contributing_no_row = contributing_indexes
        .iter()
        .all(|index| matches!(request.peers[*index].classification, PeerFileState::AbsentNoRowNoVote));

    if live_candidates.is_empty() && all_contributing_no_row {
        return absent_output(request, FileGroupOutcome::NoFile, false, Vec::new());
    }

    let live_winner = select_live_winner(&live_candidates);
    let deletion_voters = deletion_voter_indexes(&request.peers, &contributing_indexes, live_winner.as_ref());
    let deletion_vote = deletion_voters
        .iter()
        .filter_map(|index| deletion_vote_time(&request.peers[*index].classification))
        .max();

    match (live_winner, deletion_vote) {
        (Some(winner), Some(deletion_estimate))
            if more_than_tolerance_newer(deletion_estimate, winner.modified_time) =>
        {
            absent_output(
                request,
                FileGroupOutcome::Deletion {
                    deletion_estimate: Some(deletion_estimate),
                },
                false,
                deletion_voters,
            )
        }
        (Some(winner), _) => existing_file_output(request, winner, None),
        (None, Some(deletion_estimate)) => absent_output(
            request,
            FileGroupOutcome::Deletion {
                deletion_estimate: Some(deletion_estimate),
            },
            false,
            deletion_voters,
        ),
        (None, None) => invalid("contributing peers produced no coherent group decision"),
    }
}

fn existing_file_output(
    request: GroupFileDecisionRequest,
    winner: ExistingWinner,
    canon_index: Option<usize>,
) -> Result<GroupFileDecisionOutput, GroupFileDecisionError> {
    let source_index = winner
        .sources
        .first()
        .copied()
        .ok_or_else(|| GroupFileDecisionError::InvalidInput("existing winner has no source peer".to_string()))?;
    let source_peer = &request.peers[source_index];
    let source_file = live_file(&source_peer.classification)
        .ok_or_else(|| GroupFileDecisionError::InvalidInput("source peer is not a live file".to_string()))?;

    let mut copy_intents = Vec::new();
    for peer in &request.peers {
        if live_matches(&peer.classification, winner.byte_size, winner.modified_time) {
            continue;
        }

        copy_intents.push(FileCopyIntent {
            source_peer_id: source_peer.peer_id.clone(),
            source_relative_path: source_file.source_relative_path.clone(),
            destination_peer_id: peer.peer_id.clone(),
            destination_relative_path: request.relative_path.clone(),
            winning_byte_size: winner.byte_size,
            winning_modified_time: winner.modified_time,
        });
    }

    let mut source_peers = Vec::new();
    for index in &winner.sources {
        let peer = &request.peers[*index];
        let file = live_file(&peer.classification).ok_or_else(|| {
            GroupFileDecisionError::InvalidInput("source peer is not a live file".to_string())
        })?;
        source_peers.push(FileOutcomeSource {
            peer_id: peer.peer_id.clone(),
            source_relative_path: file.source_relative_path.clone(),
            byte_size: file.byte_size,
            modified_time: file.modified_time,
        });
    }

    let peer_decisions = request
        .peers
        .iter()
        .enumerate()
        .map(|(index, peer)| PeerFileDecisionFact {
            peer_id: peer.peer_id.clone(),
            role: peer.role,
            classification: peer.classification.clone(),
            statuses: existing_statuses(
                peer,
                index,
                &winner,
                source_index,
                canon_index,
                !copy_intents.is_empty(),
            ),
        })
        .collect();

    Ok(GroupFileDecisionOutput {
        relative_path: request.relative_path,
        group_outcome: FileGroupOutcome::ExistingFile {
            byte_size: winner.byte_size,
            modified_time: winner.modified_time,
        },
        source_peers,
        copy_intents,
        absence_intents: Vec::new(),
        peer_decisions,
    })
}

fn absent_output(
    request: GroupFileDecisionRequest,
    group_outcome: FileGroupOutcome,
    canon_selected: bool,
    deletion_voters: Vec<usize>,
) -> Result<GroupFileDecisionOutput, GroupFileDecisionError> {
    let mut absence_intents = Vec::new();
    for peer in &request.peers {
        if live_file(&peer.classification).is_none() {
            continue;
        }

        if should_displace(peer, &group_outcome, canon_selected) {
            absence_intents.push(FileAbsenceIntent::DisplaceFile {
                peer_id: peer.peer_id.clone(),
                relative_path: request.relative_path.clone(),
            });
        } else {
            absence_intents.push(FileAbsenceIntent::DeleteFile {
                peer_id: peer.peer_id.clone(),
                relative_path: request.relative_path.clone(),
            });
        }
    }

    let peer_decisions = request
        .peers
        .iter()
        .enumerate()
        .map(|(index, peer)| PeerFileDecisionFact {
            peer_id: peer.peer_id.clone(),
            role: peer.role,
            classification: peer.classification.clone(),
            statuses: absent_statuses(
                peer,
                index,
                &group_outcome,
                canon_selected,
                &deletion_voters,
            ),
        })
        .collect();

    Ok(GroupFileDecisionOutput {
        relative_path: request.relative_path,
        group_outcome,
        source_peers: Vec::new(),
        copy_intents: Vec::new(),
        absence_intents,
        peer_decisions,
    })
}

fn existing_statuses(
    peer: &GroupFileDecisionPeer,
    peer_index: usize,
    winner: &ExistingWinner,
    selected_source_index: usize,
    canon_index: Option<usize>,
    any_copy_intent: bool,
) -> Vec<PeerFileDecisionStatus> {
    let mut statuses = if canon_index.is_some() && canon_index != Some(peer_index) {
        vec![PeerFileDecisionStatus::DidNotVote]
    } else {
        base_vote_statuses(peer, winner)
    };

    if canon_index == Some(peer_index) {
        statuses.push(PeerFileDecisionStatus::CanonSelectedOutcome);
    }

    if live_matches(&peer.classification, winner.byte_size, winner.modified_time) {
        statuses.push(PeerFileDecisionStatus::MatchedWinner);
        statuses.push(PeerFileDecisionStatus::IdenticalSource);
        statuses.push(PeerFileDecisionStatus::NotSelectedForCopy);

        if peer_index == selected_source_index && any_copy_intent {
            statuses.push(PeerFileDecisionStatus::SelectedAsCopySource);
        }
    } else {
        statuses.push(PeerFileDecisionStatus::NeedsCopy);
    }

    statuses
}

fn absent_statuses(
    peer: &GroupFileDecisionPeer,
    peer_index: usize,
    group_outcome: &FileGroupOutcome,
    canon_selected: bool,
    deletion_voters: &[usize],
) -> Vec<PeerFileDecisionStatus> {
    let mut statuses = base_absent_vote_statuses(
        peer,
        peer_index,
        canon_selected,
        deletion_voters,
    );

    if live_file(&peer.classification).is_some() {
        if should_displace(peer, group_outcome, canon_selected) {
            statuses.push(PeerFileDecisionStatus::NeedsDisplacement);
        } else {
            statuses.push(PeerFileDecisionStatus::NeedsDeletion);
        }
    } else {
        statuses.push(PeerFileDecisionStatus::MatchedWinner);
    }

    statuses
}

fn base_vote_statuses(
    peer: &GroupFileDecisionPeer,
    winner: &ExistingWinner,
) -> Vec<PeerFileDecisionStatus> {
    if peer.role == GroupFileDecisionPeerRole::Subordinate {
        return vec![PeerFileDecisionStatus::DidNotVote];
    }

    match &peer.classification {
        PeerFileState::UnchangedLiveFile(_)
        | PeerFileState::ModifiedLiveFile(_)
        | PeerFileState::NewLiveFile(_) => {
            vec![PeerFileDecisionStatus::VotedForExistingFile]
        }
        PeerFileState::DeletedFile { .. } => vec![PeerFileDecisionStatus::VotedForDeletion],
        PeerFileState::AbsentUnconfirmed { last_seen } => {
            if let Some(last_seen) = last_seen {
                if more_than_tolerance_newer(*last_seen, winner.modified_time) {
                    return vec![PeerFileDecisionStatus::VotedForDeletion];
                }
            }

            vec![PeerFileDecisionStatus::DidNotVote]
        }
        PeerFileState::AbsentNoRowNoVote => vec![PeerFileDecisionStatus::DidNotVote],
    }
}

fn base_absent_vote_statuses(
    peer: &GroupFileDecisionPeer,
    peer_index: usize,
    canon_selected: bool,
    deletion_voters: &[usize],
) -> Vec<PeerFileDecisionStatus> {
    let mut statuses = Vec::new();

    if canon_selected && peer.role == GroupFileDecisionPeerRole::Canon {
        statuses.push(PeerFileDecisionStatus::CanonSelectedOutcome);
    }

    if canon_selected && peer.role != GroupFileDecisionPeerRole::Canon {
        statuses.push(PeerFileDecisionStatus::DidNotVote);
        return statuses;
    }

    if peer.role == GroupFileDecisionPeerRole::Subordinate {
        statuses.push(PeerFileDecisionStatus::DidNotVote);
        return statuses;
    }

    match &peer.classification {
        PeerFileState::UnchangedLiveFile(_)
        | PeerFileState::ModifiedLiveFile(_)
        | PeerFileState::NewLiveFile(_) => {
            statuses.push(PeerFileDecisionStatus::VotedForExistingFile);
        }
        PeerFileState::DeletedFile { .. } if deletion_voters.contains(&peer_index) => {
            statuses.push(PeerFileDecisionStatus::VotedForDeletion);
        }
        PeerFileState::AbsentUnconfirmed { .. } if deletion_voters.contains(&peer_index) => {
            statuses.push(PeerFileDecisionStatus::VotedForDeletion);
        }
        PeerFileState::DeletedFile { .. }
        | PeerFileState::AbsentUnconfirmed { .. }
        | PeerFileState::AbsentNoRowNoVote => {
            statuses.push(PeerFileDecisionStatus::DidNotVote);
        }
    }

    statuses
}

fn live_candidates(
    peers: &[GroupFileDecisionPeer],
    indexes: &[usize],
) -> Vec<LiveCandidate> {
    indexes
        .iter()
        .filter_map(|index| {
            live_file(&peers[*index].classification).map(|file| LiveCandidate {
                peer_index: *index,
                file: file.clone(),
            })
        })
        .collect()
}

fn select_live_winner(live_candidates: &[LiveCandidate]) -> Option<ExistingWinner> {
    let max_modified_time = live_candidates
        .iter()
        .map(|candidate| candidate.file.modified_time)
        .max()?;

    let winning_byte_size = live_candidates
        .iter()
        .filter(|candidate| within_tolerance(candidate.file.modified_time, max_modified_time))
        .map(|candidate| candidate.file.byte_size)
        .max()?;

    let winning_modified_time = live_candidates
        .iter()
        .filter(|candidate| candidate.file.byte_size == winning_byte_size)
        .filter(|candidate| within_tolerance(candidate.file.modified_time, max_modified_time))
        .map(|candidate| candidate.file.modified_time)
        .max()?;

    let sources = live_candidates
        .iter()
        .filter(|candidate| {
            candidate.file.byte_size == winning_byte_size
                && within_tolerance(candidate.file.modified_time, winning_modified_time)
        })
        .map(|candidate| candidate.peer_index)
        .collect();

    Some(ExistingWinner {
        byte_size: winning_byte_size,
        modified_time: winning_modified_time,
        sources,
    })
}

fn deletion_voter_indexes(
    peers: &[GroupFileDecisionPeer],
    contributing_indexes: &[usize],
    live_winner: Option<&ExistingWinner>,
) -> Vec<usize> {
    contributing_indexes
        .iter()
        .filter_map(|index| match &peers[*index].classification {
            PeerFileState::DeletedFile { .. } => Some(*index),
            PeerFileState::AbsentUnconfirmed {
                last_seen: Some(last_seen),
            } => live_winner.and_then(|winner| {
                if more_than_tolerance_newer(*last_seen, winner.modified_time) {
                    Some(*index)
                } else {
                    None
                }
            }),
            _ => None,
        })
        .collect()
}

fn deletion_vote_time(classification: &PeerFileState) -> Option<SyncTimestamp> {
    match classification {
        PeerFileState::DeletedFile {
            deletion_estimate,
        } => Some(*deletion_estimate),
        PeerFileState::AbsentUnconfirmed {
            last_seen: Some(last_seen),
        } => Some(*last_seen),
        _ => None,
    }
}

fn matching_live_peer_indexes(
    peers: &[GroupFileDecisionPeer],
    byte_size: u64,
    modified_time: SyncTimestamp,
) -> Vec<usize> {
    peers
        .iter()
        .enumerate()
        .filter_map(|(index, peer)| {
            if live_matches(&peer.classification, byte_size, modified_time) {
                Some(index)
            } else {
                None
            }
        })
        .collect()
}

fn live_matches(
    classification: &PeerFileState,
    byte_size: u64,
    modified_time: SyncTimestamp,
) -> bool {
    live_file(classification)
        .map(|file| {
            file.byte_size == byte_size && within_tolerance(file.modified_time, modified_time)
        })
        .unwrap_or(false)
}

fn live_file(classification: &PeerFileState) -> Option<&ClassifiedLiveFile> {
    match classification {
        PeerFileState::UnchangedLiveFile(file)
        | PeerFileState::ModifiedLiveFile(file)
        | PeerFileState::NewLiveFile(file) => Some(file),
        _ => None,
    }
}

fn deletion_estimate(classification: &PeerFileState) -> Option<SyncTimestamp> {
    match classification {
        PeerFileState::DeletedFile {
            deletion_estimate,
        } => Some(*deletion_estimate),
        PeerFileState::AbsentUnconfirmed { last_seen } => *last_seen,
        _ => None,
    }
}

fn should_displace(
    peer: &GroupFileDecisionPeer,
    group_outcome: &FileGroupOutcome,
    canon_selected: bool,
) -> bool {
    !canon_selected
        && peer.role == GroupFileDecisionPeerRole::Subordinate
        && matches!(
            group_outcome,
            FileGroupOutcome::Deletion { .. } | FileGroupOutcome::NoFile
        )
}

fn within_tolerance(left: SyncTimestamp, right: SyncTimestamp) -> bool {
    timestamp_delta_nanos(left, right).abs() <= TOLERANCE_NANOS
}

fn more_than_tolerance_newer(left: SyncTimestamp, right: SyncTimestamp) -> bool {
    timestamp_delta_nanos(left, right) > TOLERANCE_NANOS
}

fn timestamp_delta_nanos(left: SyncTimestamp, right: SyncTimestamp) -> i128 {
    let seconds = i128::from(left.unix_seconds) - i128::from(right.unix_seconds);
    let nanos = i128::from(left.nanoseconds) - i128::from(right.nanoseconds);
    seconds * NANOS_PER_SECOND + nanos
}

fn invalid<T>(message: &str) -> Result<T, GroupFileDecisionError> {
    Err(GroupFileDecisionError::InvalidInput(message.to_string()))
}
