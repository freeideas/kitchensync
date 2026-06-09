//! Public interface for the `DecisionRules` subproject.
//!
//! DecisionRules is the pure per-path resolver for a KitchenSync run. Given what
//! every peer holds at one path -- each peer's live state, its snapshot row, and
//! its role -- it classifies each peer and returns the single agreed outcome for
//! that path: which type wins, which file version wins, and what each peer must
//! do to conform.
//!
//! DecisionRules is a pure decision function. It performs no input or output: it
//! reads no filesystem, opens no connection, touches no snapshot database, and
//! writes no log line. It receives already-gathered per-peer facts for one path
//! and returns a decision describing the outcome and the per-peer actions. The
//! SyncEngine facade gathers those facts during the walk and carries out the
//! actions this child returns (creating directories, enqueuing copies, invoking
//! displacement); the timing, threading of the dry-run flag, and execution of
//! those actions are the facade's job, not this child's.
//!
//! The decision covers exactly one path. It considers only that path's per-peer
//! entries and rows; it never walks, recurses, or looks at child paths. A single
//! 5-second tolerance is the only comparison constant used for every mod_time
//! and deletion-estimate comparison.

/// The role a peer was designated with for the run.
///
/// At most one peer is [`PeerRole::Canon`]. The canon peer is identified solely
/// by carrying this role; there is no separate canon parameter.
pub enum PeerRole {
    /// The canon (`+`) peer. When present its state wins unconditionally over
    /// differing peers (007.1).
    Canon,
    /// An ordinary contributing peer. Its live entries enter the set used to pick
    /// a winner.
    Contributing,
    /// A subordinate (`-`) peer. Its entries never enter the set used to pick a
    /// winner, so the contributing outcome is identical to that peer being
    /// absent; it is conformed to the decision afterward (007.2).
    Subordinate,
}

/// What a peer currently holds at the path, as observed live by the walk.
pub enum LiveEntry {
    /// A regular file, with the size and modification time observed on the peer.
    /// `mod_time` is in the run's timestamp format (`YYYY-MM-DD_HH-mm-ss_ffffffZ`).
    File { byte_size: i64, mod_time: String },
    /// A directory. Directories are decided by existence; a directory's mod_time
    /// is never consulted (012.2).
    Directory,
    /// Nothing at the path on this peer.
    Absent,
}

/// The fields of a peer's snapshot row that the decision consults.
///
/// `None` for the whole row means the peer has no snapshot row for this path.
pub struct PeerRow {
    /// The recorded size in bytes for a regular file, or `-1` for a directory.
    pub byte_size: i64,
    /// The recorded modification time, in the run's timestamp format.
    pub mod_time: String,
    /// The tombstone timestamp recording when the entry was observed deleted, or
    /// `None` for a live row. A present file whose row carries a non-`None`
    /// `deleted_time` is a resurrection (010.4).
    pub deleted_time: Option<String>,
    /// The timestamp at which traversal last confirmed the entry present, or
    /// `None`. Used as an absent-unconfirmed peer's deletion estimate (011.10,
    /// 011.11).
    pub last_seen: Option<String>,
}

/// Everything the decision knows about one peer at the path.
pub struct PeerInput {
    /// The peer's winning (canonical) URL, its stable identity for the run and
    /// the name echoed back in [`PeerOutcome::peer`] and [`Decision::winner`].
    pub peer: String,
    /// The peer's role for this run.
    pub role: PeerRole,
    /// What the peer holds live at the path.
    pub live: LiveEntry,
    /// The peer's snapshot row for the path, or `None` when it has none.
    pub row: Option<PeerRow>,
}

/// The agreed type at the path.
pub enum DecidedType {
    /// A regular file wins; [`Decision::winner`] names the peer holding it.
    File,
    /// A directory wins.
    Directory,
    /// Nothing remains at the path; every peer that still has it displaces it.
    Absent,
}

/// What one peer must end up holding once any displacement completes.
pub enum Conform {
    /// Copy in the winning file named by [`Decision::winner`]. Emitted only when
    /// the peer does not already match the winner -- a peer already matching
    /// (mod_time within 5 seconds and equal byte_size) gets [`Conform::Nothing`]
    /// (011.15).
    CopyWinner,
    /// Create the agreed directory on a peer that lacks it.
    CreateDirectory,
    /// Nothing to add; the peer already holds the agreed state (after any
    /// displacement).
    Nothing,
}

/// The action one peer needs at the path.
///
/// A displacement, when present, always precedes the conform step: a peer
/// holding the wrong type, or a file losing to a deletion, is first displaced to
/// BAK and then conformed (012.16, 012.17).
pub struct PeerOutcome {
    /// The peer this action is for, echoing the matching [`PeerInput::peer`].
    pub peer: String,
    /// When true, the entry currently at the path on this peer is displaced to
    /// `.kitchensync/BAK` before any conform step. Set for a peer holding the
    /// losing type in a file/directory conflict, a peer whose file or directory
    /// loses to a deletion or to a canon that lacks the path, and a subordinate
    /// peer whose path has the wrong type. The rename itself is the Displacement
    /// child's job, invoked by the facade; this child only names that it is
    /// needed.
    pub displace: bool,
    /// The state the peer must hold once any displacement completes.
    pub conform: Conform,
}

/// The resolved outcome for one path.
pub struct Decision {
    /// The agreed type at the path: file, directory, or absent.
    pub agreed_type: DecidedType,
    /// When `agreed_type` is [`DecidedType::File`], the peer holding the winning
    /// version; every peer whose [`PeerOutcome::conform`] is [`Conform::CopyWinner`]
    /// receives that peer's file. `None` for a directory or absent outcome.
    pub winner: Option<String>,
    /// One entry per input peer, in input order, naming what that peer must do.
    pub actions: Vec<PeerOutcome>,
}

/// The pure per-path resolver.
///
/// A single instance is created per dependent, so `Arc<dyn DecisionRules>` is the
/// shareable handle the SyncEngine facade holds. `Send + Sync` is required so the
/// handle can be shared across the concurrent work a run performs.
pub trait DecisionRules: Send + Sync {
    /// Resolve the agreed outcome for one path from every peer's facts.
    ///
    /// This is a pure function: it reads and writes nothing, and the result is
    /// deterministic for a given set of inputs. `peers` carries one [`PeerInput`]
    /// per peer at the path; the returned [`Decision::actions`] has one
    /// [`PeerOutcome`] per peer in the same order.
    ///
    /// Each peer is first classified internally, comparing its live state against
    /// its own snapshot row into exactly one category, with the 5-second
    /// tolerance governing every mod_time comparison (010.1 through 010.8):
    /// unchanged (file matching its row in both mod_time and byte_size), modified
    /// (file differing in byte_size or by more than 5 seconds in mod_time, or a
    /// resurrection of a tombstoned row), new (file with no row), deleted (absent
    /// with a tombstoned row, whose `deleted_time` is that peer's deletion
    /// estimate), absent-unconfirmed (absent with a live row), or no-opinion
    /// (absent with no row).
    ///
    /// Roles (007): a subordinate peer's entries never enter the set used to pick
    /// a winner, so the contributing outcome is identical to that peer being
    /// absent; the subordinate peer is conformed to the contributing decision
    /// afterward, including a displacement when its existing type is wrong. When a
    /// canon peer is present its state wins unconditionally: a file canon has is
    /// copied to every other peer including subordinates (011.1), a file canon
    /// lacks is removed from every other peer (011.2), and on a file/directory
    /// conflict the canon type wins while the conflicting type is displaced to BAK
    /// (012.8 through 012.12).
    ///
    /// File decision without a canon peer (011): all contributing peers unchanged
    /// and matching produces no copy among them but copies the file to any active
    /// peer that lacks it including subordinates; otherwise the version with the
    /// newest mod_time wins and is propagated to every peer that does not already
    /// match it, where a peer within 5 seconds of the maximum mod_time is tied
    /// with it and equal mod_time with differing byte_size lets the larger file
    /// win. A deletion wins and removes the file only when its most-recent
    /// estimate exceeds the existing file's mod_time by more than 5 seconds;
    /// within 5 seconds of, or later than, the estimate the file is kept and
    /// copied to peers that lack it. An absent-unconfirmed peer's `last_seen`
    /// counts as a deletion estimate only when it exceeds the maximum mod_time
    /// among peers that have the file by more than 5 seconds; otherwise the file
    /// is re-copied to that peer and it casts no deletion vote. A peer with no
    /// snapshot row casts no vote on which version wins but still receives the
    /// decided winner (011.13, 011.14). No copy is enqueued to a peer that already
    /// matches the winner (011.15).
    ///
    /// Directory decision (012.1 through 012.7), decided by existence and never by
    /// mod_time: if any contributing peer has the directory live it is created on
    /// every active peer that lacks it; if none has it live but at least one
    /// contributing peer has a row for it and every contributing peer that has a
    /// row is now absent, it is displaced to BAK on every peer that still has it;
    /// if none has it live and none has a row, it is displaced from subordinate
    /// peers that still have it. A contributing peer with no row neither votes nor
    /// blocks a displacement.
    ///
    /// File/directory type conflict (012.8 through 012.17): with no canon the file
    /// wins -- each contributing peer's conflicting directory is displaced to BAK,
    /// then the winning file is selected among the contributing file entries by
    /// the normal file rules and synced to all active peers; a subordinate peer's
    /// file never causes the file to win over a contributing peer's directory.
    /// After the contributing type decision, a subordinate peer whose path has the
    /// wrong type is displaced to BAK and then conformed to the decided type.
    ///
    /// Invariants: the result is deterministic for a given set of inputs;
    /// subordinate peers never affect which contributing version or type wins; a
    /// peer with no snapshot row never votes but always receives the decided
    /// winner; and ties keep data rather than deleting it.
    fn decide(&self, peers: &[PeerInput]) -> Decision;
}
