use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};

use stagingrecovery_stagingcleanup::{
    new, StagingCleanupDirectoryListing, StagingCleanupFileOperations,
    StagingCleanupOperationError, StagingCleanupPeer, StagingCleanupRequest,
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

struct RecordingFileOperations {
    listings: HashMap<String, Vec<String>>,
    listed_paths: Mutex<Vec<String>>,
    removed_paths: Mutex<Vec<String>>,
}

impl RecordingFileOperations {
    fn new<const N: usize>(listings: [(String, Vec<&str>); N]) -> Self {
        Self {
            listings: listings
                .into_iter()
                .map(|(path, timestamp_directories)| {
                    (
                        path,
                        timestamp_directories
                            .into_iter()
                            .map(str::to_owned)
                            .collect::<Vec<_>>(),
                    )
                })
                .collect(),
            listed_paths: Mutex::new(Vec::new()),
            removed_paths: Mutex::new(Vec::new()),
        }
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

        Ok(match self.listings.get(path) {
            Some(direct_timestamp_directories) => StagingCleanupDirectoryListing::Present {
                direct_timestamp_directories: direct_timestamp_directories.clone(),
            },
            None => StagingCleanupDirectoryListing::Missing,
        })
    }

    fn remove_timestamp_directory_tree(
        &self,
        _peer: &StagingCleanupPeer,
        path: &str,
    ) -> Result<(), StagingCleanupOperationError> {
        self.removed_paths.lock().unwrap().push(path.to_owned());
        Ok(())
    }
}
