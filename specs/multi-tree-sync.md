# Multi-Tree Synchronization

## Overview

Synchronizes N file trees in a single recursive combined-tree walk. At each directory level: list all peers in parallel, union their entries, decide the authoritative state for each, act, and recurse. The snapshot is consulted per-peer for reconciliation (detecting deletions and modifications) but does not contribute entries to the union — only live peer listings drive traversal.

Subordinate peers (`-` prefix) are listed and receive outcomes, but their entries do not influence decisions. See "Subordinate Peers" below.

## Algorithm

```
function sync_directory(peers, path):
    // Phase 1: List all peers in parallel
    listings = parallel_for_each(peers):
        list_directory(peer, path)  // returns entries or error

    // Phase 1b: Drop peers with listing errors
    failed = [p for p in peers if listings[p] is error]
    for p in failed:
        log(error, "listing failed for {p} at {path}, excluding from this subtree")
    active_peers = peers - failed

    // Phase 2: Union entry names across contributing peers only
    contributing = [p for p in active_peers if not p.is_subordinate]
    all_names = union(contributing.listings.keys())
    // Also include names from subordinate peers (for cleanup), but they don't add to decisions
    all_names = all_names | union(subordinate.listings.keys() for subordinate in active_peers if subordinate.is_subordinate)

    // Phase 2b: Resolve .syncignore before other entries (see ignore.md)
    if ".syncignore" in all_names:
        resolve and sync .syncignore using normal decision rules
        read winning .syncignore from the peer that has it (via filesystem trait)
        if read fails: log warning, use only parent ignore rules for this directory
        else: merge with parent ignore rules
        all_names = all_names - matched_by_ignore_rules
        remove ".syncignore" from all_names  // already handled

    // Phase 3: Decide and act on each entry
    for name in all_names:
        states = gather_states(contributing, listings, name)  // subordinate peers excluded
        snap = snapshot_lookup_per_peer(path/name)
        decision = decide(states, snap)

        // Apply decision to ALL active peers (including subordinate)
        if decision.type == directory:
            recursion_peers = []
            for peer in active_peers:
                if peer has wrong type at path/name:
                    displace(peer, path/name)
                if peer needs dir created:
                    create_dir(peer, path/name)
                if peer needs dir deleted:
                    displace(peer, path/name)
                else:
                    recursion_peers.append(peer)
            update_snapshot(path/name, decision)
            if recursion_peers:
                sync_directory(recursion_peers, path/name)

        if decision.type == file:
            update_snapshot(path/name, decision)
            for peer that has directory at path/name:
                displace(peer, path/name)  // type conflict
            for each dst_peer that needs the file:
                enqueue_copy(decision.src_peer, path/name, dst_peer, path/name)
            for each peer where file should be deleted:
                displace(peer, path/name)
```

**All displacement is inline.** Every displacement (type conflicts, deletions) executes during the combined-tree walk, not in the operation queue. Displacement is a same-filesystem rename to BAK/ — fast on any transport. Running it inline eliminates ordering dependencies between displacement and file copies (e.g., a type-conflict directory must be gone before a file copy can rename into that path).

**Directory deletion:** Do not recurse into a directory that is being displaced on a peer. The displacement moves the entire subtree in a single rename, and the snapshot cascade marks all children as deleted. Only peers that are keeping the directory participate in recursion.

**Listing errors:** If `list_directory` fails for a specific path on a reachable peer, that peer is excluded from decisions for that directory and its entire subtree (equivalent to an offline peer for that path). The error is logged at `error` level. The peer's snapshot rows for that subtree are not modified — `last_seen` is not updated, so no false deletions are inferred.

## Subordinate Peers

A subordinate peer (`-` prefix on the command line) participates in listing and receives file operations, but does not contribute to decisions:

- Its entries are **not included** in the `gather_states` step — decisions are made as if the subordinate peer doesn't exist.
- After a decision is made, the subordinate peer is brought into conformance: files it has that shouldn't exist are displaced to BAK/, files it lacks are copied to it, directories are created or removed as needed.
- Its snapshot is still downloaded, updated during traversal, and uploaded back. On future runs without `-`, the peer participates normally.

This means a subordinate peer with pre-existing files that differ from the group's state will have those files displaced — it is made to match the group, not the other way around.

## Built-in Excludes

Always excluded from listings (never synced):

- `.kitchensync/` directories — sync metadata must not sync
- Symbolic links (files and directories) — following symlinks could escape the sync root or create loops
- Special files (devices, FIFOs, sockets)

Excluded by default but may be overridden by a `!.git/` entry in `.syncignore` (see ignore.md):

- `.git/` directories

## BAK/TMP Cleanup During Traversal

After processing the union of entry names at each directory level, separately check each peer for a `.kitchensync/` directory at the current path (using `list_dir` or `stat` directly — this is a metadata operation, not a sync operation, so the built-in exclude does not apply). If present, list its `BAK/` and `TMP/` subdirectories and purge expired entries:

- `.kitchensync/BAK/<timestamp>/` — remove entries older than `--bd` days
- `.kitchensync/TMP/<timestamp>/` — remove entries older than `--xd` days

This piggybacks on the existing traversal — no separate tree walk is needed. The `<timestamp>` component of each subdirectory name determines its age.

## Entry Classification

For each **file** entry, compare each contributing peer's state to that peer's snapshot row. (Directories use existence-based decisions only — see "Directory Decisions" below.)

| Peer State               | Snapshot row for **this peer** | `deleted_time` | Classification                                 |
| ------------------------ | ------------------------------ | -------------- | ---------------------------------------------- |
| Live, same mod_time      | Exists                         | NULL           | Unchanged                                      |
| Live, different mod_time | Exists                         | NULL           | Modified                                       |
| Live                     | Exists                         | NOT NULL       | Modified (resurrection — clear `deleted_time`) |
| Live                     | No row                         | —              | New (peer has never had this entry)            |
| Absent                   | Exists                         | NOT NULL       | Deleted (estimate = `deleted_time`)            |
| Absent                   | Exists                         | NULL           | Absent-unconfirmed (see rule 4b)               |
| Absent                   | No row                         | —              | — (never existed on this peer, no opinion)     |

## Decision Rules

### With a canon peer (`+`)

The canonical peer's state wins unconditionally:
- Canon has file → push to all others (including subordinate peers)
- Canon lacks file → delete everywhere else
- Canon is unreachable → exit with error at startup

### Without a canon peer

Only contributing (non-subordinate) peers participate in decisions:

1. **All unchanged** → no action
2. **Modified** → newest mod_time wins; push to all that don't match
3. **New** → newest mod_time wins; push to all peers that lack it (including peers with no snapshot row)
4. **Deleted + existing** → compare the deletion estimate against the existing file's mod_time. The deletion estimate is the `last_seen` or `deleted_time` of the absent peer (see 4b for which applies). If multiple peers have deleted the entry, use the most recent estimate among the deleting peers. If the deletion estimate > mod_time, deletion wins (displace the file on all peers that have it). If mod_time ≥ the deletion estimate, the existing file wins (push to peers that lack it)
4b. **Absent-unconfirmed** (absent, snapshot row exists, `deleted_time` NULL) → compare `last_seen` against the max mod_time of peers that have the entry. If `last_seen` > max mod_time, this is a deletion — the entry was confirmed present on this peer after the latest modification anywhere, and has since been removed. Apply rule 4 using `last_seen` as the deletion estimate. If `last_seen` ≤ max mod_time (or `last_seen` is NULL), this is a failed copy or the peer has never successfully received the file — re-enqueue the copy, no deletion vote
5. **Same mod_time, different size** → larger file wins
6. **Ties** → keep data (existence over deletion, larger over smaller)

Peers with no snapshot row for the entry ("never had it") do not vote — they are simply targets for propagation once a winner is decided.

If no contributing peer votes (all have "absent, no row"), the entry does not exist in the group's view. No copy is enqueued. Subordinate peers that have the entry are displaced to BAK/.

If the winning entry already exists on a peer with a matching mod_time (within tolerance) and matching byte_size, no copy is performed for that peer — only the snapshot row is created/updated.

Timestamp tolerance: 5 seconds in either direction. The tolerance applies to Entry Classification: a peer's mod_time is considered "same" as the snapshot row's mod_time if it differs by ≤ 5 seconds. When comparing peers' mod_times in Decision Rules, find the maximum mod_time among all peers that have the entry. Any peer whose mod_time is within 5 seconds of the maximum is treated as tied with the maximum (fall through to rules 5/6). Peers whose mod_time is more than 5 seconds behind the maximum lose to it. The same tolerance applies when comparing a deletion estimate (`deleted_time`) against a file's mod_time in rule 4. The same tolerance applies to rule 4b: `last_seen` must exceed `max mod_time` by more than 5 seconds to be considered a deletion.

## Orphaned Snapshot Rows

Snapshot rows for entries that no longer appear in any peer's listing are never visited during traversal. They are cleaned up by the startup purge (see sync.md, Run step 1): tombstone rows (where `deleted_time IS NOT NULL`) with `deleted_time` older than `--td` days are deleted. Additionally, rows where `deleted_time IS NULL` and `last_seen` is older than `--td` days (or `last_seen` is NULL) are also deleted — these are stale rows from entries that disappeared without being visited.

## Directory Decisions

Directories do not use mod_time for decision-making. Directory mod_times are filesystem bookkeeping (they change when children are added or removed) and vary in precision across filesystem types — they do not represent meaningful user intent.

Directory decisions are existence-based only:
- If any contributing peer has the directory, it should exist on all peers. Create it on peers that lack it.
- If all contributing peers have deleted the directory (tombstone in snapshot, absent in listing), delete it on remaining peers (displace to BAK/).
- Canon peer (`+`) overrides as usual: canon has it → create everywhere; canon lacks it → delete everywhere.

Directories are displaced to BAK/ just like files. The snapshot still tracks directories (with `byte_size = -1`) for deletion detection via tombstones, but `mod_time` for directory rows is informational only — it is recorded but not used in decisions.

## Type Conflicts

When the same path is a file on one peer and a directory on another: if a canon peer is present, its type wins — the tiebreaker below applies only when no canon peer is designated, or when the canon peer does not have an entry at that path. Otherwise, the file wins. The directory is displaced to BAK/ on the peer(s) that have it, then the winning entry is synced to all peers.

## Snapshot Updates

Per-peer snapshot rows are updated during traversal, as soon as a decision is made — before the actual file operations (create, delete, copy) execute. The snapshot reflects the decided state of the shared tree, not what has physically happened yet.

- **Entry confirmed present** on a peer: upsert row with current mod_time, byte_size, `last_seen` set to the current sync timestamp, and `deleted_time = NULL`
- **Entry confirmed absent** on a peer with an existing row where `deleted_time` is NULL: set `deleted_time` to the row's current `last_seen` value (the deletion happened sometime after that point). Do not update `last_seen`.
- **Entry confirmed absent** on a peer with an existing row where `deleted_time` is already set: no change (tombstone already recorded)
- **Decision: push to a peer**: upsert row for the destination peer with the winning entry's mod_time, byte_size, and `deleted_time = NULL`. Do **not** update `last_seen` — it is only set when the entry is confirmed present (in a listing or after a completed copy). If no row exists yet, `last_seen` is NULL.
- **Copy completed**: after a file copy finishes successfully, set `last_seen` to the current sync timestamp on the destination peer's snapshot row. This is the only post-traversal snapshot update.
- **Inline directory creation completed**: after `create_dir` succeeds on a destination peer, set `last_seen` to the current sync timestamp on that peer's snapshot row. Directory creation is both decided and confirmed in one step (unlike file copies, which are enqueued).
- **Decision: delete from a peer**: set `deleted_time` to the row's current `last_seen` on the row for that peer (the entry is being displaced to BAK/). Then cascade to descendants using a single recursive CTE scoped to the displaced entry's subtree:
  ```sql
  WITH RECURSIVE subtree(id) AS (
      VALUES(?displaced_id)
      UNION ALL
      SELECT s.id FROM snapshot s
      JOIN subtree st ON s.parent_id = st.id
      WHERE s.deleted_time IS NULL
  )
  UPDATE snapshot
  SET deleted_time = ?deleted_time
  WHERE deleted_time IS NULL
  AND id IN (SELECT id FROM subtree);
  ```
  This walks down from the displaced directory's `id` through `parent_id` links, marking only its descendants — not unrelated rows that happen to have a tombstoned parent. If the cascade cannot reach all descendants due to purged intermediate rows, those orphaned rows will be cleaned up by the startup purge when their `last_seen` exceeds `--td` days.

If the app exits before copies finish, the destination row has `deleted_time = NULL` and `last_seen` unchanged (NULL for first-time targets, or the old confirmation time). The next run sees the entry as absent-unconfirmed and applies rule 4b: since `last_seen` is NULL or old, it does not exceed the source's mod_time, so the copy is re-enqueued.

## Offline Peers

Unreachable peers are excluded entirely — they do not participate in listings or decisions. Their snapshot rows are not modified (`last_seen` is not updated). On the next run when they're reachable, discrepancies between their filesystem state and their snapshot rows drive sync decisions, bringing them up to date.
