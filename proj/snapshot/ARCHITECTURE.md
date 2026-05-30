# Snapshot Architecture

The `snapshot` module owns the per-peer SQLite snapshot database and the local
working-copy lifecycle for that database. It is the durable peer-history layer
used by startup, traversal, copy completion, and snapshot upload. The module
records what each peer last confirmed for each relative path and exposes narrow
row mutation APIs. It does not choose sync winners, connect peer URLs, mutate
user files, schedule copy retries, or render progress.

This module should remain a leaf. Its scope is one durable store and its
transport lifecycle. Future implementation can use private Rust files for
schema, lifecycle, path identifiers, mutations, timestamps, and cleanup, but no
child module boundary is needed unless another module must consume a new
explicit API.

## Responsibilities

`snapshot` is responsible for:

- creating and validating each peer snapshot database at
  `.kitchensync/snapshot.db`;
- keeping only the live `snapshot.db` as the peer snapshot database and keeping
  SQLite sidecar files out of peer state;
- downloading each reachable peer snapshot into a distinct local temporary
  `{tmp}/{uuid}/snapshot.db` working copy;
- creating a new empty local snapshot database when the peer has no live
  snapshot at startup;
- serving all snapshot reads and writes during sync from the local working
  copy;
- recovering incomplete peer-side snapshot SWAP state before normal startup
  reads, while skipping that recovery in dry-run mode;
- uploading completed local snapshot databases through the snapshot SWAP
  sequence after queued file copies finish in normal mode;
- defining the snapshot table, indexes, row conversions, and SQLite transaction
  boundaries;
- deriving row identifiers from validated slash-separated relative paths;
- generating process-wide strictly increasing snapshot timestamps;
- exposing lookup APIs by relative path for sync classification;
- exposing mutation APIs for confirmed-present entries, intended copy rows,
  completed copies, absent rows, displaced entries, displaced directory
  cascades, and opportunistic stale-row cleanup;
- reporting recoverable lifecycle failures with enough peer and normalized
  transport error context for runtime diagnostics.

`snapshot` is not responsible for CLI parsing, peer URL normalization, fallback
selection, peer role assignment, exclude matching, traversal decisions, sync
winner selection, user-entry SWAP recovery, BAK/TMP cleanup, safe replacement,
copy concurrency, retry accounting, progress display, or process exit status.

## Public API Shape

The exported API should be behavioral and path-oriented.

- `SnapshotStore`: an opened local SQLite working copy for one peer snapshot.
- Lifecycle APIs: normal startup recovery, dry-run startup download, download or
  create local working database, history-at-startup reporting, flush/close, and
  normal-mode upload through snapshot SWAP.
- Query APIs: look up snapshot row state by root-owned `RelPath`.
- Mutation APIs: `upsert_confirmed_present`, `upsert_intended_copy`,
  `mark_copy_complete`, `mark_absent`, `mark_displaced`,
  `cascade_displaced_directory`, and stale-row cleanup.
- Timestamp API: generate fresh `Timestamp` values for snapshot `last_seen`
  writes and for callers that need fresh BAK or TMP timestamp path segments.

Callers pass root-owned contracts such as `PeerSession`, `RelPath`,
`EntryMeta`, `Timestamp`, `TransportHandle`, normalized `TransportError`, and
retention settings. SQL statements, table names, hash details, SQLite
connection handles, local temporary paths, and row storage structs remain
private to this module.

## Schema and Rows

The peer database is a SQLite rollback-journal database with exactly one
non-internal table named `snapshot`, no views, and indexes on `parent_id`,
`last_seen`, and `deleted_time`.

The `snapshot` table shape is:

- `id TEXT PRIMARY KEY`
- `parent_id TEXT`
- `basename TEXT NOT NULL`
- `mod_time TEXT NOT NULL`
- `byte_size INTEGER NOT NULL`
- `last_seen TEXT NULL`
- `deleted_time TEXT NULL`

Each row represents one tracked relative path. The sync root itself has no row.
Direct children of the root use the hash of `/` as `parent_id`. File rows store
their file size in bytes. Directory rows store `byte_size = -1`. Present,
absent, intended-copy, completed-copy, and tombstone meanings are represented by
these fields and mutation timing, not by extra public state columns.

All stored `mod_time`, `last_seen`, and `deleted_time` values must use the root
`Timestamp` format `YYYY-MM-DD_HH-mm-ss_ffffffZ` when present. Mutation methods
should use SQLite transactions whenever multiple row changes must become
visible together, especially displaced directory cascades.

## Path Identifiers

Path identifier code accepts only validated root `RelPath` values. It computes
`id` and `parent_id` from canonical slash-separated relative paths using
xxHash64 seed `0`, encoded as an 11-character zero-padded base62 string with
digits, uppercase letters, and lowercase letters.

File and directory rows for the same relative path use the same path hash; the
`byte_size` value distinguishes directories. The identifier algorithm remains
private unless a future root contract explicitly needs another module to build
or inspect stored snapshot keys.

## Timestamps

The module owns the process-wide timestamp generator. Every generated timestamp
in one process must be strictly greater than every previously generated
timestamp in that process. Snapshot `last_seen` writes use fresh generated
timestamps.

Copied deletion estimates are not fresh generated timestamps. When
`deleted_time` is copied from an existing `last_seen` or tombstone value, the
copied value may repeat within a run.

## Lifecycle Flow

Normal startup flow:

1. Root supplies a connected `PeerSession`, transport handle, dry-run flag, and
   retention settings.
2. `snapshot` recovers peer-side `.kitchensync/SWAP/snapshot.db/` state before
   reading the live snapshot and before reporting whether history existed.
3. `snapshot` downloads live `.kitchensync/snapshot.db` to a distinct local
   temporary working file.
4. If the live database is missing, `snapshot` creates a new empty local
   database and reports no snapshot history at startup.
5. The module opens and validates the local database and returns a
   `SnapshotStore`.

Dry-run startup skips peer-side snapshot SWAP recovery and downloads the live
`.kitchensync/snapshot.db` exactly as it exists at startup. Dry-run still uses a
local working database for reads, local mutations, and diagnostics, but it does
not upload the database back to peers.

Normal shutdown flow:

1. After all queued file copies finish, each changed `SnapshotStore` is flushed
   and closed.
2. `snapshot` uploads the local database through
   `.kitchensync/SWAP/snapshot.db/`.
3. Upload failures are returned as lifecycle errors. If the failure happens
   after SWAP `old` exists, the peer-side SWAP state is left for a later normal
   startup recovery.

Snapshot upload writes and closes `new`, renames any existing live
`snapshot.db` to `old`, renames `new` to live `snapshot.db`, then deletes
`old`. The implementation must not rely on rename-over-existing.

## Mutation Flows

Traversal calls `upsert_confirmed_present` for entries that were successfully
confirmed present. This records the supplied `mod_time` and `byte_size`, sets
`last_seen` to a fresh generated timestamp, and clears `deleted_time`.

Before a queued destination file copy, sync calls `upsert_intended_copy`. This
records the winning `mod_time` and `byte_size`, clears `deleted_time`, and
leaves existing `last_seen` unchanged. For a first-time destination row,
`last_seen` remains `NULL`.

After a file copy succeeds, operations or sync calls `mark_copy_complete`. This
sets the destination row's `last_seen` to a fresh generated timestamp.

When a path is known absent, callers use `mark_absent`. If an existing row has
`deleted_time = NULL`, the module sets `deleted_time` to that row's current
`last_seen` and leaves `last_seen` unchanged. If `deleted_time` is already
non-NULL, the row is left unchanged.

After successful BAK displacement, callers use `mark_displaced`. The displaced
row's `deleted_time` is copied from its previous `last_seen`. For displaced
directories, the same deletion estimate cascades through non-tombstone
descendants reachable by `parent_id` in the same peer database. A cascade must
not modify another peer database and must not pass through an already
tombstoned or already purged intermediate row.

Failed filesystem effects do not cause snapshot mutation for the affected peer
and subtree. This includes failed directory creation, failed displacement,
listing failure subtree exclusion, unreachable peers, excluded paths, failed
copy attempts, and failed user-entry SWAP recovery.

## Cleanup

Cleanup is opportunistic. Correctness must not depend on cleanup finishing in
the current run, and cleanup must not delay the first directory scan or first
eligible file copy.

Cleanup may remove tombstones older than `--keep-del-days`. It may remove
obsolete non-tombstone rows only when they no longer appear in any peer listing
and have `last_seen` older than `--keep-del-days` or `last_seen = NULL`.

## Error Handling

A missing live `.kitchensync/snapshot.db` during startup download is a nonfatal
new-peer condition. Recovery or download failures other than `not_found` are
peer-level startup failures for the caller to log and exclude from the
reachable set.

Snapshot SWAP recovery is owned only for the snapshot database:

- `old` and live `snapshot.db` exist: delete `new` if present, then delete
  `old`;
- `old` and `new` exist while live is missing: rename `new` to live, then
  delete `old`;
- `old` exists while `new` and live are missing: rename `old` to live;
- `new` and live exist while `old` is missing: delete `new`;
- `new` exists while `old` and live are missing: rename `new` to live.

On upload failure before SWAP `old` exists, any pre-existing live snapshot stays
in place and leftover `new` state is cleaned by later startup recovery. On
upload failure after SWAP `old` exists, the module leaves the SWAP state in
place for a later normal startup recovery.

## Dependencies

`snapshot` depends on:

- SQLite through the chosen Rust SQLite binding;
- local filesystem access for temporary snapshot working files;
- root contracts for `PeerSession`, `RelPath`, `EntryMeta`, `Timestamp`,
  `TransportHandle`, normalized `TransportError`, and retention settings;
- transport operations to read, write, rename, and delete
  `.kitchensync/snapshot.db` and
  `.kitchensync/SWAP/snapshot.db/{new,old}`.

The module must not mutate user data paths, user-file SWAP directories, BAK
directories for user entries, or TMP staging. It must not depend on sync winner
internals, transport implementation details, progress rendering, copy
scheduling, or CLI parsing.

## Boundary Rules

Sibling modules interact with `snapshot` only through explicit Rust APIs.
`sync` can query row state and request row mutations, but it cannot issue SQL or
construct snapshot path hashes. `operations` can report successful peer-side
effects that require row updates, but it cannot edit snapshot database files
directly. `peer` supplies connected sessions and identity through root-owned
contracts; `snapshot` only reports whether the peer's live snapshot database
existed at startup.

Shared contracts live at the narrowest ancestor that needs them. `RelPath`,
`EntryMeta`, `Timestamp`, transport error categories, peer sessions, and
retention settings are root-owned because multiple first-layer modules consume
them. Snapshot row types, table names, SQL, path-key algorithms, and local
working-copy paths remain private to `snapshot`.

Concurrent KitchenSync runs against the same peer are not coordinated here. If
overlapping runs upload snapshots, the last upload wins; correctness relies on
later runs rediscovering any missing decisions from peer contents and snapshot
history.
