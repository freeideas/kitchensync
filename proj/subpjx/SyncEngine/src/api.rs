//! Public interface for the `SyncEngine` subproject.
//!
//! SyncEngine is the decision driver of a KitchenSync run. It performs the
//! single recursive, pre-order walk over the peer trees, decides the agreed
//! state at every path, and carries out the directory creates and inline BAK
//! displacements the decisions call for while enqueuing every file copy into
//! the copy queue.
//!
//! SyncEngine never moves file bytes and never talks to a network. It reads
//! each peer's live listing and metadata through the transport service, reads
//! and updates snapshot rows through the snapshot service, enqueues decided
//! copies into the copy queue, and reports through the output service. Those
//! four services are injected when the engine is constructed; the run
//! controller calls [`SyncEngine::run`] once per run to perform the whole
//! traversal-and-decision phase.

/// The role a peer was designated with for the run.
///
/// The role reflects the command-line designation only. SyncEngine may further
/// treat a peer as subordinate at run time: any peer with no
/// `.kitchensync/snapshot.db` is handled as subordinate unless it is the canon
/// peer, so a brand-new peer receives the group's state without influencing it
/// (007.7, 007.8, 007.9).
pub enum PeerRole {
    /// The canon (`+`) peer. At most one peer carries this role. When canon and
    /// another peer differ, the canon version wins unconditionally and is
    /// propagated to the whole group (007.1, 011.1, 011.2).
    Canon,
    /// A contributing peer carrying no marker. Its live entries enter the state
    /// set used to pick a winner.
    Contributing,
    /// A subordinate (`-`) peer. Its live entries never enter the state set used
    /// to pick a winner, so the group outcome is identical to the peer being
    /// absent; it is conformed to the contributing decision afterward (007.2
    /// through 007.6).
    Subordinate,
}

/// One connected peer participating in the run.
///
/// A peer is named throughout by its winning (canonical) URL, the same identity
/// the snapshot and copy-queue services use. The walk is rooted at each peer's
/// own sync prefix.
pub struct SyncPeer {
    /// The peer's winning (canonical) URL, used as its stable identity in every
    /// transport, snapshot, and copy-queue call.
    pub url: String,
    /// The peer's designated role for this run.
    pub role: PeerRole,
    /// The peer's sync prefix: the relative path within the peer at which the
    /// combined-tree walk is rooted.
    pub prefix: String,
}

/// The per-run inputs to a single traversal-and-decision phase.
///
/// SyncEngine owns no command-line parsing; it receives these already-validated
/// values and resolved excludes from its caller.
pub struct RunRequest {
    /// The connected, reachable peers with their roles and sync prefixes. An
    /// unreachable peer is excluded by the caller and never appears here.
    pub peers: Vec<SyncPeer>,
    /// The resolved command-line `-x` excludes, each a relative path. These are
    /// applied in addition to the built-in excludes; an `-x` entry can add an
    /// exclusion but cannot include or override a built-in one (009.5, 009.6).
    pub excludes: Vec<String>,
    /// The maximum number of listing attempts allowed for one peer directory
    /// before that subtree is skipped on that peer (the `--retries-list`
    /// value).
    pub list_retries: u32,
    /// When true, every peer-mutating step is suppressed: directory creates,
    /// displacements, and enqueued copies are threaded with the flag and read
    /// and decide normally without mutating any peer.
    pub dry_run: bool,
}

/// The traversal-and-decision engine. A single instance is created per
/// dependent, so `Arc<dyn SyncEngine>` is the shareable handle the run
/// controller holds.
///
/// `Send + Sync` is required so the handle can be shared across the concurrent
/// work a run performs (per-directory listings are fanned out in parallel and
/// copies run while later directories are still scanned).
pub trait SyncEngine: Send + Sync {
    /// Perform the whole combined-tree traversal and decision phase for the run,
    /// returning when every entry has been decided and every file copy has been
    /// enqueued into the copy queue.
    ///
    /// One recursive, pre-order walk is driven over the peer trees rooted at
    /// each peer's sync prefix. At each directory level every reachable peer's
    /// directory is listed in parallel through the transport, and the union of
    /// live entry names is built: contributing (canon and non-subordinate)
    /// peers drive the union; subordinate peers' names are included only so
    /// non-conforming entries can be cleaned up; the snapshot never contributes
    /// a name no peer still has live (008.3, 008.4, 008.5).
    ///
    /// Ordering and structure of the walk:
    /// - A directory's entries are processed in case-insensitive lexicographic
    ///   order, ties broken by the original case-sensitive name, and every entry
    ///   in a directory is finished before any subdirectory of it is entered
    ///   (008.1, 008.2).
    /// - Recursion descends into a kept directory only on the peers that keep
    ///   it. A directory chosen for displacement on a peer is moved as a single
    ///   subtree rename and is not recursed into on that peer (008.7, 008.8,
    ///   008.9).
    /// - Entry names are preserved exactly as the filesystem reports them; case
    ///   and characters are never changed, so syncing between case-sensitive and
    ///   case-insensitive peers may collapse or duplicate case-only variants,
    ///   which stay recoverable from BAK (008.16).
    ///
    /// Excludes: the built-in excludes (`.kitchensync/` directories, `.git/`
    /// directories, symbolic links, and special files) and each `-x` exclude are
    /// removed from the union before any decision. An excluded path is treated
    /// as nonexistent for the run -- not scanned, copied, deleted, or displaced,
    /// its snapshot row neither read nor updated, and any existing excluded entry
    /// left untouched on every peer; an excluded directory removes its whole
    /// subtree from the walk (009.1 through 009.9).
    ///
    /// Decisions and the actions they produce:
    /// - With a canon peer present, canon wins unconditionally: a file canon has
    ///   is copied to every other peer including subordinates, and a file canon
    ///   lacks is deleted from every other peer; the canon type wins on a
    ///   file/directory conflict (011.1, 011.2, 012.8 through 012.10).
    /// - Without a canon peer, contributing peers' classifications resolve to one
    ///   outcome per path by the file rules (newest mod_time wins; a deletion
    ///   wins over an existing file only when its estimate is later than the
    ///   file's mod_time by more than 5 seconds; equal mod_time with differing
    ///   byte_size lets the larger file win; ties keep data), directories are
    ///   decided by existence rather than mod_time, and a file/directory conflict
    ///   resolves in favor of the file (011.3 through 011.12, 012.1 through
    ///   012.7, 012.11 through 012.17).
    /// - Each per-peer file entry is classified against its own snapshot row
    ///   into exactly one of unchanged, modified, new, deleted, absent-unconfirmed,
    ///   or no-opinion, applying a 5-second tolerance to mod_time and requiring
    ///   both mod_time and byte_size to match for unchanged (010.1 through
    ///   010.8).
    ///
    /// Idempotency: no copy is enqueued to a peer that already matches the
    /// decided winner (mod_time within the 5-second tolerance and equal
    /// byte_size); among matching peers an all-unchanged path performs no copy
    /// at all (011.13 through 011.17).
    ///
    /// Inline displacement: a displacement renames `<parent>/<basename>` to
    /// `<parent>/.kitchensync/BAK/<timestamp>/<basename>` after creating that
    /// BAK directory and any missing parents through the transport, with the BAK
    /// directory at the displaced entry's own parent level, never aggregated at
    /// the sync root, and a directory displaced as a single subtree rename. This
    /// runs inline during the walk, not through the copy queue, so a displacement
    /// needed before a copy into the same path (a type conflict) finishes before
    /// that copy is enqueued and the copy succeeds within the same run (008.6,
    /// 021.1 through 021.4).
    ///
    /// Error handling at the boundary: a displacement rename failure logs an
    /// error-level diagnostic through the output service and is skipped, leaving
    /// the entry in place; the walk continues (021.5, 021.6). A peer whose
    /// listing failed all allowed tries has nothing created, deleted, displaced,
    /// or copied under that subtree and none of its snapshot rows for that
    /// subtree modified; and when the canon peer's listing fails for a subtree,
    /// no peer is modified under it (008.10 through 008.15).
    ///
    /// Dry-run: when `request.dry_run` is set the traversal reads and decides
    /// exactly as a normal run, but the flag is threaded into every mutating
    /// operation (directory create, displacement) and into each enqueued copy,
    /// suppressing the mutation while leaving the decisions unchanged.
    fn run(&self, request: RunRequest);
}
