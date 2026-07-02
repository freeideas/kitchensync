use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagingCleanupPeer {
    pub identity: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagingCleanupRequest {
    pub peer: StagingCleanupPeer,
    pub parent_directory: String,
    pub current_time: SystemTime,
    pub keep_bak_days: u64,
    pub keep_tmp_days: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StagingCleanupArea {
    Bak,
    Tmp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StagingCleanupDirectoryListing {
    Missing,
    Present {
        direct_timestamp_directories: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagingCleanupFailure {
    pub peer: StagingCleanupPeer,
    pub parent_directory: String,
    pub area: StagingCleanupArea,
    pub failed_path: String,
    pub timestamp_directory: Option<String>,
    pub operation: StagingCleanupFailureOperation,
    pub cause: StagingCleanupFailureCause,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StagingCleanupFailureOperation {
    InspectCleanupRoot,
    DetermineTimestampAge,
    RemoveTimestampDirectory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StagingCleanupFailureCause {
    Filesystem(StagingCleanupOperationError),
    InvalidTimestamp { message: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagingCleanupOperationError {
    pub category: Option<StagingCleanupOperationErrorCategory>,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StagingCleanupOperationErrorCategory {
    NotFound,
    PermissionDenied,
    NotDirectory,
    IoError,
}

pub trait StagingCleanupFileOperations: Send + Sync {
    /// Lists the direct timestamp directories below one cleanup root.
    ///
    /// `path` is exactly either `<parent>/.kitchensync/BAK` or
    /// `<parent>/.kitchensync/TMP` as supplied by StagingCleanup. A missing
    /// cleanup root returns `StagingCleanupDirectoryListing::Missing` and is
    /// not a cleanup failure. If the cleanup root exists but cannot be
    /// inspected as a directory, the operation must return an error. Returned
    /// names are single path components for direct child directories only; file
    /// entries and deeper descendants are not timestamp directories for this
    /// cleanup operation.
    fn list_direct_timestamp_directories(
        &self,
        peer: &StagingCleanupPeer,
        path: &str,
    ) -> Result<StagingCleanupDirectoryListing, StagingCleanupOperationError>;

    /// Removes one selected timestamp directory and its contents.
    ///
    /// `path` is exactly a BAK or TMP timestamp directory selected by
    /// StagingCleanup after age comparison. This operation must not remove the
    /// BAK root, the TMP root, `.kitchensync/SWAP`, or any live user path. A
    /// failure to remove the selected directory is reported back as a cleanup
    /// failure; StagingCleanup does not retry with a different path.
    fn remove_timestamp_directory_tree(
        &self,
        peer: &StagingCleanupPeer,
        path: &str,
    ) -> Result<(), StagingCleanupOperationError>;
}

pub trait StagingCleanup: Send + Sync {
    /// Removes expired BAK and TMP timestamp directories for one peer parent.
    ///
    /// The caller invokes this after it has processed the union of entry names
    /// at `request.parent_directory`; this method does not decide traversal
    /// ordering. Each call checks only
    /// `<parent>/.kitchensync/BAK` and `<parent>/.kitchensync/TMP` for the
    /// supplied parent directory. Missing cleanup roots are treated as having
    /// no timestamp directories. Existing cleanup roots that cannot be
    /// inspected cause failure.
    ///
    /// Age is determined only from each direct timestamp directory name, using
    /// `request.current_time` and the matching keep-days value. BAK timestamp
    /// directories older than `request.keep_bak_days` are removed; BAK
    /// timestamp directories that are not older are left in place. TMP
    /// timestamp directories older than `request.keep_tmp_days` are removed;
    /// TMP timestamp directories that are not older are left in place.
    /// Filesystem creation time, modification time, access time, and snapshot
    /// rows must not be used for cleanup age.
    ///
    /// Success means every expired BAK and TMP timestamp directory found for
    /// the supplied parent was removed, every unexpired BAK and TMP timestamp
    /// directory was left in place, and no SWAP directory was purged by age.
    /// Failure is returned when an existing BAK or TMP cleanup root cannot be
    /// inspected, a timestamp directory name cannot be interpreted for age
    /// comparison, or a selected timestamp directory cannot be removed. The
    /// failure includes peer, parent, area, path, timestamp, and cause context
    /// for caller reporting. This method does not retry, change retention
    /// values, delete unexpired directories, apply dry-run policy, format
    /// output, inspect live user entries, compare peers, or update snapshot
    /// rows.
    fn clean_expired_staging(
        &self,
        request: StagingCleanupRequest,
        file_operations: &dyn StagingCleanupFileOperations,
    ) -> Result<(), StagingCleanupFailure>;
}
