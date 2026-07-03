use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatRulesPeerIdentityRequest {
    pub peer_url: String,
    pub current_working_directory: PathBuf,
    pub os_username: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatRulesTimestamp {
    pub(crate) text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatRulesSnapshotPathIds {
    pub id: String,
    pub parent_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormatRulesDeletionEstimateUpdate {
    Write(FormatRulesTimestamp),
    NoWrite,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatRulesUserSwapPaths {
    pub directory_path: String,
    pub new_path: String,
    pub old_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatRulesSnapshotSwapPaths {
    pub new_path: String,
    pub old_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatRulesValidationError {
    pub kind: FormatRulesValidationErrorKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatRulesValidationErrorKind {
    InvalidPeerUrl,
    MissingOsUsername,
    InvalidRelativePath,
    RootSnapshotPath,
    InvalidTimestamp,
    InvalidSwapBasename,
}

pub trait FormatRules: Send + Sync {
    /// Normalizes one peer argument into the deterministic identity string used
    /// for peer comparison and lookup. An argument with no URL scheme is
    /// treated as `file://`, `file://` paths are resolved to absolute paths
    /// from the supplied current working directory, schemes and hostnames are
    /// lowercased, SFTP port 22 is removed, consecutive path slashes are
    /// collapsed, trailing path slashes are removed, unreserved percent-encoded
    /// characters are decoded, query strings are stripped, and an SFTP URL
    /// without a username receives the supplied OS username. This operation
    /// does not connect to a peer, inspect the filesystem, create roots,
    /// authenticate, parse query-string settings, or choose fallback winners.
    /// Invalid URL text, invalid path context, and a missing OS username when
    /// one is required for SFTP return a validation error instead of being
    /// silently rewritten into another meaning.
    fn normalize_peer_identity(
        &self,
        request: FormatRulesPeerIdentityRequest,
    ) -> Result<String, FormatRulesValidationError>;

    /// Validates one external relative user-tree path and returns the accepted
    /// path text unchanged. A valid path uses slash separators, has no leading
    /// slash, no trailing slash, no backslash separator, no empty segment, no
    /// `.` segment, no `..` segment, and no NUL character. The returned string
    /// is the exact relative path text used for progress output and snapshot
    /// ID hashing. Malformed input returns a validation error instead of being
    /// normalized to a different path.
    fn validate_relative_path(
        &self,
        path: &str,
    ) -> Result<String, FormatRulesValidationError>;

    /// Returns the snapshot row `id` and `parent_id` for one non-root entry.
    /// The entry path must be a valid relative slash path. The entry `id` is an
    /// 11-character, left-zero-padded base62 encoding of xxHash64 seed 0 over
    /// the entry's full relative path bytes. The `parent_id` is the same
    /// encoding over the parent directory's relative path bytes; for a root
    /// entry it is the encoding over the sentinel bytes `/`. Files and
    /// directories at the same path receive the same IDs. The sync root itself
    /// is rejected because it never receives a snapshot row.
    fn snapshot_path_ids(
        &self,
        relative_path: &str,
    ) -> Result<FormatRulesSnapshotPathIds, FormatRulesValidationError>;

    /// Parses one timestamp string that crossed this boundary from external
    /// text. Only UTC `YYYY-MM-DD_HH-mm-ss_ffffffZ` with exactly six
    /// microsecond digits is accepted. Malformed timestamp text returns a
    /// validation error instead of being rounded, timezone-adjusted, or
    /// accepted in another format.
    fn parse_timestamp(
        &self,
        timestamp: &str,
    ) -> Result<FormatRulesTimestamp, FormatRulesValidationError>;

    /// Formats one timestamp value for snapshot columns, BAK directory names,
    /// TMP directory names, and log output. The returned timestamp is always
    /// UTC `YYYY-MM-DD_HH-mm-ss_ffffffZ` with exactly six microsecond digits.
    /// Calling this repeatedly with the same input is idempotent.
    fn format_timestamp(&self, timestamp: SystemTime) -> FormatRulesTimestamp;

    /// Generates a process-local current timestamp for `last_seen` writes, BAK
    /// directory names, or TMP directory names. Each returned value is strictly
    /// greater than every generated current timestamp already returned by this
    /// child in the same process; if the system clock has not advanced, the
    /// generated value advances the previous generated timestamp by one
    /// microsecond. Copied deletion estimates are not produced by this
    /// operation and do not receive uniqueness treatment.
    fn current_timestamp(&self) -> FormatRulesTimestamp;

    /// Returns the canonical timestamp text for a timestamp value produced by
    /// `parse_timestamp`, `format_timestamp`, or `current_timestamp`. The text
    /// is always UTC `YYYY-MM-DD_HH-mm-ss_ffffffZ` with exactly six
    /// microsecond digits. Calling this repeatedly for the same timestamp is
    /// idempotent.
    fn timestamp_text(&self, timestamp: &FormatRulesTimestamp) -> String;

    /// Returns the instant represented by a timestamp value produced by
    /// `parse_timestamp`, `format_timestamp`, or `current_timestamp`. Callers
    /// that determine BAK or TMP cleanup age use the timestamp parsed from the
    /// path component, not filesystem metadata. Calling this repeatedly for the
    /// same timestamp is idempotent.
    fn timestamp_system_time(&self, timestamp: &FormatRulesTimestamp) -> SystemTime;

    /// Chooses the `deleted_time` update for a confirmed absence. If the row
    /// already has `deleted_time`, no write is needed and the existing value is
    /// left unchanged. If `deleted_time` is absent, the value to write is the
    /// row's existing `last_seen`; no generated current timestamp is substituted.
    fn confirmed_absence_deleted_time(
        &self,
        existing_last_seen: &FormatRulesTimestamp,
        existing_deleted_time: Option<&FormatRulesTimestamp>,
    ) -> FormatRulesDeletionEstimateUpdate;

    /// Returns the deletion estimate for an entry displaced to BAK. The value
    /// is copied from that peer row's existing `last_seen`; no generated
    /// current timestamp is substituted.
    fn displacement_deleted_time(
        &self,
        existing_last_seen: &FormatRulesTimestamp,
    ) -> FormatRulesTimestamp;

    /// Returns the deletion estimate to apply to descendant rows during a
    /// displacement cascade. The value is the displaced entry's copied deletion
    /// estimate and is used only for affected descendant rows on the same peer;
    /// this operation does not execute SQL, walk descendants, or choose the
    /// subtree.
    fn displacement_cascade_deleted_time(
        &self,
        displaced_deleted_time: &FormatRulesTimestamp,
    ) -> FormatRulesTimestamp;

    /// Formats the BAK timestamp directory path at an affected parent
    /// directory. `None` means the sync root parent; `Some` must be a valid
    /// relative slash path. The returned path uses
    /// `.kitchensync/BAK/<timestamp>/` under that parent and includes the
    /// trailing slash. Cleanup age is determined from the `<timestamp>` path
    /// component, not from filesystem metadata.
    fn bak_directory_path(
        &self,
        parent_relative_path: Option<&str>,
        timestamp: &FormatRulesTimestamp,
    ) -> Result<String, FormatRulesValidationError>;

    /// Formats the TMP timestamp directory path. The returned path is exactly
    /// `.kitchensync/TMP/<timestamp>/` and includes the trailing slash. Cleanup
    /// age is determined from the `<timestamp>` path component, not from
    /// filesystem metadata.
    fn tmp_directory_path(&self, timestamp: &FormatRulesTimestamp) -> String;

    /// Formats the SWAP directory and its `new` and `old` child paths for one
    /// user entry under an affected parent directory. `None` means the sync
    /// root parent; `Some` must be a valid relative slash path. The basename is
    /// encoded as one percent-encoded path segment under `.kitchensync/SWAP/`.
    /// Callers pass only the target basename, never a whole relative path. A
    /// basename that cannot be represented as one encoded segment on every
    /// supported transport returns a validation error.
    fn user_swap_paths(
        &self,
        parent_relative_path: Option<&str>,
        target_basename: &str,
    ) -> Result<FormatRulesUserSwapPaths, FormatRulesValidationError>;

    /// Returns the exact snapshot database SWAP paths. The `new` path is
    /// `.kitchensync/SWAP/snapshot.db/new`, the `old` path is
    /// `.kitchensync/SWAP/snapshot.db/old`, and calling this repeatedly is
    /// idempotent.
    fn snapshot_swap_paths(&self) -> FormatRulesSnapshotSwapPaths;

    /// Compares a current file `mod_time` with a snapshot row `mod_time` for
    /// entry classification. The values are treated as the same when their
    /// absolute difference is no more than five seconds and different only
    /// when their absolute difference is more than five seconds.
    fn file_mod_times_same(
        &self,
        current_mod_time: &FormatRulesTimestamp,
        snapshot_mod_time: &FormatRulesTimestamp,
    ) -> bool;

    /// Compares one peer entry `mod_time` with the maximum peer `mod_time` for
    /// decision rules. A candidate within five seconds of the maximum is tied
    /// with that maximum; a candidate more than five seconds behind the maximum
    /// is not tied.
    fn peer_mod_time_tied_with_max(
        &self,
        candidate_mod_time: &FormatRulesTimestamp,
        max_mod_time: &FormatRulesTimestamp,
    ) -> bool;

    /// Compares one peer entry `mod_time` with the maximum peer `mod_time` for
    /// decision rules. A candidate more than five seconds behind the maximum is
    /// older than the maximum; a candidate within five seconds of the maximum
    /// is not older.
    fn peer_mod_time_older_than_max(
        &self,
        candidate_mod_time: &FormatRulesTimestamp,
        max_mod_time: &FormatRulesTimestamp,
    ) -> bool;

    /// Returns whether a file deletion estimate wins over an existing file
    /// `mod_time`. The deletion estimate wins only when it is more than five
    /// seconds newer than the file `mod_time`; values equal to or within five
    /// seconds of the file `mod_time` do not win.
    fn deletion_estimate_wins_over_file_mod_time(
        &self,
        deletion_estimate: &FormatRulesTimestamp,
        file_mod_time: &FormatRulesTimestamp,
    ) -> bool;

    /// Returns whether an absent-unconfirmed file counts as a deletion. It
    /// counts as a deletion only when the row's `last_seen` exceeds the maximum
    /// live-file `mod_time` by more than five seconds; values equal to, older
    /// than, or within five seconds of the maximum do not count as deletion
    /// evidence.
    fn absent_unconfirmed_file_counts_as_deletion(
        &self,
        last_seen: &FormatRulesTimestamp,
        max_live_file_mod_time: &FormatRulesTimestamp,
    ) -> bool;

    /// Returns the newest live-file timestamp evidence for a live directory
    /// subtree. The input must contain only live file `mod_time` values; callers
    /// must not include directory `mod_time` values because directory decision
    /// timestamp evidence ignores them. A live directory subtree with no files
    /// returns no timestamp survival evidence.
    fn directory_live_file_timestamp_evidence(
        &self,
        live_file_mod_times: &[FormatRulesTimestamp],
    ) -> Option<FormatRulesTimestamp>;

    /// Returns whether a directory deletion estimate is newer than live-subtree
    /// file evidence. The estimate is newer only when it exceeds the newest
    /// live file `mod_time` by more than five seconds; values equal to or
    /// within five seconds are not newer. Directory `mod_time` values are not
    /// inputs to this comparison.
    fn directory_deletion_estimate_newer_than_live_file_evidence(
        &self,
        deletion_estimate: &FormatRulesTimestamp,
        newest_live_file_mod_time: &FormatRulesTimestamp,
    ) -> bool;
}
