use std::sync::Arc;

use treesyncplanner_fileoutcomes_peerfileclassification::{
    new, ClassifiedPeerLiveFile, PeerFileClassification, PeerFileClassificationError,
    PeerFileClassificationRequest, PeerFileClassificationResult, PeerFileClassificationState,
    PeerFilePresenceFact, PeerFileSnapshotRow, PeerFileTimestamp, PeerLiveFileFact,
};

const PEER_ID: &str = "peer-a";
const RELATIVE_PATH: &str = "docs/report.txt";

fn timestamp(unix_seconds: i64) -> PeerFileTimestamp {
    PeerFileTimestamp {
        unix_seconds,
        nanoseconds: 0,
    }
}

fn live_fact(byte_size: u64, modified_time: PeerFileTimestamp) -> PeerFilePresenceFact {
    PeerFilePresenceFact::LiveFile(PeerLiveFileFact {
        byte_size,
        modified_time,
    })
}

fn snapshot_row(
    byte_size: Option<u64>,
    modified_time: Option<PeerFileTimestamp>,
    deleted_time: Option<PeerFileTimestamp>,
) -> PeerFileSnapshotRow {
    PeerFileSnapshotRow {
        byte_size,
        modified_time,
        deleted_time,
    }
}

fn request(
    presence: PeerFilePresenceFact,
    snapshot_row: Option<PeerFileSnapshotRow>,
    last_seen: Option<PeerFileTimestamp>,
) -> PeerFileClassificationRequest {
    PeerFileClassificationRequest {
        peer_id: PEER_ID.to_string(),
        relative_path: RELATIVE_PATH.to_string(),
        presence,
        snapshot_row,
        last_seen,
    }
}

fn classify(request: PeerFileClassificationRequest) -> PeerFileClassificationResult {
    let subject: Arc<dyn PeerFileClassification> = new();
    subject.classify_peer_file(request).unwrap()
}

fn classify_error(request: PeerFileClassificationRequest) -> PeerFileClassificationError {
    let subject: Arc<dyn PeerFileClassification> = new();
    subject.classify_peer_file(request).unwrap_err()
}

fn expected_live(byte_size: u64, modified_time: PeerFileTimestamp) -> ClassifiedPeerLiveFile {
    ClassifiedPeerLiveFile {
        byte_size,
        modified_time,
    }
}

#[test]
fn live_file_with_matching_snapshot_size_and_time_is_unchanged() {
    let modified_time = timestamp(1_700_000_000);

    let result = classify(request(
        live_fact(42, modified_time),
        Some(snapshot_row(Some(42), Some(modified_time), None)),
        None,
    ));

    assert_eq!(result.peer_id, PEER_ID);
    assert_eq!(result.relative_path, RELATIVE_PATH);
    assert_eq!(
        result.state,
        PeerFileClassificationState::UnchangedLiveFile(expected_live(42, modified_time))
    );
}

#[test]
fn live_file_with_different_snapshot_size_is_modified() {
    let modified_time = timestamp(1_700_000_000);

    let result = classify(request(
        live_fact(43, modified_time),
        Some(snapshot_row(Some(42), Some(modified_time), None)),
        None,
    ));

    assert_eq!(
        result.state,
        PeerFileClassificationState::ModifiedLiveFile(expected_live(43, modified_time))
    );
}

#[test]
fn live_file_more_than_five_seconds_after_snapshot_time_is_modified() {
    let snapshot_time = timestamp(1_700_000_000);
    let live_time = timestamp(1_700_000_006);

    let result = classify(request(
        live_fact(42, live_time),
        Some(snapshot_row(Some(42), Some(snapshot_time), None)),
        None,
    ));

    assert_eq!(
        result.state,
        PeerFileClassificationState::ModifiedLiveFile(expected_live(42, live_time))
    );
}

#[test]
fn live_file_more_than_five_seconds_before_snapshot_time_is_modified() {
    let snapshot_time = timestamp(1_700_000_000);
    let live_time = timestamp(1_699_999_994);

    let result = classify(request(
        live_fact(42, live_time),
        Some(snapshot_row(Some(42), Some(snapshot_time), None)),
        None,
    ));

    assert_eq!(
        result.state,
        PeerFileClassificationState::ModifiedLiveFile(expected_live(42, live_time))
    );
}

#[test]
fn live_file_with_deleted_snapshot_row_is_modified() {
    let modified_time = timestamp(1_700_000_000);
    let deleted_time = timestamp(1_700_000_100);

    let result = classify(request(
        live_fact(42, modified_time),
        Some(snapshot_row(Some(42), Some(modified_time), Some(deleted_time))),
        None,
    ));

    assert_eq!(
        result.state,
        PeerFileClassificationState::ModifiedLiveFile(expected_live(42, modified_time))
    );
}

#[test]
fn live_file_without_snapshot_row_is_new() {
    let modified_time = timestamp(1_700_000_000);

    let result = classify(request(live_fact(42, modified_time), None, None));

    assert_eq!(
        result.state,
        PeerFileClassificationState::NewLiveFile(expected_live(42, modified_time))
    );
}

#[test]
fn absent_file_with_deleted_snapshot_row_is_deleted_with_that_estimate() {
    let deleted_time = timestamp(1_700_000_100);

    let result = classify(request(
        PeerFilePresenceFact::AbsentFile,
        Some(snapshot_row(Some(42), Some(timestamp(1_700_000_000)), Some(deleted_time))),
        None,
    ));

    assert_eq!(
        result.state,
        PeerFileClassificationState::DeletedFile {
            deletion_estimate: deleted_time
        }
    );
}

#[test]
fn absent_file_with_existing_snapshot_row_is_absent_unconfirmed() {
    let last_seen = timestamp(1_700_000_200);

    let result = classify(request(
        PeerFilePresenceFact::AbsentFile,
        Some(snapshot_row(Some(42), Some(timestamp(1_700_000_000)), None)),
        Some(last_seen),
    ));

    assert_eq!(
        result.state,
        PeerFileClassificationState::AbsentUnconfirmed {
            last_seen: Some(last_seen)
        }
    );
}

#[test]
fn absent_file_without_snapshot_row_contributes_no_vote() {
    let result = classify(request(PeerFilePresenceFact::AbsentFile, None, None));

    assert_eq!(
        result.state,
        PeerFileClassificationState::AbsentNoRowNoVote
    );
}

#[test]
fn live_file_exactly_five_seconds_from_snapshot_time_is_unchanged() {
    let snapshot_time = timestamp(1_700_000_000);
    let later_live_time = timestamp(1_700_000_005);
    let earlier_live_time = timestamp(1_699_999_995);

    let later_result = classify(request(
        live_fact(42, later_live_time),
        Some(snapshot_row(Some(42), Some(snapshot_time), None)),
        None,
    ));
    let earlier_result = classify(request(
        live_fact(42, earlier_live_time),
        Some(snapshot_row(Some(42), Some(snapshot_time), None)),
        None,
    ));

    assert_eq!(
        later_result.state,
        PeerFileClassificationState::UnchangedLiveFile(expected_live(42, later_live_time))
    );
    assert_eq!(
        earlier_result.state,
        PeerFileClassificationState::UnchangedLiveFile(expected_live(42, earlier_live_time))
    );
}

#[test]
fn live_file_with_non_deleted_snapshot_missing_comparison_metadata_is_invalid_input() {
    let modified_time = timestamp(1_700_000_000);

    let missing_byte_size_error = classify_error(request(
        live_fact(42, modified_time),
        Some(snapshot_row(None, Some(modified_time), None)),
        None,
    ));
    let missing_modified_time_error = classify_error(request(
        live_fact(42, modified_time),
        Some(snapshot_row(Some(42), None, None)),
        None,
    ));

    assert!(matches!(
        missing_byte_size_error,
        PeerFileClassificationError::InvalidInput(_)
    ));
    assert!(matches!(
        missing_modified_time_error,
        PeerFileClassificationError::InvalidInput(_)
    ));
}
