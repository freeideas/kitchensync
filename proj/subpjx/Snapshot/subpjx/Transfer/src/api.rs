//! Public interface for the `Transfer` subproject.
//!
//! Transfer owns the snapshot database file as it moves between a peer and the
//! local temporary working directory. It never reads or writes rows, never
//! knows the schema, and never computes an identity or a timestamp: its single
//! concern is the bytes of `snapshot.db` and the SWAP state machine that gets
//! those bytes safely onto a peer (016.1 through 016.21).
//!
//! Transfer issues no network or filesystem calls against a peer directly. It
//! reaches the peer only through the [`PeerFiles`] capability its parent
//! supplies, which the parent backs with its Transport bound to one peer's
//! root. That Transport layer does the actual reads, writes, renames, and
//! deletes against the peer; Transfer only decides the sequence of those
//! operations.

use std::path::{Path, PathBuf};

/// The failure categories Transfer surfaces to its caller.
///
/// These are exactly the transport error categories raised while downloading,
/// recovering, or uploading `snapshot.db`, passed through unchanged. Transfer
/// does not decide whether such a failure aborts the run.
pub enum TransferError {
    /// The peer path does not exist.
    NotFound,
    /// The peer rejected the operation for lack of permission.
    PermissionDenied,
    /// Any other failure, including all network failures.
    Io,
}

/// The peer-bound filesystem capability Transfer's parent supplies.
///
/// One instance is bound to a single peer's selected root for the whole run;
/// every `path`, `remote`, `src`, and `dst` below is interpreted relative to
/// that root. The parent backs this handle with its Transport, so a failure
/// surfaces with the same [`TransferError`] category the underlying transport
/// raised.
///
/// `Send + Sync` is required so the handle can travel with a shared
/// `Arc<dyn Transfer>`.
pub trait PeerFiles: Send + Sync {
    /// Report whether a regular file exists at `path` on the peer.
    ///
    /// Used to read the SWAP state machine during recovery and writeback; it
    /// answers a yes/no question and does not move any bytes.
    fn exists(&self, path: &str) -> Result<bool, TransferError>;

    /// Copy the peer file at `remote` down to the local `local` path,
    /// overwriting any existing local file.
    ///
    /// Returns [`TransferError::NotFound`] when the peer has no file at
    /// `remote`; Transfer reads that as "the peer has no snapshot".
    fn download(&self, remote: &str, local: &Path) -> Result<(), TransferError>;

    /// Copy the local `local` file up to the peer path `remote`, creating any
    /// missing parent directories on the peer.
    ///
    /// `remote` must not already exist; the SWAP sequence only ever uploads to
    /// a fresh `new` name, so this never relies on overwrite.
    fn upload(&self, local: &Path, remote: &str) -> Result<(), TransferError>;

    /// Move the peer file `src` to `dst`.
    ///
    /// `dst` must not already exist: this never relies on rename-over-existing,
    /// so replacement succeeds on transports whose rename rejects an existing
    /// destination (016.12).
    fn rename(&self, src: &str, dst: &str) -> Result<(), TransferError>;

    /// Remove the peer file at `path`.
    fn delete(&self, path: &str) -> Result<(), TransferError>;

    /// Remove an empty directory at `path`. Errors are ignored by callers.
    fn delete_dir(&self, path: &str) -> Result<(), TransferError>;
}

/// The result of downloading a peer's snapshot to the local temp directory.
pub struct Downloaded {
    /// The fresh local path `{tmp}/{uuid}/snapshot.db` the snapshot now lives
    /// at, ready for the caller to open. Each download gets its own `uuid`
    /// directory, so the path is never shared between peers or runs.
    pub local_path: PathBuf,
    /// Whether the peer had existing snapshot history, determined only after
    /// SWAP recovery has been applied (016.13). False when the peer had no
    /// snapshot and a new empty database was created locally instead.
    pub had_history: bool,
}

/// The file-level half of Snapshot: it moves `snapshot.db` between a peer and
/// the local temp working directory through the SWAP state machine.
///
/// A peer's snapshot lives at `{peer-root}/.kitchensync/snapshot.db` and is a
/// rollback-journal SQLite file; only `snapshot.db` itself is part of a peer's
/// state, so Transfer never uploads a SQLite sidecar file (016.1, 016.2,
/// 016.3). The peer is addressed entirely through the [`PeerFiles`] handle
/// passed to each method, already bound to that peer's root; Transfer holds no
/// peer state of its own.
///
/// `Send + Sync` is required so `Arc<dyn Transfer>` is a shareable handle.
pub trait Transfer: Send + Sync {
    /// Apply startup SWAP recovery for a peer, resolving any half-finished
    /// writeback a previous run left behind so the live `snapshot.db` is
    /// consistent before it is read.
    ///
    /// Recovery reads `.kitchensync/SWAP/snapshot.db/old`, `.../new`, and the
    /// live `.kitchensync/snapshot.db`, then acts on exactly these states:
    ///
    /// - `old` exists and `snapshot.db` exists: delete `new` if present, then
    ///   delete `old` (016.14).
    /// - `old` exists, `new` exists, `snapshot.db` missing: rename `new` to
    ///   `snapshot.db`, then delete `old` (016.15).
    /// - `old` exists, `new` missing, `snapshot.db` missing: rename `old` to
    ///   `snapshot.db` (016.16).
    /// - `old` missing, `new` exists, `snapshot.db` exists: delete `new`
    ///   (016.17).
    /// - `old` missing, `new` exists, `snapshot.db` missing: rename `new` to
    ///   `snapshot.db` (016.18).
    ///
    /// Any other state has no SWAP work and leaves the peer untouched, so the
    /// call is safe to make on a peer that was never mid-writeback. This must
    /// run before [`Transfer::download`] for the same peer, so history is
    /// determined only after recovery (016.13).
    ///
    /// When `dry_run` is true, skip peer-side SWAP recovery entirely and leave
    /// every peer file as it is (024.2).
    ///
    /// Surfaces the transport error categories raised while reading, renaming,
    /// or deleting; it does not decide whether such a failure aborts the run.
    fn recover(&self, peer: &dyn PeerFiles, dry_run: bool) -> Result<(), TransferError>;

    /// Download a peer's live snapshot to a fresh local temp path and report
    /// whether the peer had history.
    ///
    /// Downloads the peer's live `.kitchensync/snapshot.db` to a fresh
    /// `{tmp}/{uuid}/snapshot.db` under `tmp_dir`, giving each download its own
    /// `uuid` directory, and leaves the peer copy untouched until writeback
    /// (016.4, 016.5). The returned [`Downloaded::local_path`] is that path.
    ///
    /// When the peer has no snapshot (the transport reports not found), create
    /// a new empty snapshot database at the local temp path so the caller has a
    /// file to open, and report `had_history` false (016.6). Transfer creates
    /// only the empty file; it never creates a table or schema, which belong to
    /// the caller.
    ///
    /// `had_history` is meaningful only after SWAP recovery has been applied,
    /// so [`Transfer::recover`] must run for this peer first on a normal run
    /// (016.13). Under `dry_run`, recovery is skipped but the peer's live
    /// `snapshot.db` is still downloaded as-is (024.3); the local temp copy is
    /// always created because it is local-only state.
    ///
    /// Surfaces the transport error categories raised while downloading; it
    /// does not decide whether such a failure aborts the run.
    fn download(
        &self,
        peer: &dyn PeerFiles,
        tmp_dir: &Path,
        dry_run: bool,
    ) -> Result<Downloaded, TransferError>;

    /// Write a local snapshot database back to a peer through the SWAP-staged
    /// writeback.
    ///
    /// `local_db` is already a self-contained rollback-journal SQLite database
    /// with all of the run's changes committed and every connection, statement,
    /// and cursor closed; Transfer uploads it as-is and relies on the caller to
    /// have made it self-contained (016.7). Only `snapshot.db` is uploaded; its
    /// SQLite sidecar files are never sent (016.1, 016.2, 016.3).
    ///
    /// On a normal run the steps are, in order:
    ///
    /// 1. Write and close the new database at
    ///    `.kitchensync/SWAP/snapshot.db/new` (016.8).
    /// 2. Rename the live `.kitchensync/snapshot.db` to
    ///    `.kitchensync/SWAP/snapshot.db/old` when the live file exists (016.9).
    /// 3. Rename `new` to `.kitchensync/snapshot.db` (016.10).
    /// 4. Delete `old` after the new snapshot is in place (016.11).
    ///
    /// No rename ever targets a name that already exists (016.12), and the
    /// peer's `.kitchensync/snapshot.db` is never modified in place: every
    /// change reaches the peer only through this staged sequence (016.5).
    ///
    /// On failure Transfer leaves recoverable SWAP state and does not roll a
    /// peer back beyond that: a failure before `old` exists keeps the live
    /// `snapshot.db` and leaves any `new` for the next run's recovery (016.20),
    /// and a failure after `old` exists leaves the SWAP state exactly as it is
    /// to be recovered on the next normal run (016.21). Transfer neither locks
    /// nor coordinates between runs, so when two runs overlap the peer's final
    /// `snapshot.db` is the one written by the run that uploads last (016.19).
    ///
    /// When `dry_run` is true, skip the SWAP-staged writeback entirely so no
    /// peer snapshot state changes (024.18).
    ///
    /// Surfaces the transport error categories raised while uploading,
    /// renaming, or deleting; it does not decide whether such a failure aborts
    /// the run.
    fn upload(
        &self,
        peer: &dyn PeerFiles,
        local_db: &Path,
        dry_run: bool,
    ) -> Result<(), TransferError>;
}
