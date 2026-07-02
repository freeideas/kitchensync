use crate::api::*;
use std::collections::HashMap;
use std::sync::Arc;

struct FileOutcomesImpl {
    groupfiledecision: std::sync::Arc<dyn treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecision>,
    peerfileclassification: std::sync::Arc<dyn treesyncplanner_fileoutcomes_peerfileclassification::PeerFileClassification>,
}

impl FileOutcomes for FileOutcomesImpl {
    fn classify_peer_file(
        &self,
        request: PeerFileClassificationRequest,
    ) -> Result<PeerFileClassification, FileOutcomesError> {
        let source_relative_path = match &request.presence {
            PeerFilePresenceFact::LiveFile(file) => Some(file.source_relative_path.clone()),
            PeerFilePresenceFact::Absent => None,
        };

        let result = self
            .peerfileclassification
            .classify_peer_file(to_peer_classification_request(request))
            .map_err(from_peer_classification_error)?;

        Ok(PeerFileClassification {
            peer_id: result.peer_id,
            relative_path: result.relative_path,
            state: from_peer_classification_state(result.state, source_relative_path)?,
        })
    }

    fn decide_file_outcome(
        &self,
        request: FileOutcomeRequest,
    ) -> Result<FileOutcomeDecision, FileOutcomesError> {
        let source_relative_paths = source_relative_paths_by_peer(&request)?;
        let output = self
            .groupfiledecision
            .decide_group_file(to_group_request(request))
            .map_err(from_group_error)?;

        Ok(from_group_output(output, &source_relative_paths))
    }
}

pub fn new(groupfiledecision: std::sync::Arc<dyn treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecision>, peerfileclassification: std::sync::Arc<dyn treesyncplanner_fileoutcomes_peerfileclassification::PeerFileClassification>) -> std::sync::Arc<dyn FileOutcomes> {
    Arc::new(FileOutcomesImpl { groupfiledecision, peerfileclassification })
}

fn to_peer_classification_request(
    request: PeerFileClassificationRequest,
) -> treesyncplanner_fileoutcomes_peerfileclassification::PeerFileClassificationRequest {
    treesyncplanner_fileoutcomes_peerfileclassification::PeerFileClassificationRequest {
        peer_id: request.peer_id,
        relative_path: request.relative_path,
        presence: match request.presence {
            PeerFilePresenceFact::LiveFile(file) => {
                treesyncplanner_fileoutcomes_peerfileclassification::PeerFilePresenceFact::LiveFile(
                    treesyncplanner_fileoutcomes_peerfileclassification::PeerLiveFileFact {
                        byte_size: file.byte_size,
                        modified_time: to_peer_timestamp(file.modified_time),
                    },
                )
            }
            PeerFilePresenceFact::Absent => {
                treesyncplanner_fileoutcomes_peerfileclassification::PeerFilePresenceFact::AbsentFile
            }
        },
        snapshot_row: request.snapshot_row.map(|row| {
            treesyncplanner_fileoutcomes_peerfileclassification::PeerFileSnapshotRow {
                byte_size: row.byte_size,
                modified_time: row.modified_time.map(to_peer_timestamp),
                deleted_time: row.deleted_time.map(to_peer_timestamp),
            }
        }),
        last_seen: request.last_seen.map(to_peer_timestamp),
    }
}

fn from_peer_classification_state(
    state: treesyncplanner_fileoutcomes_peerfileclassification::PeerFileClassificationState,
    source_relative_path: Option<String>,
) -> Result<PeerFileState, FileOutcomesError> {
    match state {
        treesyncplanner_fileoutcomes_peerfileclassification::PeerFileClassificationState::UnchangedLiveFile(file) => {
            Ok(PeerFileState::UnchangedLiveFile(from_peer_live_file(file, source_relative_path)?))
        }
        treesyncplanner_fileoutcomes_peerfileclassification::PeerFileClassificationState::ModifiedLiveFile(file) => {
            Ok(PeerFileState::ModifiedLiveFile(from_peer_live_file(file, source_relative_path)?))
        }
        treesyncplanner_fileoutcomes_peerfileclassification::PeerFileClassificationState::NewLiveFile(file) => {
            Ok(PeerFileState::NewLiveFile(from_peer_live_file(file, source_relative_path)?))
        }
        treesyncplanner_fileoutcomes_peerfileclassification::PeerFileClassificationState::DeletedFile {
            deletion_estimate,
        } => Ok(PeerFileState::DeletedFile {
            deletion_estimate: from_peer_timestamp(deletion_estimate),
        }),
        treesyncplanner_fileoutcomes_peerfileclassification::PeerFileClassificationState::AbsentUnconfirmed {
            last_seen,
        } => Ok(PeerFileState::AbsentUnconfirmed {
            last_seen: last_seen.map(from_peer_timestamp),
        }),
        treesyncplanner_fileoutcomes_peerfileclassification::PeerFileClassificationState::AbsentNoRowNoVote => {
            Ok(PeerFileState::AbsentNoRowNoVote)
        }
    }
}

fn from_peer_live_file(
    file: treesyncplanner_fileoutcomes_peerfileclassification::ClassifiedPeerLiveFile,
    source_relative_path: Option<String>,
) -> Result<ClassifiedLiveFile, FileOutcomesError> {
    let Some(source_relative_path) = source_relative_path else {
        return Err(FileOutcomesError::InvalidInput(
            "live classification is missing source_relative_path".to_string(),
        ));
    };

    Ok(ClassifiedLiveFile {
        byte_size: file.byte_size,
        modified_time: from_peer_timestamp(file.modified_time),
        source_relative_path,
    })
}

fn to_group_request(
    request: FileOutcomeRequest,
) -> treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecisionRequest {
    let relative_path = request.relative_path;
    treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecisionRequest {
        peers: request
            .peers
            .into_iter()
            .map(|peer| to_group_peer(peer, &relative_path))
            .collect(),
        relative_path,
    }
}

fn to_group_peer(
    peer: FileOutcomePeer,
    relative_path: &str,
) -> treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecisionPeer {
    treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecisionPeer {
        peer_id: peer.peer_id,
        role: to_group_role(peer.role),
        classification: to_group_state(peer.classification, relative_path),
    }
}

fn from_group_output(
    output: treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecisionOutput,
    source_relative_paths: &HashMap<String, String>,
) -> FileOutcomeDecision {
    FileOutcomeDecision {
        relative_path: output.relative_path,
        group_outcome: from_group_outcome(output.group_outcome),
        source_peers: output
            .source_peers
            .into_iter()
            .map(|source| from_group_source(source, source_relative_paths))
            .collect(),
        copy_intents: output
            .copy_intents
            .into_iter()
            .map(|intent| from_group_copy_intent(intent, source_relative_paths))
            .collect(),
        absence_intents: output
            .absence_intents
            .into_iter()
            .map(from_group_absence_intent)
            .collect(),
        peer_decisions: output
            .peer_decisions
            .into_iter()
            .map(|fact| from_group_peer_decision(fact, source_relative_paths))
            .collect(),
    }
}

fn source_relative_paths_by_peer(
    request: &FileOutcomeRequest,
) -> Result<HashMap<String, String>, FileOutcomesError> {
    let mut source_relative_paths = HashMap::new();

    for peer in &request.peers {
        if let Some(file) = live_file_from_state(&peer.classification) {
            if file.source_relative_path.is_empty() {
                return Err(FileOutcomesError::InvalidInput(
                    "live file source_relative_path must not be empty".to_string(),
                ));
            }
            source_relative_paths.insert(peer.peer_id.clone(), file.source_relative_path.clone());
        }
    }

    Ok(source_relative_paths)
}

fn live_file_from_state(state: &PeerFileState) -> Option<&ClassifiedLiveFile> {
    match state {
        PeerFileState::UnchangedLiveFile(file)
        | PeerFileState::ModifiedLiveFile(file)
        | PeerFileState::NewLiveFile(file) => Some(file),
        PeerFileState::DeletedFile { .. }
        | PeerFileState::AbsentUnconfirmed { .. }
        | PeerFileState::AbsentNoRowNoVote => None,
    }
}

fn to_group_state(
    state: PeerFileState,
    relative_path: &str,
) -> treesyncplanner_fileoutcomes_groupfiledecision::PeerFileState {
    match state {
        PeerFileState::UnchangedLiveFile(file) => {
            treesyncplanner_fileoutcomes_groupfiledecision::PeerFileState::UnchangedLiveFile(
                to_group_live_file(file, relative_path),
            )
        }
        PeerFileState::ModifiedLiveFile(file) => {
            treesyncplanner_fileoutcomes_groupfiledecision::PeerFileState::ModifiedLiveFile(
                to_group_live_file(file, relative_path),
            )
        }
        PeerFileState::NewLiveFile(file) => {
            treesyncplanner_fileoutcomes_groupfiledecision::PeerFileState::NewLiveFile(
                to_group_live_file(file, relative_path),
            )
        }
        PeerFileState::DeletedFile {
            deletion_estimate,
        } => treesyncplanner_fileoutcomes_groupfiledecision::PeerFileState::DeletedFile {
            deletion_estimate: to_group_timestamp(deletion_estimate),
        },
        PeerFileState::AbsentUnconfirmed { last_seen } => {
            treesyncplanner_fileoutcomes_groupfiledecision::PeerFileState::AbsentUnconfirmed {
                last_seen: last_seen.map(to_group_timestamp),
            }
        }
        PeerFileState::AbsentNoRowNoVote => {
            treesyncplanner_fileoutcomes_groupfiledecision::PeerFileState::AbsentNoRowNoVote
        }
    }
}

fn from_group_state(
    state: treesyncplanner_fileoutcomes_groupfiledecision::PeerFileState,
    peer_id: &str,
    source_relative_paths: &HashMap<String, String>,
) -> PeerFileState {
    match state {
        treesyncplanner_fileoutcomes_groupfiledecision::PeerFileState::UnchangedLiveFile(file) => {
            PeerFileState::UnchangedLiveFile(from_group_live_file(
                file,
                peer_id,
                source_relative_paths,
            ))
        }
        treesyncplanner_fileoutcomes_groupfiledecision::PeerFileState::ModifiedLiveFile(file) => {
            PeerFileState::ModifiedLiveFile(from_group_live_file(
                file,
                peer_id,
                source_relative_paths,
            ))
        }
        treesyncplanner_fileoutcomes_groupfiledecision::PeerFileState::NewLiveFile(file) => {
            PeerFileState::NewLiveFile(from_group_live_file(
                file,
                peer_id,
                source_relative_paths,
            ))
        }
        treesyncplanner_fileoutcomes_groupfiledecision::PeerFileState::DeletedFile {
            deletion_estimate,
        } => PeerFileState::DeletedFile {
            deletion_estimate: from_group_timestamp(deletion_estimate),
        },
        treesyncplanner_fileoutcomes_groupfiledecision::PeerFileState::AbsentUnconfirmed {
            last_seen,
        } => PeerFileState::AbsentUnconfirmed {
            last_seen: last_seen.map(from_group_timestamp),
        },
        treesyncplanner_fileoutcomes_groupfiledecision::PeerFileState::AbsentNoRowNoVote => {
            PeerFileState::AbsentNoRowNoVote
        }
    }
}

fn to_group_live_file(
    file: ClassifiedLiveFile,
    relative_path: &str,
) -> treesyncplanner_fileoutcomes_groupfiledecision::ClassifiedLiveFile {
    treesyncplanner_fileoutcomes_groupfiledecision::ClassifiedLiveFile {
        byte_size: file.byte_size,
        modified_time: to_group_timestamp(file.modified_time),
        source_relative_path: relative_path.to_string(),
    }
}

fn from_group_live_file(
    file: treesyncplanner_fileoutcomes_groupfiledecision::ClassifiedLiveFile,
    peer_id: &str,
    source_relative_paths: &HashMap<String, String>,
) -> ClassifiedLiveFile {
    ClassifiedLiveFile {
        byte_size: file.byte_size,
        modified_time: from_group_timestamp(file.modified_time),
        source_relative_path: source_relative_paths
            .get(peer_id)
            .cloned()
            .unwrap_or(file.source_relative_path),
    }
}

fn from_group_outcome(
    outcome: treesyncplanner_fileoutcomes_groupfiledecision::FileGroupOutcome,
) -> FileGroupOutcome {
    match outcome {
        treesyncplanner_fileoutcomes_groupfiledecision::FileGroupOutcome::ExistingFile {
            byte_size,
            modified_time,
        } => FileGroupOutcome::ExistingFile {
            byte_size,
            modified_time: from_group_timestamp(modified_time),
        },
        treesyncplanner_fileoutcomes_groupfiledecision::FileGroupOutcome::Deletion {
            deletion_estimate,
        } => FileGroupOutcome::Deletion {
            deletion_estimate: deletion_estimate.map(from_group_timestamp),
        },
        treesyncplanner_fileoutcomes_groupfiledecision::FileGroupOutcome::NoFile => {
            FileGroupOutcome::NoFile
        }
    }
}

fn from_group_source(
    source: treesyncplanner_fileoutcomes_groupfiledecision::FileOutcomeSource,
    source_relative_paths: &HashMap<String, String>,
) -> FileOutcomeSource {
    let source_relative_path = source_relative_paths
        .get(&source.peer_id)
        .cloned()
        .unwrap_or(source.source_relative_path);

    FileOutcomeSource {
        peer_id: source.peer_id,
        source_relative_path,
        byte_size: source.byte_size,
        modified_time: from_group_timestamp(source.modified_time),
    }
}

fn from_group_copy_intent(
    intent: treesyncplanner_fileoutcomes_groupfiledecision::FileCopyIntent,
    source_relative_paths: &HashMap<String, String>,
) -> FileCopyIntent {
    let source_relative_path = source_relative_paths
        .get(&intent.source_peer_id)
        .cloned()
        .unwrap_or(intent.source_relative_path);

    FileCopyIntent {
        source_peer_id: intent.source_peer_id,
        source_relative_path,
        destination_peer_id: intent.destination_peer_id,
        destination_relative_path: intent.destination_relative_path,
        winning_byte_size: intent.winning_byte_size,
        winning_modified_time: from_group_timestamp(intent.winning_modified_time),
    }
}

fn from_group_absence_intent(
    intent: treesyncplanner_fileoutcomes_groupfiledecision::FileAbsenceIntent,
) -> FileAbsenceIntent {
    match intent {
        treesyncplanner_fileoutcomes_groupfiledecision::FileAbsenceIntent::DeleteFile {
            peer_id,
            relative_path,
        } => FileAbsenceIntent::DeleteFile {
            peer_id,
            relative_path,
        },
        treesyncplanner_fileoutcomes_groupfiledecision::FileAbsenceIntent::DisplaceFile {
            peer_id,
            relative_path,
        } => FileAbsenceIntent::DisplaceFile {
            peer_id,
            relative_path,
        },
    }
}

fn from_group_peer_decision(
    fact: treesyncplanner_fileoutcomes_groupfiledecision::PeerFileDecisionFact,
    source_relative_paths: &HashMap<String, String>,
) -> PeerFileDecisionFact {
    let peer_id = fact.peer_id;
    PeerFileDecisionFact {
        classification: from_group_state(fact.classification, &peer_id, source_relative_paths),
        peer_id,
        role: from_group_role(fact.role),
        statuses: fact.statuses.into_iter().map(from_group_status).collect(),
    }
}

fn to_group_role(
    role: FileOutcomePeerRole,
) -> treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecisionPeerRole {
    match role {
        FileOutcomePeerRole::Canon => {
            treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecisionPeerRole::Canon
        }
        FileOutcomePeerRole::Contributing => {
            treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecisionPeerRole::Contributing
        }
        FileOutcomePeerRole::Subordinate => {
            treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecisionPeerRole::Subordinate
        }
    }
}

fn from_group_role(
    role: treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecisionPeerRole,
) -> FileOutcomePeerRole {
    match role {
        treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecisionPeerRole::Canon => {
            FileOutcomePeerRole::Canon
        }
        treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecisionPeerRole::Contributing => {
            FileOutcomePeerRole::Contributing
        }
        treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecisionPeerRole::Subordinate => {
            FileOutcomePeerRole::Subordinate
        }
    }
}

fn from_group_status(
    status: treesyncplanner_fileoutcomes_groupfiledecision::PeerFileDecisionStatus,
) -> PeerFileDecisionStatus {
    match status {
        treesyncplanner_fileoutcomes_groupfiledecision::PeerFileDecisionStatus::CanonSelectedOutcome => PeerFileDecisionStatus::CanonSelectedOutcome,
        treesyncplanner_fileoutcomes_groupfiledecision::PeerFileDecisionStatus::VotedForExistingFile => PeerFileDecisionStatus::VotedForExistingFile,
        treesyncplanner_fileoutcomes_groupfiledecision::PeerFileDecisionStatus::VotedForDeletion => PeerFileDecisionStatus::VotedForDeletion,
        treesyncplanner_fileoutcomes_groupfiledecision::PeerFileDecisionStatus::DidNotVote => PeerFileDecisionStatus::DidNotVote,
        treesyncplanner_fileoutcomes_groupfiledecision::PeerFileDecisionStatus::MatchedWinner => PeerFileDecisionStatus::MatchedWinner,
        treesyncplanner_fileoutcomes_groupfiledecision::PeerFileDecisionStatus::IdenticalSource => PeerFileDecisionStatus::IdenticalSource,
        treesyncplanner_fileoutcomes_groupfiledecision::PeerFileDecisionStatus::SelectedAsCopySource => PeerFileDecisionStatus::SelectedAsCopySource,
        treesyncplanner_fileoutcomes_groupfiledecision::PeerFileDecisionStatus::NeedsCopy => PeerFileDecisionStatus::NeedsCopy,
        treesyncplanner_fileoutcomes_groupfiledecision::PeerFileDecisionStatus::NotSelectedForCopy => PeerFileDecisionStatus::NotSelectedForCopy,
        treesyncplanner_fileoutcomes_groupfiledecision::PeerFileDecisionStatus::NeedsDeletion => PeerFileDecisionStatus::NeedsDeletion,
        treesyncplanner_fileoutcomes_groupfiledecision::PeerFileDecisionStatus::NeedsDisplacement => PeerFileDecisionStatus::NeedsDisplacement,
    }
}

fn to_peer_timestamp(
    timestamp: SyncTimestamp,
) -> treesyncplanner_fileoutcomes_peerfileclassification::PeerFileTimestamp {
    treesyncplanner_fileoutcomes_peerfileclassification::PeerFileTimestamp {
        unix_seconds: timestamp.unix_seconds,
        nanoseconds: timestamp.nanoseconds,
    }
}

fn from_peer_timestamp(
    timestamp: treesyncplanner_fileoutcomes_peerfileclassification::PeerFileTimestamp,
) -> SyncTimestamp {
    SyncTimestamp {
        unix_seconds: timestamp.unix_seconds,
        nanoseconds: timestamp.nanoseconds,
    }
}

fn to_group_timestamp(
    timestamp: SyncTimestamp,
) -> treesyncplanner_fileoutcomes_groupfiledecision::SyncTimestamp {
    treesyncplanner_fileoutcomes_groupfiledecision::SyncTimestamp {
        unix_seconds: timestamp.unix_seconds,
        nanoseconds: timestamp.nanoseconds,
    }
}

fn from_group_timestamp(
    timestamp: treesyncplanner_fileoutcomes_groupfiledecision::SyncTimestamp,
) -> SyncTimestamp {
    SyncTimestamp {
        unix_seconds: timestamp.unix_seconds,
        nanoseconds: timestamp.nanoseconds,
    }
}

fn from_peer_classification_error(
    error: treesyncplanner_fileoutcomes_peerfileclassification::PeerFileClassificationError,
) -> FileOutcomesError {
    match error {
        treesyncplanner_fileoutcomes_peerfileclassification::PeerFileClassificationError::InvalidInput(message) => {
            FileOutcomesError::InvalidInput(message)
        }
    }
}

fn from_group_error(
    error: treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecisionError,
) -> FileOutcomesError {
    match error {
        treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecisionError::InvalidInput(message) => {
            FileOutcomesError::InvalidInput(message)
        }
    }
}
