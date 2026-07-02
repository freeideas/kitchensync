use std::io::{Read, Write};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedTransferPeer {
    pub id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StagedTransferModificationTime {
    pub seconds_since_unix_epoch: i64,
    pub nanoseconds: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedTransferRequest {
    pub source_peer: StagedTransferPeer,
    pub destination_peer: StagedTransferPeer,
    pub relative_source_file_path: String,
    pub relative_destination_file_path: String,
    pub user_path: String,
    pub winning_modification_time: StagedTransferModificationTime,
    pub winning_byte_size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StagedTransferTryOutcome {
    Success,
    SkipRestOfRun(StagedTransferFailure),
    RecoveryFailure(StagedTransferOperationError),
    Failure(StagedTransferFailure),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedTransferFailure {
    pub phase: StagedTransferFailurePhase,
    pub swap_old_state: StagedTransferSwapOldState,
    pub error: StagedTransferOperationError,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StagedTransferFailurePhase {
    ReadSource,
    WriteSwapNew,
    MoveExistingToSwapOld,
    RenameFinal,
    SetModTime,
    ArchiveOld,
    Cleanup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StagedTransferSwapOldState {
    NotCreated,
    Created,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedTransferOperationError {
    pub transport_error_category: Option<StagedTransferTransportErrorCategory>,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StagedTransferTransportErrorCategory {
    NotFound,
    PermissionDenied,
    IoError,
}

pub trait StagedTransferFileOperations: Send + Sync {
    /// Returns whether the exact peer path currently names an existing file.
    ///
    /// The check is scoped to the supplied connected peer handle and path. It
    /// must not create, delete, rename, or modify peer data. Errors are
    /// reported to StagedTransfer so the active transfer phase can be returned
    /// to the caller.
    fn file_exists(
        &self,
        peer: &StagedTransferPeer,
        path: &str,
    ) -> Result<bool, StagedTransferOperationError>;

    /// Opens the exact peer file path for streaming reads.
    ///
    /// The returned reader must let StagedTransfer read incrementally. It must
    /// not require the whole file to be buffered in memory before destination
    /// writing can begin. Errors opening or reading the source are reported as
    /// `StagedTransferFailurePhase::ReadSource`.
    fn open_for_read(
        &self,
        peer: &StagedTransferPeer,
        path: &str,
    ) -> Result<Box<dyn Read + Send>, StagedTransferOperationError>;

    /// Creates the exact peer path as a new file for streaming writes.
    ///
    /// The path is for SWAP `new`, not the final destination. The operation
    /// must not overwrite an existing path and must let StagedTransfer write
    /// incrementally while it reads the source. Errors creating or writing the
    /// destination stream are reported as
    /// `StagedTransferFailurePhase::WriteSwapNew`.
    fn create_new_for_write(
        &self,
        peer: &StagedTransferPeer,
        path: &str,
    ) -> Result<Box<dyn Write + Send>, StagedTransferOperationError>;

    /// Creates the supplied directory path and any missing parent directories.
    ///
    /// This is used only for SWAP and BAK directories derived for the current
    /// transfer try. It must not create, delete, or move user files outside
    /// those staging paths. The active phase that requested the directory
    /// determines the failure phase returned to the caller.
    fn create_directory_all(
        &self,
        peer: &StagedTransferPeer,
        path: &str,
    ) -> Result<(), StagedTransferOperationError>;

    /// Renames one peer path to a destination path that must be missing.
    ///
    /// StagedTransfer uses this operation to move the existing destination to
    /// SWAP `old`, to move SWAP `new` into the final destination, and to
    /// archive SWAP `old` to BAK. The operation must not rely on
    /// rename-over-existing behavior. The active transfer step determines the
    /// returned failure phase.
    fn rename_to_missing_path(
        &self,
        peer: &StagedTransferPeer,
        source_path: &str,
        destination_path: &str,
    ) -> Result<(), StagedTransferOperationError>;

    /// Deletes the exact peer file path.
    ///
    /// StagedTransfer uses this to remove SWAP `new` after a failure that
    /// happens before SWAP `old` exists. A deletion failure during that
    /// best-effort cleanup does not change the originally failed transfer
    /// phase.
    fn delete_file(
        &self,
        peer: &StagedTransferPeer,
        path: &str,
    ) -> Result<(), StagedTransferOperationError>;

    /// Removes the exact peer directory path only when it is empty.
    ///
    /// Successful transfers remove the empty SWAP directory for the encoded
    /// basename and then any now-empty parent SWAP directories for this
    /// transfer. A failure after all replacement and archive work succeeded is
    /// reported as `StagedTransferFailurePhase::Cleanup`.
    fn remove_empty_directory(
        &self,
        peer: &StagedTransferPeer,
        path: &str,
    ) -> Result<(), StagedTransferOperationError>;

    /// Sets the exact peer file path to the winning modification time.
    ///
    /// StagedTransfer calls this only after SWAP `new` has become the final
    /// destination. If it fails, the replacement is not undone and the returned
    /// failure phase is `StagedTransferFailurePhase::SetModTime`.
    fn set_modification_time(
        &self,
        peer: &StagedTransferPeer,
        path: &str,
        modification_time: StagedTransferModificationTime,
    ) -> Result<(), StagedTransferOperationError>;
}

pub trait StagedTransferSwapRecovery: Send + Sync {
    /// Recovers existing user-data SWAP state for one encoded target basename.
    ///
    /// StagedTransfer calls this before writing any replacement content. A
    /// failure aborts the try before SWAP `old` exists and before replacement
    /// begins, and the result is `StagedTransferTryOutcome::RecoveryFailure`.
    fn recover_user_data_swap(
        &self,
        peer: &StagedTransferPeer,
        target_parent_path: &str,
        basename: &str,
        encoded_basename: &str,
    ) -> Result<(), StagedTransferOperationError>;
}

pub trait StagedTransferTimestampGenerator: Send + Sync {
    /// Returns the timestamp path segment for a BAK archive directory.
    ///
    /// StagedTransfer calls this only after SWAP `new` has become the final
    /// destination and only when SWAP `old` exists. The returned value is used
    /// as `<timestamp>` in
    /// `<target-parent>/.kitchensync/BAK/<timestamp>/<basename>`.
    fn next_bak_timestamp(&self) -> String;
}

pub trait StagedTransfer: Send + Sync {
    /// Runs one granted file-copy try through SWAP staging and returns its
    /// phase-specific outcome.
    ///
    /// The request supplies connected source and destination peer handles,
    /// relative source and destination file paths, the slash-separated user
    /// path used for reporting, the winning modification time, and the winning
    /// byte size. StagedTransfer derives the destination parent and basename
    /// from `relative_destination_file_path`, percent-encodes the basename when
    /// needed so it is one path segment, and uses exactly
    /// `<target-parent>/.kitchensync/SWAP/<encoded-basename>/new` for SWAP
    /// `new` and
    /// `<target-parent>/.kitchensync/SWAP/<encoded-basename>/old` for SWAP
    /// `old`.
    ///
    /// The operation first recovers existing user-data SWAP state for the
    /// encoded basename. If recovery fails, no replacement begins and the
    /// result is `StagedTransferTryOutcome::RecoveryFailure`. For a normal try,
    /// it streams source content into SWAP `new`, moves an existing destination
    /// to SWAP `old` when one exists, renames SWAP `new` into the final
    /// destination, sets the final modification time, archives SWAP `old` to
    /// `<target-parent>/.kitchensync/BAK/<timestamp>/<basename>` when SWAP
    /// `old` exists, and removes the empty SWAP directories for the transfer.
    ///
    /// Replacement content must reach the destination only through SWAP `new`.
    /// The operation must never write replacement content directly to the final
    /// destination and must never rely on rename-over-existing behavior for the
    /// final user path. It starts destination writing while reading from the
    /// source and uses fixed buffer memory independent of source file size.
    ///
    /// `Success` means the final file is in place, the winning modification
    /// time has been set, any SWAP `old` has been archived, and empty SWAP
    /// directories for this transfer have been removed. A first-time
    /// destination creates no BAK entry. If moving an existing destination to
    /// SWAP `old` fails, the original destination remains in place, SWAP `new`
    /// is deleted when possible, and the result is `SkipRestOfRun` with phase
    /// `MoveExistingToSwapOld` and `swap_old_state` `NotCreated`.
    ///
    /// If a failure happens before SWAP `old` exists, SWAP `new` is deleted
    /// when possible before returning. If a failure happens after SWAP `old`
    /// exists and before replacement fully completes, peer-visible SWAP state
    /// is left for later recovery. A `SetModTime` failure after SWAP `new` has
    /// become the destination does not undo the replacement. An `ArchiveOld`
    /// failure leaves SWAP `old` for later recovery. A final SWAP cleanup
    /// failure after replacement and archive work succeeded returns `Cleanup`.
    ///
    /// This operation does not decide scheduling, copy-slot accounting, retry
    /// counts, file eligibility, peer roles, exclusions, peer connection,
    /// authentication, traversal-wide recovery, snapshot updates, stdout
    /// formatting, or exhausted-try behavior. Repeated calls run repeated copy
    /// tries against the supplied peer operations and are not idempotent.
    fn run_transfer_try(
        &self,
        request: StagedTransferRequest,
        file_operations: &dyn StagedTransferFileOperations,
        swap_recovery: &dyn StagedTransferSwapRecovery,
        timestamp_generator: &dyn StagedTransferTimestampGenerator,
    ) -> StagedTransferTryOutcome;
}
