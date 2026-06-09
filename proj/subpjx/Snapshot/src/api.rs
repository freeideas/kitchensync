//! Public interface for the `Snapshot` subproject.
//!
//! Snapshot owns every peer's SQLite snapshot database: its schema, the path
//! hashing that gives each tracked entry a stable identity, the run-wide
//! monotonic timestamp source, the per-peer row reads and updates that record
//! what each peer holds, the tombstones that record deletions, the
//! opportunistic cleanup of old rows, and the download, recovery, and
//! SWAP-staged upload of the `snapshot.db` file itself.
//!
//! All knowledge of "what a peer held last time we looked" lives here. Other
//! components ask Snapshot for a peer's prior state and tell it the new state
//! they observed or intend; nothing else opens a snapshot database, computes a
//! path identity, or decides where the file lives on a peer. Snapshot records
//! state and decisions handed to it: it does not list peers, classify entries,
//! apply the canon/subordinate/bidirectional rules, perform user-file copies or
//! BAK displacement, emit progress, or own the dry-run decision.

/// A single tracked entry as recorded in a peer's snapshot database.
///
/// The row shape mirrors the `snapshot` table columns (013.4 through 013.16).
/// `byte_size` is the file size in bytes for a regular file and `-1` for a
/// directory (013.13, 013.14). A file and a directory that share the same
/// canonical path share the same `id`, so at most one row exists per tracked
/// path (013.20).
pub struct SnapshotRow {
    /// The entry's stable identity: the base62 xxHash64 of its canonical
    /// relative path (014.1 through 014.7). Primary key of the table.
    pub id: String,
    /// The identity of the entry's parent path. A root-level entry's parent is
    /// the identity of the sentinel `/` (014.12).
    pub parent_id: String,
    /// The entry's final path segment.
    pub basename: String,
    /// The entry's modification time, in the timestamp format produced by
    /// [`Snapshot::now`].
    pub mod_time: String,
    /// The size in bytes for a regular file, or `-1` for a directory (013.13,
    /// 013.14).
    pub byte_size: i64,
    /// The fresh timestamp at which traversal last confirmed the entry present,
    /// or `None` when no presence has yet been confirmed (for example a row that
    /// exists only because of a push decision).
    pub last_seen: Option<String>,
    /// The tombstone timestamp recording when the entry was observed deleted, or
    /// `None` for a live entry. This is a copied estimate taken from the row's
    /// `last_seen`, not a freshly generated timestamp, and is exempt from the
    /// timestamp uniqueness rule (015.9, 015.10).
    pub deleted_time: Option<String>,
}

/// A failure surfaced by Snapshot to its caller.
///
/// Snapshot surfaces database-open and SQLite errors and the transport error
/// categories raised while downloading, recovering, or uploading `snapshot.db`;
/// it does not decide whether such a failure aborts the run.
pub enum SnapshotError {
    /// A database-open or SQLite error from working with the local snapshot
    /// database. The string carries the underlying detail.
    Database(String),
    /// A transport failure raised while downloading, recovering, or uploading
    /// `snapshot.db`. The string is the transport error category, surfaced
    /// unchanged: one of `not found`, `permission denied`, or `I/O error`.
    Transport(String),
}

/// The per-run snapshot service.
///
/// A single shared instance is used for the whole run, so `Arc<dyn Snapshot>`
/// is the handle other components hold. `Send + Sync` is required so that
/// handle can be shared across the concurrent components that read and update
/// peer state. The monotonic timestamp generator behind [`Snapshot::now`] is a
/// single source for the whole process, and every peer database is managed
/// through this one instance so the schema, hashing, and timestamp rules stay
/// uniform.
///
/// Peers are named throughout by their winning (canonical) URL, the same
/// identity other components use.
pub trait Snapshot: Send + Sync {
    /// Compute the stable identity of a tracked entry from its relative path.
    ///
    /// The identity is the xxHash64 (seed 0) of the entry's canonical relative
    /// path, base62-encoded with digits `0-9`, then `A-Z`, then `a-z`,
    /// zero-padded to an 11-character string (014.1, 014.2, 014.3). The path is
    /// canonicalized before hashing to use forward slashes and to carry no
    /// leading or trailing slash, so a file and a directory with the same path
    /// produce the same identity (014.4 through 014.7).
    ///
    /// Pass an entry's own path to get its `id`, and its parent's path to get
    /// its `parent_id`. The worked examples hold exactly: `docs/readme.txt`
    /// hashes to the identity of `docs/readme.txt`, directory `docs/notes` to
    /// the identity of `docs/notes`, and both have parent identity equal to the
    /// identity of `docs` (014.8 through 014.11). Pass the sentinel `/` to get
    /// the parent identity of a root-level entry (014.12). The sync root itself
    /// is never tracked; only its children are (014.13).
    fn path_identity(&self, relative_path: &str) -> String;

    /// Return a fresh "now" timestamp from the run-wide monotonic generator.
    ///
    /// The format is `YYYY-MM-DD_HH-mm-ss_ffffffZ`: UTC, microsecond precision,
    /// lexicographically sortable and filesystem-safe, and shared with the
    /// components that name BAK/ and TMP/ directories and that write log output
    /// (015.1 through 015.5).
    ///
    /// Every call returns a value strictly greater than any this instance has
    /// returned before in the process, adding one microsecond on collision, so
    /// no two freshly generated timestamps in one run are equal and they sort
    /// chronologically as plain strings (015.6, 015.7, 015.8).
    fn now(&self) -> String;

    /// Prepare a peer's local working snapshot database for the run.
    ///
    /// On a normal run, applies the five snapshot SWAP recovery states before
    /// deciding whether the peer has history, honoring last-upload-wins for
    /// overlapping runs (016.13 through 016.21). Then downloads the peer's
    /// `snapshot.db` to a local temporary path `{tmp}/{uuid}/snapshot.db`, where
    /// all later reads and writes happen; the peer copy is left untouched until
    /// writeback (016.4, 016.5). When the transport reports the peer has no
    /// snapshot, a new empty database is created locally (016.6).
    ///
    /// When `dry_run` is true, skip the peer-side SWAP recovery entirely and
    /// download the peer's live `.kitchensync/snapshot.db` as-is (024.2, 024.3);
    /// the local temp working copy is still created and later updated, because
    /// it is local-only state (024.6).
    ///
    /// The created or downloaded database has exactly one table named
    /// `snapshot`, no view, the columns and indexes the schema requires, and at
    /// most one row per tracked path (013.1 through 013.20).
    ///
    /// Surfaces transport errors from the download or recovery and database-open
    /// errors from the local file; it does not decide whether such a failure
    /// aborts the run.
    fn open(&self, peer: &str, dry_run: bool) -> Result<(), SnapshotError>;

    /// Commit the run's work for a peer and write the snapshot back to it.
    ///
    /// Commits or rolls back all database work and closes every connection,
    /// statement, and cursor so the uploaded file is self-contained and opens
    /// standalone with all of the run's changes committed (016.7). On a normal
    /// run, then writes back through the snapshot SWAP path: writes and closes
    /// `.kitchensync/SWAP/snapshot.db/new`, renames the live `snapshot.db` to
    /// `.kitchensync/SWAP/snapshot.db/old` when it exists, renames `new` into
    /// place, then deletes `old`, never relying on rename-over-existing (016.8
    /// through 016.12).
    ///
    /// When `dry_run` is true, skip the SWAP-staged upload entirely so no peer
    /// snapshot state changes; the local temp working copy still receives its
    /// commit and close but is never written back to the peer (024.18).
    ///
    /// The peer's `.kitchensync/snapshot.db` is never modified in place; it
    /// reaches the peer only through this SWAP-staged writeback. An upload that
    /// fails leaves the live file and SWAP state in a state the next run's
    /// recovery can resolve, and is not rolled back beyond that (016.20,
    /// 016.21). Surfaces the transport and SQLite errors raised while
    /// committing, closing, or uploading.
    fn writeback(&self, peer: &str, dry_run: bool) -> Result<(), SnapshotError>;

    /// Read a peer's prior recorded state for one tracked path, by identity.
    ///
    /// Returns the stored row so another component can compare it against what
    /// it observes, or `None` when the peer has no row for that identity.
    fn read_row(&self, peer: &str, id: &str) -> Result<Option<SnapshotRow>, SnapshotError>;

    /// Record that traversal confirmed an entry present on a peer.
    ///
    /// Upserts the row's `mod_time` and `byte_size`, sets `last_seen` to a fresh
    /// timestamp from [`Snapshot::now`], and clears `deleted_time` to NULL
    /// (017.1 through 017.4). When no row exists the entry's `id`, `parent_id`,
    /// and `basename` create it.
    fn record_present(
        &self,
        peer: &str,
        id: &str,
        parent_id: &str,
        basename: &str,
        mod_time: &str,
        byte_size: i64,
    ) -> Result<(), SnapshotError>;

    /// Record that traversal confirmed a previously known entry absent on a
    /// peer.
    ///
    /// For a live row (`deleted_time` NULL), sets `deleted_time` to that row's
    /// current `last_seen` without touching `last_seen` (017.5, 017.6). A row
    /// that already carries a `deleted_time` is left unchanged (017.7).
    fn record_absent(&self, peer: &str, id: &str) -> Result<(), SnapshotError>;

    /// Record a push decision: the winning state this peer is to receive.
    ///
    /// Upserts the winning `mod_time` and `byte_size` with `deleted_time` NULL
    /// and without setting `last_seen`, leaving `last_seen` NULL when no prior
    /// row exists (017.8 through 017.11). When no row exists the entry's `id`,
    /// `parent_id`, and `basename` create it.
    ///
    /// `last_seen` stays unset until the copy actually completes (see
    /// [`Snapshot::record_copied`]), so a copy that never completes leaves this
    /// row with `deleted_time` NULL and `last_seen` unchanged and is
    /// re-enqueued next run (017.21, 017.22).
    fn record_push(
        &self,
        peer: &str,
        id: &str,
        parent_id: &str,
        basename: &str,
        mod_time: &str,
        byte_size: i64,
    ) -> Result<(), SnapshotError>;

    /// Record that a queued copy or an inline directory creation completed on a
    /// peer.
    ///
    /// Sets the entry's `last_seen` to a fresh timestamp (017.12, 017.13). Call
    /// this only after the owning component reports the filesystem operation
    /// succeeded; when an inline filesystem operation fails the caller does not
    /// call this, leaving the row unchanged (017.14).
    fn record_copied(&self, peer: &str, id: &str) -> Result<(), SnapshotError>;

    /// Record a successful displacement of an entry on a peer, cascading the
    /// tombstone to its descendants.
    ///
    /// Sets the entry's `deleted_time` to the row's current `last_seen`, then
    /// cascades `deleted_time` to descendant rows reached through `parent_id`
    /// links, without overwriting descendants that already have `deleted_time`
    /// set and without touching unrelated rows (017.15 through 017.18).
    ///
    /// The cascade runs against this peer's own snapshot database only, and is
    /// run once per peer after that peer's displacement succeeds, even when
    /// several peers lose the same subtree (017.19, 017.20).
    fn record_displaced(&self, peer: &str, id: &str) -> Result<(), SnapshotError>;

    /// Opportunistically remove aged rows from a peer's snapshot database.
    ///
    /// Removes tombstone rows (`deleted_time IS NOT NULL`) older than
    /// `keep_del_days` and keeps those within the window (018.1, 018.2). Removes
    /// a stale live row (`deleted_time` NULL) that traversal did not visit when
    /// its `last_seen` is older than `keep_del_days` (018.3).
    ///
    /// This maintenance is opportunistic: it never delays the first directory
    /// scan or the first eligible copy, and the run exits 0 even if it does not
    /// finish (018.4, 018.5, 018.6).
    fn prune(&self, peer: &str, keep_del_days: u32) -> Result<(), SnapshotError>;
}
