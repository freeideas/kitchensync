use std::time::SystemTime;

pub const SNAPSHOT_ROOT_PARENT_ID: &str = "JyBskcNRrBK";

pub type SnapshotIdentityResult<T> = Result<T, SnapshotIdentityError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotIdentityError {
    pub kind: SnapshotIdentityErrorKind,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotIdentityErrorKind {
    InvalidRelativePath,
    TimestampOutOfRange,
    SystemClockUnavailable,
}

pub trait SnapshotIdentity: Send + Sync {
    /// Returns the deterministic snapshot row ID for one entry below the sync
    /// root.
    ///
    /// The input must be a slash-separated relative path with no leading slash,
    /// no trailing slash, no repeated slash separators, and no `.` or `..`
    /// components. Empty strings are invalid, and the sync root itself is not
    /// accepted because it has no snapshot row. File and directory paths use
    /// the same rule. Invalid path input returns `InvalidRelativePath` and no
    /// path ID.
    ///
    /// For a valid input, the returned ID is deterministic, is always 11
    /// US-ASCII characters, and is the left-padded base62 encoding of the
    /// xxHash64 seed-0 value for the full relative path. The base62 alphabet is
    /// exactly `0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz`.
    fn path_id(&self, relative_path: &str) -> SnapshotIdentityResult<String>;

    /// Returns the deterministic parent row ID for one entry below the sync
    /// root.
    ///
    /// The input follows the same validation rules and error behavior as
    /// `path_id`. Entries directly under the sync root return
    /// `SNAPSHOT_ROOT_PARENT_ID`. Deeper entries return the same path ID that
    /// `path_id` would return for the slash-separated parent directory. The
    /// sync root itself is invalid because no row is stored for it.
    fn parent_path_id(&self, relative_path: &str) -> SnapshotIdentityResult<String>;

    /// Formats a caller-supplied UTC time for snapshot columns, timestamped
    /// KitchenSync paths, and log output.
    ///
    /// The returned string uses exactly `YYYY-MM-DD_HH-mm-ss_ffffffZ`,
    /// represents a UTC value at microsecond precision, and sorts
    /// lexicographically in the same order as its represented UTC time. Values
    /// that cannot be represented in that format return `TimestampOutOfRange`
    /// instead of a malformed timestamp.
    fn format_utc_timestamp(&self, time: SystemTime) -> SnapshotIdentityResult<String>;

    /// Generates one process-local UTC timestamp string.
    ///
    /// Each call reads the current UTC time, drops any sub-microsecond
    /// remainder, compares the resulting UTC microsecond value with the last
    /// value generated in this process, and uses the later of the current value
    /// or one microsecond after the last generated value. Successful generated
    /// timestamps are therefore strictly increasing within one process and use
    /// the same `YYYY-MM-DD_HH-mm-ss_ffffffZ` format as `format_utc_timestamp`.
    ///
    /// Callers that update `last_seen` must call this method separately for
    /// each snapshot row instead of reusing a generated value across rows.
    /// Failure to read the system clock returns `SystemClockUnavailable`.
    /// Failure to format the selected UTC value returns `TimestampOutOfRange`.
    fn generate_timestamp(&self) -> SnapshotIdentityResult<String>;
}
