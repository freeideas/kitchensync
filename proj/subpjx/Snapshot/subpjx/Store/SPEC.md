# Store:

## Purpose

Store owns the one SQLite table that records what a single peer held the last
time we looked. It works only on the local working copy of a peer's snapshot
database -- the temporary file that Transfer has already downloaded (or created
empty) -- and never touches a peer's filesystem directly. Within that working
copy Store owns three things: the schema of the `snapshot` table, the per-peer
row reads and updates that happen as traversal confirms what a peer holds, and
the opportunistic removal of old rows.

It exists so that the schema, the row-transition rules, and the tombstone
cascade live in exactly one place. Other components observe a peer's filesystem
and report what they found or what they intend; they hand those facts to Store,
and Store turns them into rows. Store reuses Identity to compute the `id` and
`parent_id` strings for a path and reuses Clock for the fresh "now" timestamp it
writes into `last_seen`. It does not decide what to sync, does not move files,
and does not move the database file between peer and local temp.

## Responsibilities

The operations Store exposes across its boundary fall into three groups.

### Schema and lifecycle

- Initialize a peer's local working database so it contains exactly one table,
  named `snapshot` (singular, lowercase), and no view (013.1, 013.2, 013.3).
- Define the `snapshot` table with these columns and constraints:
  - `id` TEXT, the primary key (013.4, 013.5).
  - `parent_id` TEXT (013.6).
  - `basename` TEXT not null (013.7, 013.8).
  - `mod_time` TEXT not null (013.9, 013.10).
  - `byte_size` INTEGER not null (013.11, 013.12).
  - `last_seen` TEXT, nullable (013.15).
  - `deleted_time` TEXT, nullable (013.16).
- Store a file's size in bytes in `byte_size`, and store `-1` for a directory
  (013.13, 013.14).
- Create an index on `parent_id`, an index on `last_seen`, and an index on
  `deleted_time` (013.17, 013.18, 013.19).
- Keep at most one row per tracked path: a path's `id` (from Identity) is the
  primary key, so writing the same path again replaces, rather than duplicates,
  its row (013.20).

A path is the unit every row operation below is keyed on. Store derives a row's
`id` and `parent_id` from the path through Identity, so callers name an entry by
its relative path and Store finds or creates the single row that path owns.

### Per-peer row updates during traversal

All of these act on one named peer's working database. The peer is selected per
call; an operation for one peer never reads or writes another peer's database.

- Confirmed present: when an entry is confirmed present on a peer, upsert that
  peer's row for the path so it records the entry's current `mod_time` (017.1)
  and current `byte_size` (017.2), sets `last_seen` to a fresh timestamp from
  Clock (017.3), and sets `deleted_time` to NULL (017.4).
- Confirmed absent (live row): when an entry is confirmed absent on a peer whose
  existing row has `deleted_time` NULL, set that row's `deleted_time` to the
  row's current `last_seen` value (017.5) and leave `last_seen` unchanged
  (017.6). Store reads the existing `last_seen` from the row and copies it; it
  does not mint a new timestamp here.
- Confirmed absent (already tombstoned): when the existing row already has
  `deleted_time` set, leave the row unchanged -- the operation is idempotent
  (017.7).
- Push decision: when the decision is to push an entry to a peer, upsert that
  peer's destination row so it records the winning entry's `mod_time` (017.8)
  and `byte_size` (017.9) with `deleted_time` NULL (017.10), and does not set
  `last_seen` -- so when no prior row exists for that path, `last_seen` remains
  NULL (017.11).
- Copy completed: after a file copy completes successfully, set the destination
  peer's row's `last_seen` to a fresh timestamp from Clock (017.12).
- Inline directory created: after an inline directory creation succeeds on a
  peer, set that peer's row's `last_seen` to a fresh timestamp (017.13).
- Inline operation failed: when an inline filesystem operation fails on a peer,
  leave that peer's existing row unchanged -- Store records no effect and does
  not retry (017.14).
- Interrupted copy: a queued copy whose `last_seen` was never set (because the
  copy did not complete) keeps `deleted_time` NULL (017.21) and keeps
  `last_seen` unchanged -- remaining NULL for a first-time target -- so the next
  run re-enqueues it (017.22). Store guarantees this by never setting
  `last_seen` at push-decision time; only a completed copy sets it.

### Displacement tombstone cascade

- Entry displaced: after an entry is successfully displaced to BAK/ on a peer,
  set that peer's row for the entry's `deleted_time` to the row's current
  `last_seen` value (017.15).
- Cascade to descendants: then set `deleted_time` on every descendant row of the
  displaced entry (017.16). Descendants are the rows reached transitively
  through `parent_id` links from the displaced entry's `id`; only those rows are
  touched, and unrelated rows are left unchanged (017.17).
- Preserve earlier tombstones: the cascade does not overwrite `deleted_time` on
  any descendant row that already has `deleted_time` set (017.18).
- One peer at a time: the cascade for a peer runs against that peer's own
  working database and never against another peer's (017.19). When several peers
  lose the same subtree in one decision, the cascade runs once per peer, each
  against that peer's own database, after that peer's displacement succeeds
  (017.20).

### Opportunistic maintenance

- Remove tombstone rows (`deleted_time IS NOT NULL`) whose `deleted_time` is
  older than `--keep-del-days` days, and keep those whose `deleted_time` is
  within that window (018.1, 018.2).
- Remove a live row (`deleted_time` NULL) that traversal did not visit when its
  `last_seen` is older than `--keep-del-days` days (018.3).
- Run this maintenance opportunistically: it must not delay the first directory
  scan of a run (018.4) or the first eligible file copy (018.5), and the run
  exits 0 even when maintenance does not finish removing every eligible row
  during that run (018.6). Correctness never depends on maintenance completing.

### Dry-run

- Under `--dry-run`, Store still creates and updates the local temp working
  databases exactly as in a normal run, because that working copy is local-only
  state and is never written back to a peer (024.6). The dry-run flag does not
  change any row-update or schema behavior Store performs; only the writeback,
  owned by Transfer, is skipped elsewhere.

## Boundaries

### Operations across the boundary

Store exposes, against a named peer's working database:

- Initialize / open the working database with the `snapshot` schema.
- Read a peer's recorded row for a path (so other components can compare it
  against what they observe).
- Record confirmed-present, confirmed-absent, push-decision, copy-completed,
  inline-directory-created, and inline-operation-failed transitions for a path.
- Run the displacement cascade for a displaced path.
- Run opportunistic maintenance against the working database.

### Error obligations

- Store surfaces database-open and SQLite errors raised while working on a peer's
  database to its caller; it does not decide whether such a failure aborts the
  run.
- When an inline filesystem operation is reported to Store as failed, Store
  leaves the affected row unchanged (017.14); it does not retry the operation and
  raises no error of its own for that case.
- Maintenance never fails the run: a run exits 0 even if maintenance leaves
  eligible rows in place (018.6).

### Invariants

- A peer working database always has exactly one table, named `snapshot`, no
  view, and at most one row per tracked path (013.1, 013.2, 013.3, 013.20).
- `byte_size` is always non-null: a file's size for a file, `-1` for a directory
  (013.12, 013.14).
- A confirmed-absent transition only ever copies an existing `last_seen` into
  `deleted_time`; it never mints a timestamp and never alters `last_seen`
  (017.5, 017.6).
- The displacement cascade touches only the displaced entry's row and the rows
  reachable from it through `parent_id`, and never lowers an existing tombstone
  (017.17, 017.18).
- A cascade or any row operation for one peer reads and writes only that peer's
  own working database (017.19).
- A push decision never sets `last_seen`; only a completed copy or completed
  inline directory creation does, which is what leaves an interrupted copy's row
  re-enqueueable (017.11, 017.12, 017.21, 017.22).

### What Store does not do

- It does not compute the `id`/`parent_id` hash rule itself; it asks Identity for
  those strings from a path.
- It does not generate the timestamp format or the run-wide monotonic sequence;
  it asks Clock for a fresh "now" when it needs one for `last_seen`.
- It does not download, create-on-the-peer, recover, or upload the `snapshot.db`
  file, and it does not run the SWAP state machine; Transfer owns moving the file
  between peer and local temp. Store only operates on the local working copy
  Transfer provides.
- It does not list peers, classify entries, or apply any sync decision rule; it
  records the state and decisions other components hand it.
- It does not perform file copies or BAK displacement on a filesystem; it records
  their snapshot effects only after the owning component reports success.
- It does not honor or own the dry-run decision beyond the fact that its
  local-only updates run unchanged under `--dry-run` (024.6); skipping the
  peer-side writeback is Transfer's concern.
