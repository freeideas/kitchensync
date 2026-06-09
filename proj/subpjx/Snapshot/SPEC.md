# Snapshot:

## Purpose

Snapshot owns every peer's SQLite snapshot database: its schema, the path
hashing that gives each tracked entry a stable identity, the run-wide monotonic
timestamp source, the per-peer row reads and updates that record what each peer
holds, the tombstones that record deletions, the opportunistic cleanup of old
rows, and the download, recovery, and SWAP-staged upload of the `snapshot.db`
file itself.

It exists so that all knowledge of "what a peer held last time we looked" lives
in one place. Every other component asks Snapshot what a peer's prior state was
and tells Snapshot the new state it observed or intends; nothing else opens a
snapshot database, computes a path identity, or decides where the file lives on
a peer. Concentrating this here keeps the schema, the hashing rule, and the
timestamp generator consistent across the whole run.

## Responsibilities

The operations Snapshot exposes across its boundary fall into five groups.

Database schema and lifecycle:

- Create a peer's local working snapshot database with exactly one table named
  `snapshot` and no view (013.1, 013.2, 013.3).
- Define that table with the columns `id` TEXT primary key, `parent_id` TEXT,
  `basename` TEXT not null, `mod_time` TEXT not null, `byte_size` INTEGER not
  null, `last_seen` TEXT nullable, and `deleted_time` TEXT nullable (013.4
  through 013.16).
- Store a file's size in bytes in `byte_size` and store `-1` for a directory
  (013.13, 013.14).
- Create indexes on `parent_id`, `last_seen`, and `deleted_time` (013.17,
  013.18, 013.19).
- Hold at most one row per tracked path (013.20).

Path hashing and identity:

- Compute an entry's identity as the xxHash64 (seed 0) of its canonical relative
  path, base62-encoded with digits `0-9`, then `A-Z`, then `a-z`, zero-padded to
  an 11-character string (014.1, 014.2, 014.3).
- Feed the hash a canonical path that uses forward slashes and has no leading or
  trailing slash, so a file and a directory with the same path share an identity
  (014.4, 014.5, 014.6, 014.7).
- Honor the worked examples: the identity of `docs/readme.txt` is the hash of
  `docs/readme.txt`, the identity of directory `docs/notes` is the hash of
  `docs/notes`, and both have parent identity equal to the hash of `docs`
  (014.8, 014.9, 014.10, 014.11).
- Use the hash of the sentinel `/` as the parent identity of a root-level entry,
  and track only the sync root's children, never the sync root itself (014.12,
  014.13).

Monotonic timestamps:

- Provide the single timestamp string format `YYYY-MM-DD_HH-mm-ss_ffffffZ`, UTC,
  microsecond precision, lexicographically sortable and filesystem-safe, used for
  database columns, BAK/ and TMP/ directory names, and log output (015.1 through
  015.5).
- Provide the run-wide generator: every request for a fresh "now" returns a value
  strictly greater than any it returned before in the process (adding one
  microsecond on collision), so no two freshly generated timestamps in one run are
  equal (015.6, 015.7, 015.8).
- Treat `deleted_time` as a copied deletion estimate taken from a row's existing
  `last_seen`, reused across descendant cascades, and exempt from the uniqueness
  rule (015.9, 015.10).

Per-peer row reads and updates during traversal:

- Read a peer's prior recorded state for a path so other components can compare it
  against what they observe.
- On confirmed-present, upsert the row's `mod_time` and `byte_size`, set
  `last_seen` to a fresh timestamp, and clear `deleted_time` to NULL (017.1
  through 017.4).
- On confirmed-absent of a live row (`deleted_time` NULL), set `deleted_time` to
  that row's current `last_seen` without touching `last_seen`, and leave an
  already-tombstoned row unchanged (017.5, 017.6, 017.7).
- On a push decision, upsert the winning `mod_time` and `byte_size` with
  `deleted_time` NULL and without setting `last_seen`, leaving it NULL when no
  prior row exists (017.8 through 017.11).
- On a completed file copy or a completed inline directory creation, set
  `last_seen` to a fresh timestamp (017.12, 017.13).
- Leave an existing row unchanged when an inline filesystem operation fails
  (017.14).
- On a successful displacement, set the entry's `deleted_time` to the row's
  current `last_seen`, then cascade `deleted_time` to descendant rows reached
  through `parent_id` links, without overwriting descendants that already have
  `deleted_time` set and without touching unrelated rows (017.15 through 017.18).
- Run the displacement cascade against that peer's own snapshot database only,
  once per peer after that peer's displacement succeeds, even when several peers
  lose the same subtree (017.19, 017.20).
- Leave a queued copy's destination row with `deleted_time` NULL and `last_seen`
  unchanged when a run exits before the copy completes, so it is re-enqueued next
  run (017.21, 017.22).

Opportunistic maintenance:

- Remove tombstone rows (`deleted_time IS NOT NULL`) older than `--keep-del-days`
  and keep those within the window (018.1, 018.2).
- Remove a stale live row (`deleted_time` NULL) that traversal does not visit when
  its `last_seen` is older than `--keep-del-days` (018.3).
- Perform this maintenance opportunistically: never delay the first directory scan
  or the first eligible copy, and let the run exit 0 even if maintenance does not
  finish (018.4, 018.5, 018.6).

Storage, recovery, and SWAP-staged transfer:

- Treat each peer's snapshot as living at `{peer-root}/.kitchensync/snapshot.db`,
  a rollback-journal SQLite file, with SQLite sidecar files never synced (016.1,
  016.2, 016.3).
- Download a peer's `snapshot.db` to a local temporary path
  `{tmp}/{uuid}/snapshot.db` where all reads and writes happen, leaving the peer
  copy untouched until writeback; create a new empty database locally when the
  transport reports the peer has none (016.4, 016.5, 016.6).
- Before upload, commit or roll back all database work and close every connection,
  statement, and cursor so the uploaded file is self-contained and opens standalone
  with all of the run's changes committed (016.7).
- Write back through the snapshot SWAP path: write and close
  `.kitchensync/SWAP/snapshot.db/new`, rename the live `snapshot.db` to
  `.kitchensync/SWAP/snapshot.db/old` when it exists, rename `new` into place, then
  delete `old`, never relying on rename-over-existing (016.8 through 016.12).
- Apply the five snapshot SWAP recovery states before deciding whether a peer has
  history, honoring last-upload-wins for overlapping runs and leaving SWAP state in
  place for next-run recovery when upload fails before or after `old` exists
  (016.13 through 016.21).

Dry-run handling:

- Under `--dry-run`, download each reachable peer's live
  `.kitchensync/snapshot.db` as-is, but skip the peer-side SWAP recovery that a
  normal run applies at startup (024.2, 024.3).
- Under `--dry-run`, still create and update the local temp snapshot databases,
  because that working copy is local-only state (024.6).
- Under `--dry-run`, do not write the updated local temp snapshot back to the
  peer: skip the SWAP-staged upload entirely so no peer snapshot state changes
  (024.18).

## Boundaries

Error obligations:

- Snapshot surfaces database-open and SQLite errors and the transport error
  categories raised while downloading, recovering, or uploading `snapshot.db` to
  its caller; it does not decide whether such a failure aborts the run.
- When an inline filesystem operation reported to Snapshot failed, Snapshot leaves
  the affected row unchanged (017.14); it does not retry the operation.
- A snapshot upload that fails leaves the live file and SWAP state in a state the
  next run's recovery can resolve (016.20, 016.21); Snapshot does not roll the
  peer back beyond that.

Invariants:

- A peer working database always has exactly one table, named `snapshot`, no view,
  and at most one row per tracked path (013.1, 013.2, 013.3, 013.20).
- The sync root has no row; only its children are tracked (014.13).
- Within one run, every freshly generated timestamp is strictly greater than every
  earlier one, so timestamps sort chronologically as plain strings and never
  collide (015.4, 015.8).
- A peer's `.kitchensync/snapshot.db` is never modified in place during the run;
  all changes happen on the local working copy and reach the peer only through the
  SWAP-staged writeback (016.5, 016.8 through 016.12).
- The displacement cascade for a peer touches only that peer's database and only
  rows reachable as descendants through `parent_id` (017.17, 017.19).

What Snapshot does not do:

- It does not list peers, classify entries, or apply the canon/subordinate/
  bidirectional decision rules; it records the state and decisions other components
  hand it.
- It does not perform user-file copies or BAK displacement on the filesystem; it
  records their snapshot effects after the owning component reports success.
- It does not emit progress lines; snapshot work produces no `C`/`X` output.
- It does not own the global dry-run decision; the run lifecycle hands Snapshot
  the flag, and Snapshot honors it only for its own peer-touching steps: it skips
  startup SWAP recovery and the writeback upload while still downloading the live
  database and updating the local temp copy (024.2, 024.3, 024.6, 024.18).

Snapshot is a per-run singleton: the monotonic timestamp generator must be a
single source for the whole process, and snapshot databases are managed centrally
so the schema, hashing, and timestamp rules stay uniform. It depends on Transport
to move `snapshot.db` between the peer and the local temp directory and to perform
the SWAP renames and deletes; it shares its timestamp format and generator with
the components that name BAK/ and TMP/ directories and that write log output.

Construction and the hidden helpers:

- Snapshot is split internally into private helpers it owns and builds itself:
  the timestamp clock, the path-identity hasher, the row store over the local
  working database, and the database-file transfer helper. These helpers are an
  implementation detail of Snapshot, not part of its public surface.
- The function that creates a Snapshot instance takes exactly one parameter, the
  shared Transport service it depends on. It constructs its own clock, identity,
  store, and transfer helpers internally; a caller hands it only the Transport
  service and never names, imports, or constructs any of those helpers. No
  parameter or return type of any public Snapshot operation, and no parameter of
  its constructor other than the Transport service, is a type that belongs to the
  clock, identity, store, or transfer helper. Those helper types stay entirely
  behind the Snapshot boundary.
