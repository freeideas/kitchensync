//! Public specification for the StagingCleanup subproject.
//!
//! StagingCleanup is the cleanup worker the CopyQueue facade delegates to when a
//! normal run finishes processing a directory level. For one directory on one
//! peer it inspects the `.kitchensync/BAK/` and `.kitchensync/TMP/` staging
//! areas and deletes the entries that have aged past their retention limits,
//! judging each entry's age only from the `<timestamp>` component of its own
//! directory name. Recent entries are left in place, and `.kitchensync/SWAP/`
//! entries are never removed on the basis of age.
//!
//! StagingCleanup does not decide when cleanup runs, does not traverse the tree,
//! does not list a directory's live entries for sync decisions, and never
//! displaces or archives anything into BAK. It only deletes already-aged BAK and
//! TMP entries, and it reaches peer state solely through the per-peer filesystem
//! handle it is given -- it never opens a connection itself and never branches on
//! the peer's scheme, so the same code path serves local and SFTP peers.

use std::time::SystemTime;

/// The per-peer filesystem handle StagingCleanup is handed by the CopyQueue
/// facade for one cleanup call.
///
/// The handle is already bound to a single peer, so no operation names a peer or
/// a scheme: the same handle drives a local and an SFTP peer identically. All
/// paths are relative to that peer's root and are slash-separated.
///
/// Both operations are best-effort and infallible at this boundary. A listing
/// that the underlying transport cannot complete yields an empty list, and a
/// removal the transport cannot complete is silently no special treatment --
/// any diagnostic reporting belongs to the facade, not to StagingCleanup.
pub trait PeerFs: Send + Sync {
    /// Return the names of the immediate children of the directory at `path`,
    /// each with no path prefix.
    ///
    /// Returns an empty list when the directory does not exist or cannot be
    /// listed, so a peer with no staging area simply yields nothing to purge.
    fn list(&self, path: &str) -> Vec<String>;

    /// Remove the entry at `path` together with everything beneath it.
    ///
    /// Used to purge one aged `<timestamp>/` staging entry and its contents.
    /// Removing an entry that is already gone is a no-op.
    fn remove(&self, path: &str);
}

/// The aged-staging cleanup worker for one directory level on one peer. A single
/// shared instance serves the whole run, so `Arc<dyn StagingCleanup>` is the
/// handle the CopyQueue facade holds.
pub trait StagingCleanup: Send + Sync {
    /// Purge aged `BAK/` and `TMP/` entries under one directory's
    /// `.kitchensync/` area on one peer.
    ///
    /// `fs` is the per-peer filesystem handle to use for every listing and
    /// removal; the `.kitchensync/` area to clean is the one directly under
    /// `dir_path`. The `.kitchensync/` area is inspected directly even though the
    /// built-in exclude removes it from synced listings.
    ///
    /// Each `.kitchensync/BAK/<timestamp>/` entry whose `<timestamp>` is older
    /// than the BAK retention limit is removed, and each
    /// `.kitchensync/TMP/<timestamp>/` entry whose `<timestamp>` is older than
    /// the TMP retention limit is removed. An entry whose `<timestamp>` is not
    /// older than its limit is left in place. `.kitchensync/SWAP/` entries are
    /// never removed on the basis of age.
    ///
    /// An entry's age is judged solely from the `<timestamp>` component of its
    /// directory name, never from any filesystem modification time, and is
    /// measured against `now`.
    ///
    /// `bak_keep_days` and `tmp_keep_days` are the retention limits in days.
    /// `None` selects the default: 90 days for BAK and 2 days for TMP.
    ///
    /// When `dry_run` is true, cleanup is skipped entirely and no peer state is
    /// mutated.
    ///
    /// Cleanup is best-effort: a removal the filesystem handle cannot complete
    /// receives no special treatment, and the call always returns without
    /// surfacing a cleanup-specific error.
    fn cleanup(
        &self,
        fs: &dyn PeerFs,
        dir_path: &str,
        bak_keep_days: Option<u64>,
        tmp_keep_days: Option<u64>,
        now: SystemTime,
        dry_run: bool,
    );
}
