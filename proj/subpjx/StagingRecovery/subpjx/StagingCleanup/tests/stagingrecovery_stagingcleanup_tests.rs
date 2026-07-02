use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};

use stagingrecovery_stagingcleanup::{
    new, StagingCleanupArea, StagingCleanupDirectoryListing, StagingCleanupFailureCause,
    StagingCleanupFailureOperation, StagingCleanupFileOperations, StagingCleanupOperationError,
    StagingCleanupOperationErrorCategory, StagingCleanupPeer, StagingCleanupRequest,
};

#[test]
fn removes_expired_bak_and_tmp_directories_and_leaves_current_and_swap_state() {
    let peer = StagingCleanupPeer {
        identity: "left-peer".to_owned(),
    };
    let parent = "sync-root/subdir".to_owned();
    let bak_root = "sync-root/subdir/.kitchensync/BAK".to_owned();
    let tmp_root = "sync-root/subdir/.kitchensync/TMP".to_owned();

    let file_operations = RecordingFileOperations::new([
        (
            bak_root.clone(),
            vec![
                "1970-01-05_00-00-00_000000Z",
                "1970-01-06_00-00-00_000000Z",
            ],
        ),
        (
            tmp_root.clone(),
            vec![
                "1970-01-08_00-00-00_000000Z",
                "1970-01-09_00-00-00_000000Z",
            ],
        ),
        (
            "sync-root/subdir/.kitchensync/SWAP".to_owned(),
            vec!["1970-01-01_00-00-00_000000Z"],
        ),
    ]);

    let subject = new();
    subject
        .clean_expired_staging(
            StagingCleanupRequest {
                peer,
                parent_directory: parent,
                current_time: UNIX_EPOCH + Duration::from_secs(10 * 86_400),
                keep_bak_days: 5,
                keep_tmp_days: 2,
            },
            &file_operations,
        )
        .expect("cleanup should remove only expired BAK and TMP timestamp directories");

    assert_eq!(
        file_operations.listed_paths(),
        vec![bak_root.clone(), tmp_root.clone()]
    );
    assert_eq!(
        file_operations.removed_paths(),
        vec![
            format!("{bak_root}/1970-01-05_00-00-00_000000Z"),
            format!("{tmp_root}/1970-01-08_00-00-00_000000Z"),
        ]
    );
}

#[test]
fn treats_missing_bak_and_tmp_cleanup_roots_as_empty() {
    let peer = StagingCleanupPeer {
        identity: "left-peer".to_owned(),
    };
    let parent = "sync-root/subdir".to_owned();
    let bak_root = "sync-root/subdir/.kitchensync/BAK".to_owned();
    let tmp_root = "sync-root/subdir/.kitchensync/TMP".to_owned();
    let file_operations = RecordingFileOperations::empty();

    let subject = new();
    subject
        .clean_expired_staging(
            StagingCleanupRequest {
                peer,
                parent_directory: parent,
                current_time: UNIX_EPOCH + Duration::from_secs(10 * 86_400),
                keep_bak_days: 5,
                keep_tmp_days: 2,
            },
            &file_operations,
        )
        .expect("missing cleanup roots should not fail cleanup");

    assert_eq!(file_operations.listed_paths(), vec![bak_root, tmp_root]);
    assert_eq!(file_operations.removed_paths(), Vec::<String>::new());
}

#[test]
fn reports_failure_when_existing_cleanup_root_cannot_be_inspected() {
    let peer = StagingCleanupPeer {
        identity: "left-peer".to_owned(),
    };
    let parent = "sync-root/subdir".to_owned();
    let bak_root = "sync-root/subdir/.kitchensync/BAK".to_owned();
    let inspect_error = StagingCleanupOperationError {
        category: Some(StagingCleanupOperationErrorCategory::PermissionDenied),
        message: "permission denied".to_owned(),
    };
    let file_operations = RecordingFileOperations::with_listing_outcomes([(
        bak_root.clone(),
        ListingOutcome::Err(inspect_error.clone()),
    )]);

    let subject = new();
    let failure = subject
        .clean_expired_staging(
            StagingCleanupRequest {
                peer: peer.clone(),
                parent_directory: parent.clone(),
                current_time: UNIX_EPOCH + Duration::from_secs(10 * 86_400),
                keep_bak_days: 5,
                keep_tmp_days: 2,
            },
            &file_operations,
        )
        .expect_err("cleanup should report an inspect failure");

    assert_eq!(failure.peer, peer);
    assert_eq!(failure.parent_directory, parent);
    assert_eq!(failure.area, StagingCleanupArea::Bak);
    assert_eq!(failure.failed_path, bak_root);
    assert_eq!(failure.timestamp_directory, None);
    assert_eq!(
        failure.operation,
        StagingCleanupFailureOperation::InspectCleanupRoot
    );
    assert_eq!(
        failure.cause,
        StagingCleanupFailureCause::Filesystem(inspect_error)
    );
}

#[test]
fn reports_failure_when_timestamp_directory_name_cannot_be_aged() {
    let peer = StagingCleanupPeer {
        identity: "left-peer".to_owned(),
    };
    let parent = "sync-root/subdir".to_owned();
    let bak_root = "sync-root/subdir/.kitchensync/BAK".to_owned();
    let invalid_timestamp = "not-a-timestamp";
    let file_operations = RecordingFileOperations::new([(
        bak_root.clone(),
        vec![invalid_timestamp],
    )]);

    let subject = new();
    let failure = subject
        .clean_expired_staging(
            StagingCleanupRequest {
                peer: peer.clone(),
                parent_directory: parent.clone(),
                current_time: UNIX_EPOCH + Duration::from_secs(10 * 86_400),
                keep_bak_days: 5,
                keep_tmp_days: 2,
            },
            &file_operations,
        )
        .expect_err("cleanup should report a timestamp parsing failure");

    assert_eq!(failure.peer, peer);
    assert_eq!(failure.parent_directory, parent);
    assert_eq!(failure.area, StagingCleanupArea::Bak);
    assert_eq!(failure.failed_path, format!("{bak_root}/{invalid_timestamp}"));
    assert_eq!(
        failure.timestamp_directory,
        Some(invalid_timestamp.to_owned())
    );
    assert_eq!(
        failure.operation,
        StagingCleanupFailureOperation::DetermineTimestampAge
    );
    assert!(matches!(
        failure.cause,
        StagingCleanupFailureCause::InvalidTimestamp { .. }
    ));
    assert_eq!(file_operations.removed_paths(), Vec::<String>::new());
}

#[test]
fn reports_failure_when_expired_timestamp_directory_cannot_be_removed() {
    let peer = StagingCleanupPeer {
        identity: "left-peer".to_owned(),
    };
    let parent = "sync-root/subdir".to_owned();
    let bak_root = "sync-root/subdir/.kitchensync/BAK".to_owned();
    let expired_timestamp = "1970-01-05_00-00-00_000000Z";
    let expired_path = format!("{bak_root}/{expired_timestamp}");
    let remove_error = StagingCleanupOperationError {
        category: Some(StagingCleanupOperationErrorCategory::IoError),
        message: "remove failed".to_owned(),
    };
    let file_operations =
        RecordingFileOperations::new([(bak_root.clone(), vec![expired_timestamp])])
            .with_remove_errors([(expired_path.clone(), remove_error.clone())]);

    let subject = new();
    let failure = subject
        .clean_expired_staging(
            StagingCleanupRequest {
                peer: peer.clone(),
                parent_directory: parent.clone(),
                current_time: UNIX_EPOCH + Duration::from_secs(10 * 86_400),
                keep_bak_days: 5,
                keep_tmp_days: 2,
            },
            &file_operations,
        )
        .expect_err("cleanup should report a remove failure");

    assert_eq!(failure.peer, peer);
    assert_eq!(failure.parent_directory, parent);
    assert_eq!(failure.area, StagingCleanupArea::Bak);
    assert_eq!(failure.failed_path, expired_path);
    assert_eq!(
        failure.timestamp_directory,
        Some(expired_timestamp.to_owned())
    );
    assert_eq!(
        failure.operation,
        StagingCleanupFailureOperation::RemoveTimestampDirectory
    );
    assert_eq!(
        failure.cause,
        StagingCleanupFailureCause::Filesystem(remove_error)
    );
}

#[derive(Clone, Debug)]
enum ListingOutcome {
    Missing,
    Present(Vec<String>),
    Err(StagingCleanupOperationError),
}

struct RecordingFileOperations {
    listings: HashMap<String, ListingOutcome>,
    remove_errors: HashMap<String, StagingCleanupOperationError>,
    listed_paths: Mutex<Vec<String>>,
    removed_paths: Mutex<Vec<String>>,
}

impl RecordingFileOperations {
    fn empty() -> Self {
        Self {
            listings: HashMap::new(),
            remove_errors: HashMap::new(),
            listed_paths: Mutex::new(Vec::new()),
            removed_paths: Mutex::new(Vec::new()),
        }
    }

    fn new<const N: usize>(listings: [(String, Vec<&str>); N]) -> Self {
        Self::with_listing_outcomes(listings.map(|(path, timestamp_directories)| {
            (
                path,
                ListingOutcome::Present(
                    timestamp_directories
                        .into_iter()
                        .map(str::to_owned)
                        .collect::<Vec<_>>(),
                ),
            )
        }))
    }

    fn with_listing_outcomes<const N: usize>(listings: [(String, ListingOutcome); N]) -> Self {
        Self {
            listings: listings.into_iter().collect(),
            remove_errors: HashMap::new(),
            listed_paths: Mutex::new(Vec::new()),
            removed_paths: Mutex::new(Vec::new()),
        }
    }

    fn with_remove_errors<const N: usize>(
        mut self,
        errors: [(String, StagingCleanupOperationError); N],
    ) -> Self {
        self.remove_errors = errors.into_iter().collect();
        self
    }

    fn listed_paths(&self) -> Vec<String> {
        self.listed_paths.lock().unwrap().clone()
    }

    fn removed_paths(&self) -> Vec<String> {
        self.removed_paths.lock().unwrap().clone()
    }
}

impl StagingCleanupFileOperations for RecordingFileOperations {
    fn list_direct_timestamp_directories(
        &self,
        _peer: &StagingCleanupPeer,
        path: &str,
    ) -> Result<StagingCleanupDirectoryListing, StagingCleanupOperationError> {
        self.listed_paths.lock().unwrap().push(path.to_owned());

        match self
            .listings
            .get(path)
            .cloned()
            .unwrap_or(ListingOutcome::Missing)
        {
            ListingOutcome::Missing => Ok(StagingCleanupDirectoryListing::Missing),
            ListingOutcome::Present(direct_timestamp_directories) => {
                Ok(StagingCleanupDirectoryListing::Present {
                    direct_timestamp_directories,
                })
            }
            ListingOutcome::Err(error) => Err(error),
        }
    }

    fn remove_timestamp_directory_tree(
        &self,
        _peer: &StagingCleanupPeer,
        path: &str,
    ) -> Result<(), StagingCleanupOperationError> {
        self.removed_paths.lock().unwrap().push(path.to_owned());
        match self.remove_errors.get(path) {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}
