# classify API

The `classify` module exposes a sync-internal Rust contract for building the
normalized decision input for exactly one candidate path. It has no root-visible
or crate-public API: first-layer sibling modules such as `snapshot`,
`transport`, `operations`, and `runtime` must not import `sync::classify`
directly. Any contract that must cross the `sync` boundary belongs at `sync` or
at the root shared-contract layer instead.

All items below are intended to be visible only to the parent `sync` module and
its private decision, dispatch, and snapshot-flow helpers, for example with
`pub(super)` or narrower Rust visibility.

## Dependencies

`classify` may use these already-defined contracts by value or reference:

- `RelPath`
- `PeerId`
- `PeerSession`
- `EffectivePeerRole`
- `EntryMeta`
- `EntryKind`
- `Timestamp`
- `SnapshotRow`
- `SnapshotEntryKind`

The module must not expose or depend on `SnapshotStore`, SQLite row ids, SQLite
table names, path hashes, local database paths, `TransportHandle`,
`OperationExecutor`, `CopyScheduler`, diagnostic sinks, progress sinks, or
runtime output types.

## Entry Point

```rust
pub(super) fn classify_candidate(
    input: ClassificationInput,
) -> Result<ClassifiedCandidate, ClassificationError>;
```

`classify_candidate` consumes one fully gathered in-memory candidate input and
returns an owned deterministic classification result. It performs no I/O,
snapshot mutation, transport operation, logging, progress reporting, directory
recursion, copy scheduling, or winner selection.

The function must preserve active peer order in returned vectors. If maps are
used internally, public result records must still expose enough stable ordering
or explicit `PeerId` keys for downstream tie handling to be repeatable.

## Input Records

```rust
pub(super) struct ClassificationInput {
    pub path: RelPath,
    pub basename: String,
    pub peers: Vec<PeerCandidateInput>,
}

pub(super) struct PeerCandidateInput {
    pub session: PeerSession,
    pub live: Option<EntryMeta>,
    pub snapshot: SnapshotLookup,
}

pub(super) enum SnapshotLookup {
    NotLookedUp,
    Missing,
    Present(SnapshotRow),
}
```

`path` is the normalized candidate relative path owned by traversal.
`basename` is the preserved transport-supplied spelling for the candidate name
at the current directory level. `peers` contains one record for each active peer
at that directory level in stable run order.

`SnapshotLookup::NotLookedUp` represents caller policy that snapshot lookup was
not allowed or not applicable for this peer. It is valid only when downstream
classification state does not require snapshot facts. Lookup failures are not
represented here; callers must handle them before invoking `classify`.

## Output Records

```rust
pub(super) struct ClassifiedCandidate {
    pub path: RelPath,
    pub basename: String,
    pub canon: Option<CanonObservation>,
    pub contributors: Vec<ContributingObservation>,
    pub subordinates: Vec<SubordinateTarget>,
    pub summary: ClassificationSummary,
}

pub(super) struct ClassificationSummary {
    pub has_live_file: bool,
    pub has_live_directory: bool,
    pub has_deletion_vote: bool,
    pub has_unconfirmed_absence: bool,
}
```

`ClassifiedCandidate` is the only successful return value. It is decision input,
not a decision. It must not identify a winner, loser, copy source, delete target,
or final group outcome.

`canon` is present only when exactly one active canon peer contributes to the
candidate. It records that peer's already-classified observation without
comparing it to other peers.

`summary` contains derived predicates only. Downstream code may rely on these
booleans as shortcuts, but the typed peer observations remain the authoritative
state.

## Contributing Observations

```rust
pub(super) struct CanonObservation {
    pub peer_id: PeerId,
    pub state: ContributingState,
}

pub(super) struct ContributingObservation {
    pub peer_id: PeerId,
    pub state: ContributingState,
}

pub(super) enum ContributingState {
    LiveFile(LiveFileObservation),
    LiveDirectory(LiveDirectoryObservation),
    TombstoneDeletionVote(TombstoneDeletionVote),
    AbsentUnconfirmedFile(AbsentUnconfirmedFile),
    AbsentDirectoryHistory(AbsentDirectoryHistory),
    NoVote,
}
```

Only normal and canon peers produce `ContributingObservation` values.
Subordinate peers must never appear in `contributors` and must never influence
`summary` values other than target counts kept outside decision predicates.

```rust
pub(super) struct LiveFileObservation {
    pub meta: EntryMeta,
    pub snapshot: LiveFileSnapshotState,
}

pub(super) enum LiveFileSnapshotState {
    Unchanged {
        previous: SnapshotFileFacts,
    },
    Modified {
        previous: SnapshotKnownFacts,
    },
    Resurrected {
        tombstone: SnapshotTombstoneFacts,
    },
    New,
}

pub(super) struct LiveDirectoryObservation {
    pub meta: EntryMeta,
    pub previous: Option<SnapshotDirectoryFacts>,
}
```

A live file is `Unchanged` only when a non-tombstone file snapshot row exists
and both byte size and modification time match with sync's required five-second
tolerance. A live file with prior non-tombstone history that does not match is
`Modified`. A live file paired with tombstone history is `Resurrected`. A live
file with no snapshot row is `New`.

Directory modification time is preserved in `LiveDirectoryObservation::meta` for
later snapshot updates, but classify must not treat directory modification time
as decision-significant.

```rust
pub(super) struct TombstoneDeletionVote {
    pub deleted_time: Timestamp,
}

pub(super) struct AbsentUnconfirmedFile {
    pub previous: SnapshotFileFacts,
}

pub(super) struct AbsentDirectoryHistory {
    pub previous: SnapshotDirectoryFacts,
}
```

`TombstoneDeletionVote` exposes an existing tombstone as a deletion vote without
choosing deletion. `AbsentUnconfirmedFile` exposes prior file `last_seen` facts
without deciding whether the absence becomes a deletion vote. Decision code owns
the comparison between that `last_seen` value and the newest live file
modification time.

## Snapshot Fact Records

```rust
pub(super) struct SnapshotFileFacts {
    pub size: i64,
    pub modified_time: Timestamp,
    pub last_seen: Timestamp,
}

pub(super) struct SnapshotDirectoryFacts {
    pub modified_time: Option<Timestamp>,
    pub last_seen: Timestamp,
}

pub(super) struct SnapshotTombstoneFacts {
    pub previous_kind: Option<SnapshotEntryKind>,
    pub deleted_time: Timestamp,
    pub last_seen: Option<Timestamp>,
}

pub(super) enum SnapshotKnownFacts {
    File(SnapshotFileFacts),
    Directory(SnapshotDirectoryFacts),
    Tombstone(SnapshotTombstoneFacts),
}
```

These records contain only row facts needed by sync decision and later snapshot
flow. They must not expose durable storage implementation details.

## Subordinate Targets

```rust
pub(super) struct SubordinateTarget {
    pub peer_id: PeerId,
    pub live: Option<EntryMeta>,
    pub snapshot: Option<SubordinateSnapshotFacts>,
}

pub(super) enum SubordinateSnapshotFacts {
    File(SnapshotFileFacts),
    Directory(SnapshotDirectoryFacts),
    Tombstone(SnapshotTombstoneFacts),
}
```

Subordinate records are effect targets only. Their live entries and snapshot
history must not be converted into contributing observations, canon
observations, deletion votes, newest-file candidates, or conflict inputs.

## Errors

```rust
pub(super) enum ClassificationError {
    DuplicatePeer { peer_id: PeerId },
    UnknownOrInactivePeer { peer_id: PeerId },
    MultipleCanonPeers,
    InvalidLiveMetadata {
        peer_id: PeerId,
        reason: InvalidLiveMetadata,
    },
    InvalidSnapshotState {
        peer_id: PeerId,
        reason: InvalidSnapshotState,
    },
    MissingRequiredSnapshot {
        peer_id: PeerId,
    },
}

pub(super) enum InvalidLiveMetadata {
    FileWithoutSize,
    DirectoryWithFileSize,
    UnsupportedEntryKind,
}

pub(super) enum InvalidSnapshotState {
    KindFactsMismatch,
    TombstoneWithoutDeletedTime,
    UnsupportedEntryKind,
}
```

`ClassificationError` is an internal sync orchestration error for inconsistent
caller-supplied memory state. The module must return an error rather than panic
for ordinary malformed input such as duplicate peer records, multiple active
canon peers, invalid live metadata for the declared entry kind, snapshot facts
that do not match their snapshot kind, or a missing snapshot lookup required to
classify an absence.

Transport, filesystem, listing, and snapshot lookup failures are outside this
error surface and must be handled by callers before classification.

## Ownership Rules

`ClassificationInput` is consumed by value and `ClassifiedCandidate` owns its
returned path, basename, metadata, and snapshot fact records. Borrowed records
must not escape `classify_candidate`.

`EntryMeta` and snapshot-derived fact records must be cloned or moved into the
result as needed so downstream decision, dispatch, and snapshot-flow code can
operate without retaining references to traversal-owned buffers or snapshot
lookup cursors.

`classify` must not mutate input peer sessions, snapshot rows, transport state,
or any external store. It must not cache candidate results across calls.

## Non-API Behavior

The following behavior is intentionally not part of the `classify` API:

- choosing canon authority outcomes;
- resolving file-vs-directory conflicts;
- selecting newest files or tie-breaking winners;
- converting absent-unconfirmed file history into deletion;
- scheduling or executing copies;
- creating, deleting, displacing, or cleaning peer entries;
- mutating snapshot rows;
- listing directories or applying excludes;
- recursing into child directories;
- emitting diagnostics or progress events.
