# Multi-Tree Synchronization

## Overview

Synchronizes N file trees in a single recursive combined-tree walk. At each directory level: list all peers in parallel, union their entries, decide the authoritative state for each, act, and recurse. The traversal is pre-order: every entry in a directory is decided and acted on before recursing into any subdirectory. Entry traversal order within a directory is deterministic, case-insensitive lexicographic order with the original case-sensitive name as a tie-breaker. This means a directory marked for displacement is renamed (with its entire subtree) before its children are ever visited - there is no separate "file deletion" pass. The snapshot is consulted per-peer for reconciliation (detecting deletions and modifications) but does not contribute entries to the union - only live peer listings drive traversal.

Subordinate peers (`-` prefix) are listed and receive outcomes, but their entries do not influence decisions. See "Subordinate Peers" below.

## Algorithm

```
function sync_directory(peers, path):
    // Phase 0: Recover incomplete swaps before interpreting live state
    if not dry_run:
        parallel_for_each(peers):
            recover_swaps(peer, path)

    // Phase 1: List all peers in parallel
    listings = parallel_for_each(peers):
        list_directory(peer, path)  // returns entries or error

    // Phase 1b: Drop peers with listing errors
    failed = [p for p in peers if listings[p] is error]
    for p in failed:
        log(error, "listing failed for {p} at {path}, excluding from this subtree")
    if canon peer is in failed:
        return  // no authoritative state; no decisions or subordinate file displacement
    active_peers = peers - failed

    // If all contributing peers failed listing, skip this directory entirely
    contributing = [p for p in active_peers if not p.is_subordinate]
    if not contributing:
        return  // no decisions, no subordinate file displacement

    // Phase 2: Union entry names across contributing peers only
    all_names = union(contributing.listings.keys())
    // Also include names from subordinate peers (for cleanup), but they don't add to decisions
    all_names = all_names | union(subordinate.listings.keys() for subordinate in active_peers if subordinate.is_subordinate)

    // Phase 2a: Apply built-in and command-line excludes
    all_names = all_names - matched_by_built_in_excludes - matched_by_command_line_excludes

    // Phase 3: Decide and act on each entry
    ordered_names = sort_case_insensitive_then_case_sensitive(all_names)
    for name in ordered_names:
        states = gather_states(contributing, listings, name)  // subordinate peers excluded
        snap = snapshot_lookup_per_peer(path/name)
        decision = decide(states, snap)

        // Apply decision to ALL active peers (including subordinate)
        if decision.type == directory:
            recursion_peers = []
            for peer in active_peers:
                if directory should exist on peer:
                    if peer has wrong type at path/name:
                        if displace(peer, path/name) succeeds:
                            update_snapshot_deleted(peer, path/name)
                        else:
                            continue
                    if peer lacks directory at path/name:
                        if create_dir(peer, path/name) succeeds:
                            update_snapshot_present(peer, path/name)
                            recursion_peers.append(peer)
                    else:
                        update_snapshot_present(peer, path/name)
                        recursion_peers.append(peer)
                else if peer has any entry at path/name:
                    if displace(peer, path/name) succeeds:
                        update_snapshot_deleted(peer, path/name)
            if recursion_peers:
                sync_directory(recursion_peers, path/name)

        if decision.type == file:
            update_snapshot_listed_sources(path/name, decision)
            for peer that has directory at path/name:
                if displace(peer, path/name) succeeds:  // type conflict
                    update_snapshot_deleted(peer, path/name)
            for each dst_peer that needs the file:
                update_snapshot_intended_copy(dst_peer, path/name, decision)
                enqueue_copy(decision.src_peer, path/name, dst_peer, path/name)
            for each peer where file should be deleted:
                if displace(peer, path/name) succeeds:
                    update_snapshot_deleted(peer, path/name)
```

**All displacement is inline.** Every displacement (type conflicts, deletions) executes during the combined-tree walk, not in the operation queue. Displacement is a same-filesystem rename to BAK/. Running it inline eliminates ordering dependencies between displacement and file copies (e.g., a type-conflict directory must be gone before a file copy can rename into that path).

**Directory deletion:** Do not recurse into a directory that is being displaced on a peer. The displacement moves the entire subtree in a single rename, and the snapshot cascade marks all children as deleted. Only peers that are keeping the directory participate in recursion. Because the traversal is pre-order (decide every entry before recursing), a displaced directory is always renamed as a whole before any of its children are visited - never split files and directories into separate passes.

**Listing errors:** If `list_directory` fails for a specific path on a reachable peer, try that same listing up to `--retries-list` total times. Directory listing failures are not placed in the file-copy queue; they affect visibility for a whole subtree, not one file transfer.

If listing still fails after all allowed tries, that peer is excluded from decisions for that directory and its entire subtree (equivalent to an offline peer for that path). The error is logged at `error` level. The peer's snapshot rows for that subtree are not modified - `last_seen` is not updated, so no false deletions are inferred. No files or directories are created, deleted, displaced, or copied on that peer under the failed subtree during this run.

If the failed listing is for the canon peer (`+`), skip decisions for that
directory and its entire subtree for all peers. No other peer can supply
authoritative state for that path while canon is unavailable, so no peer files
or snapshot rows are modified under that subtree during this run.

If all contributing peers fail listing for a directory (none of the contributing peers remain in `active_peers` for that level), skip decisions for that directory and its entire subtree - no entries are processed and no subordinate peer files are displaced. On the next run, the failed peer participates normally again if listing succeeds.

**Excludes:** Built-in excludes and paths supplied with `-x` are removed from
the entry union before decisions are made. A matching directory is not recursed
into. A matching file is not copied or deleted. Excluded paths do not consult or
update snapshot rows during the run, and existing entries on any peer are left
in place.

## Subordinate Peers

A subordinate peer (`-` prefix on the command line) participates in listing and receives file operations, but does not contribute to decisions:

- Its entries are **not included** in the `gather_states` step - decisions are made as if the subordinate peer doesn't exist.
- After a decision is made, the subordinate peer is brought into conformance: files it has that shouldn't exist are displaced to BAK/, files it lacks are copied to it, directories are created or removed as needed.
- Its snapshot is still downloaded, updated during traversal, and uploaded back. In `--dry-run`, the local temp snapshot is updated but not uploaded. On future real runs without `-`, the peer participates normally.

This means a subordinate peer with pre-existing files that differ from the group's state will have those files displaced - it is made to match the group, not the other way around.

## Built-in Excludes

Always excluded from listings (never synced):

- `.kitchensync/` directories - sync metadata must not sync
- `.git/` directories - repository metadata must not sync
- Symbolic links (files and directories) - following symlinks could escape the sync root or create loops
- Special files (devices, FIFOs, sockets)

Excluded by explicit command-line request:

- Paths supplied with `-x <relative-path>`. These excludes are not overridden by
  anything else.

## SWAP Recovery During Traversal

In `--dry-run`, peer-side SWAP recovery during traversal is skipped.

Before listing a directory for sync decisions in a normal run, check each peer for
`.kitchensync/SWAP/` at that directory level. Each direct child is one
`<encoded-basename>` swap directory for the corresponding user entry in the
same parent directory. Recover every swap directory before the directory's live
entries are listed for sync decisions.

For target `<basename>`:

- `old` exists and target exists: replacement completed; move `old` to BAK and
  remove the empty SWAP directory.
- `old` exists, `new` exists, and target is missing: rename `new` to target,
  move `old` to BAK, and remove the empty SWAP directory.
- `old` exists, `new` is missing, and target is missing: rename `old` back to
  target and remove the empty SWAP directory.
- `old` is missing, `new` exists, and target exists: delete `new` and remove
  the empty SWAP directory.
- `old` is missing, `new` exists, and target is missing: rename `new` to target
  and remove the empty SWAP directory.

If recovery for a swap directory fails, treat that peer's listing for the
current directory as failed. The peer is excluded from this directory subtree
using the normal listing-error rules, and its snapshot rows for the subtree are
not modified.

## BAK/TMP Cleanup During Traversal

In normal runs, after processing the union of entry names at each directory
level, separately check each peer for a `.kitchensync/` directory at the current
path (using `list_dir` or `stat` directly - this is a metadata operation, not a
sync operation, so the built-in exclude does not apply). If present, list its
`BAK/` and `TMP/` subdirectories and purge expired entries:

- `.kitchensync/BAK/<timestamp>/` - remove entries older than `--keep-bak-days` days
- `.kitchensync/TMP/<timestamp>/` - remove entries older than `--keep-tmp-days` days

This piggybacks on the existing traversal - no separate tree walk is needed. The `<timestamp>` component of each subdirectory name determines its age.

Do not purge `.kitchensync/SWAP/` by age. SWAP directories are recovered before
listing and are deleted only by successful recovery.

In `--dry-run`, BAK/TMP cleanup on peers is skipped.

## Entry Classification

For each **file** entry, compare each contributing peer's state to that peer's snapshot row. A live file is unchanged only when both its mod_time and byte_size match the snapshot row. (Directories are decided by existence and tombstone evidence, never their own mod_time - see "Directory Decisions" below.)

| Peer State               | Snapshot row for **this peer** | `deleted_time` | Classification                                 |
| ------------------------ | ------------------------------ | -------------- | ---------------------------------------------- |
| Live, same mod_time and byte_size | Exists                | NULL           | Unchanged                                      |
| Live, different mod_time or byte_size | Exists           | NULL           | Modified                                       |
| Live                     | Exists                         | NOT NULL       | Modified (resurrection - clear `deleted_time`) |
| Live                     | No row                         | -              | New (peer has never had this entry)            |
| Absent                   | Exists                         | NOT NULL       | Deleted (estimate = `deleted_time`)            |
| Absent                   | Exists                         | NULL           | Absent-unconfirmed (see rule 4b)               |
| Absent                   | No row                         | -              | - (never existed on this peer, no opinion)     |

## Decision Rules

### With a canon peer (`+`)

The canonical peer's state wins unconditionally:
- Canon has file -> push to all others (including subordinate peers)
- Canon lacks file -> delete everywhere else
- Canon is unreachable -> exit with error at startup

### Without a canon peer

Only contributing (non-subordinate) peers participate in decisions:

1. **All contributing peers unchanged and matching** -> the unchanged entry is
   the group outcome. No copy is needed between contributing peers that already
   match, but any active peer that lacks the entry or has the wrong type
   (including subordinate peers) is brought into conformance.
2. **Modified** -> newest mod_time wins; push to all that don't match
3. **New** -> newest mod_time wins; push to all peers that lack it (including peers with no snapshot row)
4. **Deleted + existing** -> compare the deletion estimate against the existing file's mod_time. The deletion estimate is the `last_seen` or `deleted_time` of the absent peer (see 4b for which applies). If multiple peers have deleted the entry, use the most recent estimate among the deleting peers. If the deletion estimate > mod_time, deletion wins (displace the file on all peers that have it). If mod_time >= the deletion estimate, the existing file wins (push to peers that lack it)
4b. **Absent-unconfirmed** (absent, snapshot row exists, `deleted_time` NULL) -> compare `last_seen` against the max mod_time of peers that have the entry. If `last_seen` > max mod_time, this is a deletion - the entry was confirmed present on this peer after the latest modification anywhere, and has since been removed. Apply rule 4 using `last_seen` as the deletion estimate. If `last_seen` <= max mod_time (or `last_seen` is NULL), this is a failed copy or the peer has never successfully received the file - re-enqueue the copy, no deletion vote
5. **Same mod_time, different size** -> larger file wins
6. **Ties** -> keep data (existence over deletion, larger over smaller)
7. **Exact tie** (mod_time within tolerance and equal byte_size) -> the entries
   are treated as identical, even if their bytes differ. No copy is enqueued
   between the tied peers; each keeps its current content, and only snapshot
   rows are updated. Content is never read or hashed to break a tie -
   decisions use only mod_time and byte_size. When another peer needs the
   entry, any one tied peer may be chosen as the copy source; the recorded
   winning mod_time and byte_size are the tied values either way.

Peers with no snapshot row for the entry ("never had it") do not vote - they are simply targets for propagation once a winner is decided.

If no contributing peer votes (all have "absent, no row"), the entry does not exist in the group's view. No copy is enqueued. Subordinate peers that have the entry are displaced to BAK/.

If the winning entry already exists on a peer with a matching mod_time (within tolerance) and matching byte_size, no copy is performed for that peer - only the snapshot row is created/updated.

Timestamp tolerance: 5 seconds in either direction. The tolerance applies to Entry Classification: a peer's mod_time is considered "same" as the snapshot row's mod_time if it differs by <= 5 seconds. When comparing peers' mod_times in Decision Rules, find the maximum mod_time among all peers that have the entry. Any peer whose mod_time is within 5 seconds of the maximum is treated as tied with the maximum (fall through to rules 5/6). Peers whose mod_time is more than 5 seconds behind the maximum lose to it. The same tolerance applies when comparing a deletion estimate (`deleted_time`) against a file's mod_time in rule 4. The same tolerance applies to rule 4b: `last_seen` must exceed `max mod_time` by more than 5 seconds to be considered a deletion.

## Orphaned Snapshot Rows

Snapshot rows for entries that no longer appear in any peer's listing may not be visited during traversal. They are cleaned up opportunistically, not by a blocking startup phase. Cleanup may run while visiting related directories, in bounded background work after traversal has begun, or after copying has already started. Correctness must not depend on cleanup finishing in the current run.

Cleanup removes tombstone rows (where `deleted_time IS NOT NULL`) with `deleted_time` older than `--keep-del-days` days. It may also remove stale rows where `deleted_time IS NULL` and `last_seen` is older than `--keep-del-days` days, or where `last_seen` is NULL, when those rows are known to be obsolete.

## Directory Decisions

Directories do not use mod_time for decision-making. Directory mod_times are filesystem bookkeeping (they change when children are added or removed) and vary in precision across filesystem types - they do not represent meaningful user intent.

Directory decisions use existence, snapshot tombstone evidence, and the
mod_times of live **files** inside the directory's subtree (never the
directory's own mod_time):

- Canon peer (`+`) overrides as usual: canon has it -> create everywhere;
  canon lacks it -> delete everywhere.
- If every contributing peer that votes has the directory live, it should
  exist on all peers. Create it on active peers that lack it, and recurse.
- If at least one contributing peer has the directory live and at least one
  contributing peer votes deletion - absent in the current listing with a
  snapshot row for the directory - the conflict is decided like file rule 4,
  with the live subtree's file evidence standing in for the file's mod_time:
  - The deletion estimate is the absent peer's row's `deleted_time` if set,
    else its `last_seen` (rule 4b's estimate). If several peers vote
    deletion, use the most recent estimate among them.
  - The survival evidence is the newest mod_time among all live files
    anywhere in the directory's subtree, across the peers that have the
    directory live (gathered by recursively listing that subtree).
    Directories inside contribute no evidence of their own; a subtree
    containing no files provides no evidence.
  - If the deletion estimate exceeds the survival evidence by more than the
    5-second tolerance - or there is no survival evidence at all - the
    deletion is the latest user event and wins: displace the directory to
    BAK/ on every active peer that has it, do not recreate it anywhere,
    cascade tombstones per Snapshot Updates, and do not recurse.
  - Otherwise the directory survives: create it on peers that lack it and
    recurse. Content newer than the deletion estimate is preserved and
    propagates by the file rules; content older is still removed entry by
    entry via file rules 4/4b during recursion. A directory that survives
    only because of newer content therefore converges to holding just that
    newer content.
  - If the survival-evidence listing fails on a peer after the normal
    `--retries-list` tries, skip decisions for this directory and its subtree
    for all peers this run (the same conservative outcome as a canon listing
    failure): with incomplete evidence, neither deletion nor recreation is
    safe.
- If no contributing peer has the directory live in its listing, at least one contributing peer has a snapshot row for the directory, and every contributing peer with a snapshot row for the directory is absent in the current listing, delete it on all remaining peers (displace to BAK/). A row with `deleted_time IS NOT NULL` is already a recorded deletion. A row with `deleted_time = NULL` becomes a confirmed absence for this run and is tombstoned using the normal snapshot update rule.
- A contributing peer with no snapshot row for the directory has no opinion: it neither votes deletion nor blocks one - consistent with the file decision rules where no-row peers do not vote. A peer with the directory live votes for existence regardless of rows.
- If no contributing peer has the directory - neither live in its listing nor as a snapshot row (with or without tombstone) - the directory does not exist in the group's view. Subordinate peers that have it are displaced to BAK/.

Directories are displaced to BAK/ just like files. The snapshot still tracks directories (with `byte_size = -1`) for deletion detection via tombstones, but `mod_time` for directory rows is informational only - it is recorded but not used in decisions.

## Type Conflicts

When the same path is a file on one peer and a directory on another: if a canon peer is present, the canon peer's state wins unconditionally. Canon has a file -> displace directories and sync the file everywhere. Canon has a directory -> displace files and create/sync the directory everywhere. Canon lacks the path -> displace the path everywhere else.

Without a canon peer, type-conflict decisions are based on contributing peers only. If at least one contributing peer has a file and at least one contributing peer has a directory at the same path, the file wins. The directory is displaced to BAK/ on the contributing peer(s) that have it, then the winning file is selected by the normal decision rules (rules 1-6) applied to the contributing file entries only and synced to all active peers. A subordinate peer's file does not make the file win over a contributing peer's directory; after the contributing decision is made, any subordinate path with the wrong type is displaced to BAK/ and replaced as needed.

## Snapshot Updates

Per-peer snapshot rows are updated during traversal, but the update timing
depends on whether the filesystem state has already been confirmed:

- Listed state may be recorded immediately because the entry has already been
  observed on that peer.
- Queued file-copy destinations may be recorded as intended state before the
  copy runs, but their `last_seen` is not updated until the copy succeeds.
- Inline filesystem operations such as directory creation and displacement to
  BAK/ update the affected peer's snapshot only after the operation succeeds.
  If the operation fails, the existing snapshot row remains unchanged for that
  peer.

- **Entry confirmed present** on a peer: upsert row with current mod_time, byte_size, `last_seen` set to the current sync timestamp, and `deleted_time = NULL`
- **Entry confirmed absent** on a peer with an existing row where `deleted_time` is NULL: set `deleted_time` to the row's current `last_seen` value (the deletion happened sometime after that point). Do not update `last_seen`.
- **Entry confirmed absent** on a peer with an existing row where `deleted_time` is already set: no change (tombstone already recorded)
- **Decision: push to a peer**: upsert row for the destination peer with the winning entry's mod_time, byte_size, and `deleted_time = NULL`. Do **not** update `last_seen` - it is only set when the entry is confirmed present (in a listing or after a completed copy). If no row exists yet, `last_seen` is NULL.
- **Copy completed**: after a file copy finishes successfully, set `last_seen` to the current sync timestamp on the destination peer's snapshot row. This is the only post-traversal snapshot update.
- **Inline directory creation completed**: after `create_dir` succeeds on a destination peer, set `last_seen` to the current sync timestamp on that peer's snapshot row. Directory creation is both decided and confirmed in one step (unlike file copies, which are enqueued).
- **Displacement completed**: after the entry is successfully moved to BAK/, set `deleted_time` to the row's current `last_seen` on the row for that peer. Then cascade to descendants using a single recursive CTE scoped to the displaced entry's subtree. The cascade runs against **that same peer's** snapshot.db (each peer has its own); if multiple peers are losing the same subtree in the same decision, the cascade runs once per peer after that peer's displacement succeeds, each against its own snapshot.db, and never against another peer's database. The CTE:
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
  This walks down from the displaced directory's `id` through `parent_id` links, marking only its descendants - not unrelated rows that happen to have a tombstoned parent. If the cascade cannot reach all descendants due to purged intermediate rows, those orphaned rows will be cleaned up by opportunistic snapshot maintenance when their `last_seen` exceeds `--keep-del-days` days.

If the app exits before copies finish, the destination row has `deleted_time = NULL` and `last_seen` unchanged (NULL for first-time targets, or the old confirmation time). The next run sees the entry as absent-unconfirmed and applies rule 4b: since `last_seen` is NULL or old, it does not exceed the source's mod_time, so the copy is re-enqueued.

## Offline Peers

Unreachable peers are excluded entirely - they do not participate in listings or decisions. Their snapshot rows are not modified (`last_seen` is not updated). On the next run when they're reachable, discrepancies between their filesystem state and their snapshot rows drive sync decisions, bringing them up to date.
