# Multi-Tree Synchronization

## Overview

Synchronizes N file trees in a single recursive traversal. At each directory level: list all peers in parallel, union the entries, decide the authoritative state for each, act, and recurse.

## Algorithm

```
function sync_directory(peers, path, snapshot):
    // Phase 1: List all peers in parallel
    listings = parallel_for_each(peers):
        list_directory(peer, path)  // returns entries or empty on error

    // Phase 2: Union entry names (peers + snapshot children)
    all_names = union(listings.keys(), snapshot_children(path))

    // Phase 3: Decide and act on each entry
    for name in all_names:
        states = gather_states(peers, listings, name)
        snap = snapshot_lookup(path/name)
        decision = decide(states, snap)

        if decision.type == directory:
            for peer in peers:
                if peer needs dir created:  create_dir(peer, path/name)
                if peer needs dir deleted:  displace_to_back(peer, path/name)
            update_snapshot(path/name, decision)
            sync_directory(peers, path/name, snapshot)  // recurse

        if decision.type == file:
            update_snapshot(path/name, decision)
            for each dst_peer that needs the file:
                enqueue_copy(decision.src_peer, path/name, dst_peer, path/name)
            for each peer where file should be deleted:
                displace_to_back(peer, path/name)
```

## Built-in Excludes

Always excluded from listings (never synced):

- `.kitchensync/` directories — sync metadata must not sync
- Symbolic links (files and directories) — following symlinks could escape the sync root or create loops
- Special files (devices, FIFOs, sockets)
- `.git/` directories

## Entry Classification

For each entry, compare each peer's state to the snapshot:

| Peer State               | Snapshot          | Classification |
| ------------------------ | ----------------- | -------------- |
| Live, same mod_time      | Live              | Unchanged      |
| Live, different mod_time | Live              | Modified       |
| Live                     | Absent or Deleted | New            |
| Absent                   | Live              | Deleted        |
| Absent                   | Absent or Deleted | —              |

## Decision Rules

### With `--canon <peer>`

The canonical peer's state wins unconditionally:
- Canon has file → push to all others
- Canon lacks file → delete everywhere else
- Canon is unreachable → exit with error at startup

### Without `--canon`

1. **All unchanged** → no action
2. **Modified** → newest mod_time wins; push to all that don't match
3. **New** → newest mod_time wins; push to all that lack it
4. **Deleted + unchanged** → deletion wins (it's more recent than the last sync; the unchanged copies haven't been touched)
5. **Deleted + modified** → modification wins (active edit beats deletion)
6. **Same mod_time, different size** → larger file wins
7. **Ties** → keep data (existence over deletion, larger over smaller)

Timestamp tolerance: 5 seconds in either direction. The tolerance applies to Entry Classification: a peer's mod_time is considered "same" as the snapshot if it differs by ≤ 5 seconds. When comparing two peers' mod_times in Decision Rules, timestamps within 5 seconds of each other are treated as equal (fall through to rule 6/7).

## Orphaned Tombstones

If an entry (file or directory) is absent on all reachable peers and exists only as a tombstone in the snapshot, the tombstone is removed.

## Directory Decisions

Directories use the same entry classification and decision rules as files (mod_time comparison, newest wins, timestamp tolerance). The same rules (1–7) apply. Directories are displaced to BACK/ just like files.

## Type Conflicts

When the same path is a file on one peer and a directory on another, the standard decision rules apply. Since directories have byte_size −1, files win when mod_times are within tolerance (rule 6). The losing entry is displaced to BACK/ on the peer(s) that have it, then the winning entry is synced to all peers.

## Snapshot Updates

The snapshot is updated during traversal to reflect decisions, before file copies complete. If the app exits before copies finish, the next run detects discrepancies between snapshot and peer state and re-enqueues them.

## Offline Peers

Unreachable peers are simply not listed. No decisions are made about their files. On the next run when they're reachable, the snapshot reveals discrepancies and they're brought up to date.
