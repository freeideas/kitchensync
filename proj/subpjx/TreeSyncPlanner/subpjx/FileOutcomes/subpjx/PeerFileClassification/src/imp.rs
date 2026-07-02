use std::sync::Arc;

use crate::api::*;

struct PeerFileClassificationImpl;

impl PeerFileClassification for PeerFileClassificationImpl {
    fn classify_peer_file(
        &self,
        request: PeerFileClassificationRequest,
    ) -> Result<PeerFileClassificationResult, PeerFileClassificationError> {
        if request.peer_id.is_empty() {
            return invalid_input("peer_id is required");
        }

        if request.relative_path.is_empty() {
            return invalid_input("relative_path is required");
        }

        let state = match request.presence {
            PeerFilePresenceFact::LiveFile(live_file) => {
                classify_live_file(live_file, request.snapshot_row)?
            }
            PeerFilePresenceFact::AbsentFile => {
                classify_absent_file(request.snapshot_row, request.last_seen)
            }
        };

        Ok(PeerFileClassificationResult {
            peer_id: request.peer_id,
            relative_path: request.relative_path,
            state,
        })
    }
}

pub fn new() -> std::sync::Arc<dyn PeerFileClassification> {
    Arc::new(PeerFileClassificationImpl)
}

fn classify_live_file(
    live_file: PeerLiveFileFact,
    snapshot_row: Option<PeerFileSnapshotRow>,
) -> Result<PeerFileClassificationState, PeerFileClassificationError> {
    let classified = ClassifiedPeerLiveFile {
        byte_size: live_file.byte_size,
        modified_time: live_file.modified_time,
    };

    let Some(snapshot_row) = snapshot_row else {
        return Ok(PeerFileClassificationState::NewLiveFile(classified));
    };

    if snapshot_row.deleted_time.is_some() {
        return Ok(PeerFileClassificationState::ModifiedLiveFile(classified));
    }

    let Some(snapshot_byte_size) = snapshot_row.byte_size else {
        return invalid_input("snapshot byte_size is required for live comparison");
    };

    let Some(snapshot_modified_time) = snapshot_row.modified_time else {
        return invalid_input("snapshot modified_time is required for live comparison");
    };

    if live_file.byte_size == snapshot_byte_size
        && within_five_seconds(live_file.modified_time, snapshot_modified_time)
    {
        Ok(PeerFileClassificationState::UnchangedLiveFile(classified))
    } else {
        Ok(PeerFileClassificationState::ModifiedLiveFile(classified))
    }
}

fn classify_absent_file(
    snapshot_row: Option<PeerFileSnapshotRow>,
    last_seen: Option<PeerFileTimestamp>,
) -> PeerFileClassificationState {
    let Some(snapshot_row) = snapshot_row else {
        return PeerFileClassificationState::AbsentNoRowNoVote;
    };

    if let Some(deletion_estimate) = snapshot_row.deleted_time {
        PeerFileClassificationState::DeletedFile {
            deletion_estimate,
        }
    } else {
        PeerFileClassificationState::AbsentUnconfirmed { last_seen }
    }
}

fn within_five_seconds(left: PeerFileTimestamp, right: PeerFileTimestamp) -> bool {
    const TOLERANCE_NANOSECONDS: i128 = 5_000_000_000;

    let left_nanos =
        i128::from(left.unix_seconds) * 1_000_000_000 + i128::from(left.nanoseconds);
    let right_nanos =
        i128::from(right.unix_seconds) * 1_000_000_000 + i128::from(right.nanoseconds);

    (left_nanos - right_nanos).abs() <= TOLERANCE_NANOSECONDS
}

fn invalid_input<T>(message: &str) -> Result<T, PeerFileClassificationError> {
    Err(PeerFileClassificationError::InvalidInput(
        message.to_string(),
    ))
}
