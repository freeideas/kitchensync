//! Public specification for the SwapTransfer subproject.
//!
//! SwapTransfer performs one file copy at a time and makes that copy
//! recoverable. It never writes a destination in place: every replacement is
//! staged through the SWAP directory under the target's `.kitchensync/`, so an
//! interruption at any step leaves a state a later recovery pass can finish or
//! roll back. It owns two things: the ordered SWAP replacement sequence that
//! carries one source file into one destination path, and the five-state SWAP
//! recovery machine that reconciles a leftover SWAP directory back to a single
//! consistent outcome.
//!
//! SwapTransfer does not decide which files to copy or which modification time
//! wins, and it does not own the copy-slot limit or the per-copy try count. The
//! caller supplies the source and destination, the winning mod_time, and the
//! per-peer filesystem primitives; SwapTransfer carries out one try safely and
//! reports its outcome so the sibling scheduler can requeue or fail the copy.
//!
//! Every filesystem action is a primitive supplied for the relevant peer through
//! the [`Fs`] port. SwapTransfer orchestrates those calls and never branches on
//! the peer scheme itself.

use std::time::SystemTime;

/// A failure reported by an [`Fs`] primitive. SwapTransfer never branches on a
/// failure category: any error from a step is simply that step failing, which
/// SwapTransfer maps to a [`TransferOutcome`] or to a failed recovery. Diagnostic
/// text is the output component's concern, so this carries no payload.
pub struct FsError;

/// An opaque handle to an open streaming read on a peer, produced by
/// [`Fs::open_read`] and consumed by [`Fs::read`] / [`Fs::close_read`].
pub struct ReadHandle(pub u64);

/// An opaque handle to an open streaming write on a peer, produced by
/// [`Fs::open_write`] and consumed by [`Fs::write`] / [`Fs::close_write`].
pub struct WriteHandle(pub u64);

/// The per-peer filesystem port SwapTransfer requires. The caller binds one of
/// these to each peer and hands it in; SwapTransfer calls these primitives and
/// never inspects the peer's scheme. All paths are relative to the peer's root
/// and slash-separated. `Send + Sync` is required because transfers run
/// concurrently behind a shared `Arc<dyn SwapTransfer>`.
pub trait Fs: Send + Sync {
    /// Open a file for streaming read.
    fn open_read(&self, path: &str) -> Result<ReadHandle, FsError>;

    /// Read the next chunk of at most `max_bytes` bytes, or `None` at end of
    /// file. The chunk bound is fixed by the caller and is independent of the
    /// size of the file being copied.
    fn read(&self, handle: &ReadHandle, max_bytes: usize) -> Result<Option<Vec<u8>>, FsError>;

    /// Close an open streaming read, releasing its resources.
    fn close_read(&self, handle: ReadHandle) -> Result<(), FsError>;

    /// Open a file for streaming write, creating the file and any missing
    /// parent directories.
    fn open_write(&self, path: &str) -> Result<WriteHandle, FsError>;

    /// Append the given bytes to an open streaming write.
    fn write(&self, handle: &WriteHandle, bytes: &[u8]) -> Result<(), FsError>;

    /// Close an open streaming write, flushing and releasing its resources.
    fn close_write(&self, handle: WriteHandle) -> Result<(), FsError>;

    /// Create the directory and any missing parent directories.
    fn create_dir(&self, path: &str) -> Result<(), FsError>;

    /// Move `src` to `dst`. The destination must not already exist; SwapTransfer
    /// never relies on rename-over-existing, which is why a replaced target is
    /// first moved aside to SWAP `old`.
    fn rename(&self, src: &str, dst: &str) -> Result<(), FsError>;

    /// Remove a regular file.
    fn delete_file(&self, path: &str) -> Result<(), FsError>;

    /// Remove an empty directory.
    fn delete_dir(&self, path: &str) -> Result<(), FsError>;

    /// Report whether a path exists. This is the presence test that drives both
    /// the "file already exists at the target" check during a transfer and the
    /// five-state recovery machine, which keys off the presence of `old`, `new`,
    /// and the target.
    fn exists(&self, path: &str) -> Result<bool, FsError>;

    /// Set the modification time of a file or directory.
    fn set_mod_time(&self, path: &str, time: SystemTime) -> Result<(), FsError>;

    /// Host-native local copy of `src_path` on `src` into `dst_path` on `self`,
    /// usable only when both peers are the same local host. Returns an error when
    /// a native copy is not possible (for example a remote peer), so SwapTransfer
    /// falls back to the streaming read/write path. Either way the copy targets
    /// the SWAP `new` path, never the destination in place (020.15).
    fn native_copy(&self, src: &dyn Fs, src_path: &str, dst_path: &str) -> Result<(), FsError>;
}

/// The outcome of one transfer try, reported so the scheduler can requeue or
/// fail the copy.
pub enum TransferOutcome {
    /// The new content is in place at the target with its mod_time set, the SWAP
    /// directories are removed, and the copy is complete. This includes the case
    /// where the replacement is in place but archiving SWAP `old` to BAK failed:
    /// `old` is left for a later recovery pass (019.13), and the copy itself
    /// still succeeded.
    Done,
    /// The try failed and the copy may be retried by its try count. The staged
    /// state has been cleaned or left for recovery as the error obligations
    /// require: a failure before SWAP `old` existed deletes the staged SWAP
    /// `new` (019.9); a failure after SWAP `old` exists leaves the SWAP state in
    /// place for a later recovery pass (019.12).
    Failed,
    /// Moving the existing destination into SWAP `old` failed. The original
    /// destination was left untouched (019.10) and this copy is skipped for the
    /// run (019.11): it is not requeued regardless of any remaining tries.
    Skipped,
}

/// The single-file recoverable copy service. A single shared instance serves the
/// whole run, so `Arc<dyn SwapTransfer>` is the handle the scheduler holds.
/// `Send + Sync` is required so that handle is shareable across the concurrent
/// transfers the scheduler drives.
pub trait SwapTransfer: Send + Sync {
    /// Perform one transfer try: copy the source file into the destination path
    /// through SWAP staging, never writing the destination in place.
    ///
    /// `src`/`dst` are the bound filesystem ports for the source and destination
    /// peers; `src_path`/`dst_path` are peer-relative. `mod_time` is the winning
    /// modification time, stamped on the destination verbatim and never re-read
    /// from the source (019.4). `tmp_dir` is a distinct peer-relative TMP staging
    /// directory under the destination's `.kitchensync/` for this transfer's use;
    /// it is distinct per transfer so concurrent transfers do not collide
    /// (021.7, 021.8). `bak_dest` is the peer-relative destination under the
    /// destination's `.kitchensync/BAK/<timestamp>/` to which SWAP `old` is
    /// archived if one is produced; SwapTransfer names BAK only as that archive
    /// destination, and the timestamped BAK layout belongs to the staging
    /// concern.
    ///
    /// The ordered, recoverable SWAP sequence, all under the destination peer's
    /// `<dst-parent>/.kitchensync/SWAP/<encoded-basename>/`, where
    /// `<encoded-basename>` is the target basename percent-encoded to a single
    /// path segment (019.7):
    ///
    /// 1. Recover or fail any existing SWAP directory for the target basename,
    ///    using the same reconciliation as [`SwapTransfer::recover`] (019.8).
    /// 2. Stream the source content into the SWAP `new` path before the target is
    ///    touched (019.1). The total buffer is independent of the size of the
    ///    file being copied (020.13) and the destination begins receiving bytes
    ///    before the whole source has been read (020.14). When both ends are
    ///    local the host's native copy may be used, but the copy still passes
    ///    through SWAP `new` rather than writing the destination in place
    ///    (020.15).
    /// 3. When a file already exists at the target, move it to the SWAP `old`
    ///    path before swapping in the new content (019.2).
    /// 4. Rename the SWAP `new` file onto the final target path (019.3).
    /// 5. Set the destination's modification time to `mod_time` (019.4).
    /// 6. When SWAP `old` exists after the new file is in place, archive it to
    ///    `bak_dest` (019.5).
    /// 7. Remove the now-empty SWAP directories (019.6).
    ///
    /// Error obligations:
    /// - Failure before SWAP `old` exists deletes the staged SWAP `new` (019.9)
    ///   and reports [`TransferOutcome::Failed`].
    /// - Failure to move the existing destination to SWAP `old` leaves the
    ///   original destination in place (019.10) and reports
    ///   [`TransferOutcome::Skipped`] (019.11).
    /// - Failure after SWAP `old` exists leaves the SWAP state in place for a
    ///   later recovery pass (019.12) and reports [`TransferOutcome::Failed`].
    /// - Failure to archive SWAP `old` to BAK after the replacement is in place
    ///   leaves SWAP `old` for a later recovery pass (019.13) and still reports
    ///   [`TransferOutcome::Done`].
    ///
    /// When `dry_run` is true the copy machinery is exercised making no change to
    /// any peer: the source file is still read (024.5), but no TMP, SWAP, or BAK
    /// directory is created (024.13), no destination file is written (024.14),
    /// and no modification time is set (024.17). The reported outcome is
    /// [`TransferOutcome::Done`].
    fn transfer(
        &self,
        src: &dyn Fs,
        src_path: &str,
        dst: &dyn Fs,
        dst_path: &str,
        mod_time: SystemTime,
        tmp_dir: &str,
        bak_dest: &str,
        dry_run: bool,
    ) -> TransferOutcome;

    /// Reconcile a single `.kitchensync/SWAP/<encoded-basename>` directory to one
    /// consistent outcome, keyed off the presence of `old`, `new`, and the
    /// target. This is both the "recover or fail" step that precedes a
    /// replacement (019.8) and the pass run before a directory's live entries are
    /// listed for sync decisions (019.14).
    ///
    /// `fs` is the bound port for the peer; `target_path` is the peer-relative
    /// real destination file path, from which the SWAP directory is located by
    /// percent-encoding the basename (019.7). `bak_dest` is the peer-relative
    /// destination under `.kitchensync/BAK/<timestamp>/` to which SWAP `old` is
    /// archived when a state requires it.
    ///
    /// The five states:
    /// - `old` present, target present: move `old` to BAK, remove the empty SWAP
    ///   directory (019.15).
    /// - `old` present, `new` present, target missing: rename `new` to the
    ///   target, move `old` to BAK, remove the empty SWAP directory (019.16).
    /// - `old` present, `new` missing, target missing: rename `old` back to the
    ///   target, remove the empty SWAP directory (019.17).
    /// - `old` missing, `new` present, target present: delete `new`, remove the
    ///   empty SWAP directory (019.18).
    /// - `old` missing, `new` present, target missing: rename `new` to the
    ///   target, remove the empty SWAP directory (019.19).
    ///
    /// Returns `true` when recovery succeeded. On `false`, the caller treats this
    /// peer's listing for the directory as failed and excludes the peer from that
    /// directory subtree (019.20). When `dry_run` is true this peer-mutating
    /// recovery is skipped and `true` is returned (019.21, 024.20).
    fn recover(&self, fs: &dyn Fs, target_path: &str, bak_dest: &str, dry_run: bool) -> bool;
}
