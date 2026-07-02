#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeConflictRequest {
    pub relative_path: String,
    pub active_peers: Vec<TypeConflictPeerInput>,
    pub canon_peer_identity: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeConflictPeerInput {
    pub peer_identity: String,
    pub role: TypeConflictPeerRole,
    pub is_active_target: bool,
    pub live_entry: TypeConflictLiveEntry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeConflictPeerRole {
    Contributing,
    Subordinate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeConflictLiveEntry {
    File {
        source_relative_path: String,
    },
    Directory {
        source_relative_path: String,
    },
    Missing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeConflictResult {
    Decision(TypeConflictDecision),
    InvalidInput(TypeConflictInvalidInput),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeConflictDecision {
    pub relative_path: String,
    pub group_outcome: TypeConflictGroupOutcome,
    pub peer_decisions: Vec<TypeConflictPeerDecision>,
    pub displacement_intents: Vec<TypeConflictDisplacementIntent>,
    pub replacement_intents: Vec<TypeConflictReplacementIntent>,
    pub directory_recursion: Option<TypeConflictDirectoryRecursion>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeConflictGroupOutcome {
    File {
        source: TypeConflictSyncSource,
    },
    Directory {
        source: TypeConflictSyncSource,
    },
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeConflictSyncSource {
    pub peer_identity: String,
    pub source_relative_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeConflictPeerDecision {
    pub peer_identity: String,
    pub role: TypeConflictPeerRole,
    pub live_entry: TypeConflictLiveEntry,
    pub disposition: TypeConflictPeerDisposition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeConflictPeerDisposition {
    KeepsWinningFile,
    ReceivesWinningFile,
    DisplacesDirectoryThenReceivesFile,
    KeepsWinningDirectory,
    ReceivesWinningDirectory,
    DisplacesFileThenReceivesDirectory,
    DisplacesFileForAbsence,
    DisplacesDirectoryForAbsence,
    AlreadyAbsent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeConflictDisplacementIntent {
    pub peer_identity: String,
    pub relative_path: String,
    pub kind: TypeConflictDisplacementKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeConflictDisplacementKind {
    File,
    DirectoryWholeSubtree,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeConflictReplacementIntent {
    SyncFile {
        source_peer_identity: String,
        source_relative_path: String,
        destination_peer_identity: String,
        destination_relative_path: String,
    },
    SyncDirectory {
        source_peer_identity: String,
        source_relative_path: String,
        destination_peer_identity: String,
        destination_relative_path: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeConflictDirectoryRecursion {
    pub relative_path: String,
    pub peer_identities: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeConflictInvalidInput {
    pub relative_path: String,
    pub reason: TypeConflictInvalidReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeConflictInvalidReason {
    EmptyPeerSet,
    DuplicatePeerIdentity(String),
    CanonPeerNotActive(String),
    NotOneMixedFileDirectoryPath,
    NoContributingWinningType,
    MissingCanonSource(String),
    MissingEligibleContributingSource,
}

pub trait TypeConflictOutcomes: Send + Sync {
    /// Resolves one visible file-versus-directory conflict path from supplied
    /// peer-role, canon, live-type, and source-name facts.
    ///
    /// The operation is a pure planner decision for one path. It must not list
    /// directories, apply excludes, normalize paths or peer identities, inspect
    /// metadata beyond the supplied live entry type, compare file modification
    /// times, collect survival evidence, recurse into children, execute file
    /// copies, create directories, move entries, write snapshot rows, format
    /// output, or fetch additional state. Repeating the same request must
    /// return the same result.
    ///
    /// With a canon peer, the canon peer's live type wins unconditionally and
    /// non-canon peers cannot change the selected group outcome. A live canon
    /// file selects a file outcome: active targets with live directories
    /// receive directory displacement intents, and active targets that do not
    /// already have a file receive file sync intents from the canon source. A
    /// live canon directory selects a directory outcome: active targets with
    /// live files receive file displacement intents, and active targets that
    /// do not already have a directory receive directory sync intents from the
    /// canon source. A missing canon path selects absence: every active target
    /// with a live file or directory receives a displacement intent, active
    /// targets already missing the path receive no mutation intent, and the
    /// result is not eligible for child recursion.
    ///
    /// Without a canon peer, only contributing peers choose the winning type.
    /// Any contributing live file selects the file outcome, even when one or
    /// more contributing peers also have a live directory. The winning file
    /// source must be selected only from contributing peers that have a live
    /// file at the path. Subordinate files never make a file beat a
    /// contributing directory, and subordinate files are never eligible as the
    /// winning source. If no contributing peer has a live file and at least
    /// one contributing peer has a live directory, the directory outcome is
    /// selected, with the source selected only from contributing live
    /// directories.
    ///
    /// After the winning type is selected, every active target with the losing
    /// live type receives a displacement intent for that entry. A target that
    /// lacks the winning type then receives the replacement intent required
    /// for the selected outcome: file sync for a file winner, or directory
    /// sync for a directory winner. Subordinate targets are displaced and
    /// replaced in the same way as contributing targets after the canon or
    /// contributing outcome is selected.
    ///
    /// Returned displacement intents must be interpreted before returned
    /// replacement intents for the same peer and path. Directory displacement
    /// intents are whole-subtree displacements. A directory outcome is
    /// eligible for child recursion only with active targets that keep or
    /// receive the winning directory. File and absent outcomes are not
    /// eligible for child recursion. Every returned decision fact and intent
    /// is about the single input path and has no execution side effect.
    ///
    /// Every returned source fact and replacement intent must preserve the
    /// exact `source_relative_path` supplied for the selected source
    /// filesystem. The method must not normalize, lowercase, or otherwise
    /// rewrite the selected source name.
    ///
    /// Invalid input returns `TypeConflictResult::InvalidInput` instead of
    /// inventing a source, choosing a winner, or returning copy,
    /// displacement, replacement, or recursion intents. Invalid input includes
    /// an empty peer set, duplicate peer identities, a canon identity that is
    /// not active for this path, facts that do not describe one mixed
    /// file-versus-directory path, missing source names for the selected canon
    /// or contributing source, or a non-canon request with no contributing
    /// live file or directory from which a winning type can be selected.
    fn decide_type_conflict(&self, request: TypeConflictRequest) -> TypeConflictResult;
}
