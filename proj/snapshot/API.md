# snapshot API

Rust module path: `kitchensync::snapshot`.

The `snapshot` module exports the durable peer-history API used by root,
`sync`, and `operations`. It owns SQLite snapshot working copies, snapshot
download and upload lifecycle, row lookup and mutation, tombstone cleanup, and
process-wide timestamp generation. Callers may rely only on the types and
functions documented here. SQL, SQLite connection handles, table names, local
working-copy paths, path-hash generation, transaction layout, and SWAP recovery
steps are private implementation details.

## Consumed Root Contracts

The public API uses root-owned contracts rather than redefining them:

- `PeerSession`: connected peer identity and transport access.
- `RelPath`: validated slash-separated relative path. The sync root itself is
  not accepted as a snapshot row path.
- `EntryMeta`: listed or stated file/directory metadata, including `EntryKind`,
  modification `Timestamp`, and byte size, with directories represented as
  `-1`.
- `Timestamp`: UTC value formatted as `YYYY-MM-DD_HH-mm-ss_ffffffZ`.
- `TransportError`: normalized transport error category.
- `RetentionPolicy` or the equivalent root run-config field containing
  `keep_del_days`.

## Public Types

```rust
pub struct SnapshotStore { /* private fields */ }
```

Opened local SQLite working copy for one peer snapshot. A `SnapshotStore`
serves all snapshot reads and writes for that peer during a sync run. Stores
are peer-local: mutations on one store must never affect another peer's
snapshot database.

```rust
pub struct SnapshotOpen {
    pub store: SnapshotStore,
    pub had_history_at_startup: bool,
}
```

Result of preparing a peer snapshot. `had_history_at_startup` is `true` only
when the peer had a live `.kitchensync/snapshot.db` at startup.

```rust
pub enum SnapshotStartupMode {
    Normal,
    DryRun,
}
```

Controls peer-side lifecycle behavior. `Normal` performs snapshot SWAP recovery
before reading the live database. `DryRun` skips peer-side snapshot SWAP
recovery and never uploads updated local snapshots.

```rust
pub enum SnapshotEntryKind {
    File,
    Directory,
}
```

Public row classification returned from lookup. Directory rows correspond to
stored byte size `-1`; file rows carry the stored file byte size.

```rust
pub struct SnapshotRow {
    pub path: RelPath,
    pub kind: SnapshotEntryKind,
    pub mod_time: Timestamp,
    pub byte_size: i64,
    pub last_seen: Option<Timestamp>,
    pub deleted_time: Option<Timestamp>,
}
```

Path-oriented snapshot state for sync classification. A row with
`deleted_time = Some(_)` is a tombstone. Callers must not infer or depend on
stored path identifiers.

```rust
pub struct SnapshotCleanupScope<'a> {
    pub listed_paths: &'a dyn SnapshotListedPaths,
    pub retention: RetentionPolicy,
}
```

Inputs for opportunistic stale-row cleanup. `listed_paths` answers whether a
row is still present in any peer listing known to the caller.

```rust
pub trait SnapshotListedPaths {
    fn contains(&self, path: &RelPath) -> bool;
}
```

Minimal query interface used by cleanup to avoid depending on sync traversal
internals.

```rust
pub enum SnapshotError {
    Transport {
        peer: PeerId,
        category: TransportError,
        operation: SnapshotTransportOperation,
    },
    InvalidDatabase {
        peer: PeerId,
        reason: SnapshotDatabaseError,
    },
    LocalIo {
        peer: PeerId,
        operation: SnapshotLocalOperation,
    },
}
```

Recoverable snapshot lifecycle and storage failures. Errors must include enough
peer and operation context for runtime diagnostics. A missing live peer
snapshot during startup download is not returned as an error; it creates an
empty local database with `had_history_at_startup = false`.

```rust
pub enum SnapshotTransportOperation {
    RecoverSwap,
    DownloadLive,
    UploadNew,
    RenameLiveToOld,
    RenameNewToLive,
    DeleteOld,
    DeleteNew,
}

pub enum SnapshotLocalOperation {
    CreateTempDirectory,
    CreateDatabase,
    OpenDatabase,
    FlushDatabase,
    CloseDatabase,
    ReadDatabase,
    WriteDatabase,
}

pub enum SnapshotDatabaseError {
    OpenFailed,
    SchemaMismatch,
    UnsupportedObjects,
    Corrupt,
}
```

Diagnostic enums describing where a snapshot failure occurred. Variants are
stable for matching and rendering; they do not expose backend-specific errors.

## Public Functions

```rust
pub fn prepare_peer_snapshot(
    peer: &PeerSession,
    tmp_root: &std::path::Path,
    mode: SnapshotStartupMode,
) -> Result<SnapshotOpen, SnapshotError>;
```

Downloads or creates the local snapshot working database for `peer`.

In `Normal` mode this function first recovers incomplete peer-side
`.kitchensync/SWAP/snapshot.db/` state, then reads the live
`.kitchensync/snapshot.db`. In `DryRun` mode it skips peer-side recovery and
downloads the live database as it exists at startup. If the live database is
missing, it creates a new empty local database and reports
`had_history_at_startup = false`.

Each successful call returns a `SnapshotStore` backed by a distinct local
temporary `{tmp_root}/{uuid}/snapshot.db` working copy.

```rust
pub fn fresh_timestamp() -> Timestamp;
```

Returns a process-wide fresh timestamp. Every value returned by this function
in one process is strictly greater than every earlier returned value. Snapshot
mutations use this function for fresh `last_seen` writes. Other modules may use
it for fresh BAK or TMP timestamp path segments.

## SnapshotStore Methods

```rust
impl SnapshotStore {
    pub fn peer(&self) -> PeerId;

    pub fn had_changes(&self) -> bool;

    pub fn lookup(&self, path: &RelPath) -> Result<Option<SnapshotRow>, SnapshotError>;

    pub fn upsert_confirmed_present(
        &mut self,
        path: &RelPath,
        meta: &EntryMeta,
    ) -> Result<Timestamp, SnapshotError>;

    pub fn upsert_intended_copy(
        &mut self,
        path: &RelPath,
        winning_meta: &EntryMeta,
    ) -> Result<(), SnapshotError>;

    pub fn mark_copy_complete(
        &mut self,
        path: &RelPath,
    ) -> Result<Timestamp, SnapshotError>;

    pub fn mark_absent(
        &mut self,
        path: &RelPath,
    ) -> Result<(), SnapshotError>;

    pub fn mark_displaced(
        &mut self,
        path: &RelPath,
        kind: SnapshotEntryKind,
    ) -> Result<(), SnapshotError>;

    pub fn cleanup_stale_rows(
        &mut self,
        scope: SnapshotCleanupScope<'_>,
    ) -> Result<(), SnapshotError>;

    pub fn flush(&mut self) -> Result<(), SnapshotError>;

    pub fn close(self) -> Result<ClosedSnapshotStore, SnapshotError>;
}
```

### `peer`

Returns the peer identity associated with the store.

### `had_changes`

Returns whether the local working snapshot has been mutated since it was
opened. Callers may use this to skip unnecessary upload attempts.

### `lookup`

Returns the stored row for `path`, if present. Lookup is by `RelPath` only.
Callers must not use or construct snapshot row identifiers.

### `upsert_confirmed_present`

Records that traversal successfully confirmed `path` as present on this peer.
It stores the supplied `mod_time` and `byte_size`, sets `last_seen` to a fresh
timestamp, clears `deleted_time`, and returns the generated `last_seen`.

### `upsert_intended_copy`

Records an intended queued destination file copy. It stores the winning
metadata, clears `deleted_time`, and leaves any existing `last_seen` unchanged.
For a first-time destination row, `last_seen` remains `NULL`.

### `mark_copy_complete`

Records that a file copy to `path` succeeded. It sets `last_seen` to a fresh
timestamp and returns that timestamp.

### `mark_absent`

Idempotently marks an existing row absent. If the row exists and has
`deleted_time = NULL`, it sets `deleted_time` to that row's current `last_seen`
and does not change `last_seen`. If the row is already tombstoned or does not
exist, it leaves the store unchanged.

### `mark_displaced`

Records a successful BAK displacement. The displaced row's `deleted_time` is
copied from its previous `last_seen`. When `kind` is `Directory`, the same
deletion estimate cascades through non-tombstone descendants reachable through
that directory in this peer's database. The cascade must not pass through an
already tombstoned or purged intermediate row.

### `cleanup_stale_rows`

Opportunistically removes stale rows according to the provided retention
policy. Cleanup may delete tombstones older than `keep_del_days`. It may delete
obsolete non-tombstone rows only when they no longer appear in any peer listing
and their `last_seen` is older than `keep_del_days` or `NULL`. Correctness must
not depend on this method being called or completing in the current run.

### `flush`

Flushes local SQLite state for the working copy. It does not upload to the
peer.

### `close`

Closes the local working database and returns a closed handle eligible for
upload. After `close`, callers cannot perform row reads or mutations on the
store.

## Closed Store Upload

```rust
pub struct ClosedSnapshotStore { /* private fields */ }

pub fn upload_peer_snapshot(
    peer: &PeerSession,
    store: ClosedSnapshotStore,
) -> Result<(), SnapshotError>;
```

Uploads a closed local snapshot database to the peer through
`.kitchensync/SWAP/snapshot.db/`. This function is for normal mode only. Dry
run callers must leave closed local snapshot databases local and must not call
upload.

The upload operation writes and closes `new`, renames any existing live
`snapshot.db` to `old`, renames `new` to live `snapshot.db`, then deletes
`old`. It must not rely on rename-over-existing. Upload failures leave peer
state in a form that a later normal startup recovery can resolve.

## Ownership and Mutation Rules

- `SnapshotStore` owns one local working database and is mutable for all row
  changes. Callers borrow it mutably when they request mutations.
- A store is bound to exactly one peer. Directory cascades and cleanup are
  scoped to that store only.
- Snapshot mutations are performed only after the corresponding peer-side fact
  or filesystem effect has succeeded. Failed listing subtrees, unreachable
  peers, excluded paths, failed directory creation, failed displacement, failed
  copy attempts, and failed user-entry SWAP recovery must not cause row
  mutation for the affected peer and subtree.
- `RelPath`, `EntryMeta`, and `Timestamp` values are copied into snapshot
  storage. Borrowed inputs do not need to outlive the method call.
- `SnapshotRow` values are detached read models. Editing a returned row does not
  affect the store.
- The module does not coordinate concurrent KitchenSync runs against the same
  peer. If overlapping runs upload snapshots, the last upload wins.

## Private Behavior

Other modules must not depend on:

- SQLite table names, SQL statements, connection types, pragmas, or indexes;
- local temporary database paths or UUID generation;
- snapshot row `id` or `parent_id` values;
- xxHash64 or base62 implementation details;
- snapshot SWAP intermediate path layout beyond lifecycle function behavior;
- transaction boundaries, batching, or cleanup scheduling.
