//! Public interface for the `Displacement` subproject.
//!
//! Displacement is the inline mover for a KitchenSync run. When a sync decision
//! says an existing entry must be set aside rather than overwritten -- a file
//! losing to a deletion, a directory no peer keeps, or a type conflict that must
//! be cleared before a copy -- Displacement performs the one rename that moves
//! that entry into a recoverable BAK location beside it, so it stays recoverable
//! and the rest of the walk can continue.
//!
//! Displacement decides nothing. It does not classify entries, resolve winners,
//! or choose which entries to displace; DecisionRules and the SyncEngine facade
//! make that choice and hand Displacement an entry already chosen to be moved
//! aside. It does not move file bytes between peers; the copy queue owns byte
//! copying. It reaches the filesystem only through the transport service and
//! never branches on URL scheme, opens connections, or moves bytes directly.
//! It is created per dependent and holds no persistent global state.

/// The outcome of a single displace call: whether the entry was moved aside or
/// left where it was.
pub enum DisplaceOutcome {
    /// The entry was renamed into its BAK directory and now lives only at
    /// `<parent>/.kitchensync/BAK/<timestamp>/<basename>`. Under dry-run this is
    /// also returned -- nothing was moved, but the call reports as if the move
    /// were the decided action so the facade can report what would have happened
    /// (024.16).
    Displaced,
    /// The rename failed: the entry was left untouched at its original path,
    /// neither deleted nor partially moved, and an error diagnostic was emitted.
    /// The walk should continue without treating the entry as displaced (021.6).
    LeftInPlace,
}

/// The inline BAK mover. A single instance is created per dependent, so
/// `Arc<dyn Displacement>` is the shareable handle the facade holds.
///
/// `Send + Sync` is required so the handle can be shared across the concurrent
/// work a run performs.
pub trait Displacement: Send + Sync {
    /// Move the entry `<parent>/<basename>` on `peer` aside into a recoverable
    /// BAK location, performing exactly one displacement and deciding nothing.
    ///
    /// The caller has already decided this entry must be set aside; `displace`
    /// only carries out the move on the named peer through the supplied
    /// `transport`, and reports outcomes through the supplied `output`. The
    /// `peer` handle identifies the peer whose tree the entry lives in; `parent`
    /// is that peer-relative parent directory path and `basename` the entry's
    /// own name with no path prefix. `timestamp` is the already-formatted run
    /// timestamp -- Displacement never generates it and only places it in the
    /// BAK path.
    ///
    /// On a normal run the steps are ordered: first create the BAK directory
    /// `<parent>/.kitchensync/BAK/<timestamp>/`, including any missing parent
    /// directories (`.kitchensync/` and `BAK/`), through the transport (021.1);
    /// then rename `<parent>/<basename>` to
    /// `<parent>/.kitchensync/BAK/<timestamp>/<basename>`, preserving the
    /// basename (021.2). The BAK directory is always co-located under the
    /// displaced entry's own parent; displaced entries are never aggregated into
    /// a single BAK directory at the sync root (021.4).
    ///
    /// A directory entry is moved as one subtree rename, so its entire subtree
    /// is preserved and travels with it; the entry is never copied and deleted
    /// piece by piece (021.3). A successful call leaves the entry under
    /// `<parent>/.kitchensync/BAK/<timestamp>/<basename>` and nowhere else, and
    /// returns [`DisplaceOutcome::Displaced`].
    ///
    /// When the rename into BAK fails, the entry is left in place at its original
    /// path -- not deleted, not partially moved -- an error-level diagnostic is
    /// logged through `output` (021.5), and the call returns
    /// [`DisplaceOutcome::LeftInPlace`] so the walk can continue without treating
    /// the entry as displaced (021.6). Apart from that single error diagnostic on
    /// rename failure, Displacement emits no output; it does not own the success
    /// progress line, which the facade emits.
    ///
    /// When `dry_run` is true, no rename is performed and no BAK directory is
    /// created, so no entry on any peer is moved aside or removed -- a deletion is
    /// only ever carried out as a displacement, and the suppressed displacement
    /// leaves the entry untouched (024.15, 024.16). The call still returns
    /// [`DisplaceOutcome::Displaced`], as if the move were the decided action, so
    /// the facade can report what would have happened.
    fn displace(
        &self,
        transport: &dyn transport::Transport,
        output: &dyn output::Output,
        peer: &transport::PeerHandle,
        parent: &str,
        basename: &str,
        timestamp: &str,
        dry_run: bool,
    ) -> DisplaceOutcome;
}
