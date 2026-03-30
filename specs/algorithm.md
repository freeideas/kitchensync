# Sync Algorithm

This is the complete sync algorithm. It covers startup, the combined-tree walk, decision rules, snapshot updates, and the operation queue.

## Startup

```python
def startup(args):
    options, peers = parse_args(args)
    # Validation: at least 1 peer, at most 1 canon (+), all option values valid
    # Option values --mc, --ct, --xd, --bd, --td are positive integers (>= 1)
    # --td 0 is a special case: purge all tombstones immediately
    # -vl is one of: error, info, debug, trace
    # On any validation error: print error + help text to stdout, exit 1
    # No args / -h / --help / /? : print help to stdout, exit 0

    # Connect to all peers in parallel
    for peer in peers:
        for url in peer.urls:          # fallback URLs tried in order
            conn = try_connect(url)    # SFTP: use OS hostname resolution (net.Dial), not numeric-only parsing
            if conn:
                peer.active_url = url
                break
        if not peer.active_url:
            log(warn, f"peer unreachable: {peer}")
            peer.reachable = False

    # Auto-create peer root dirs (both file:// and sftp://) on connect

    if canon_peer and not canon_peer.reachable:
        exit(1, "canon peer unreachable")

    # Single-peer mode: the normal algorithm works — decisions are trivially
    # no-ops (zero targets), but snapshot updates fire correctly (present files
    # get last_seen=now, absent files get tombstoned). No special case needed.

    if len(reachable) < 2 and len(peers) >= 2:
        exit(1, "fewer than two peers reachable")
    if len(reachable) < 1:
        exit(1, "peer unreachable")

    # Download snapshots
    for peer in reachable_peers:
        download peer's .kitchensync/snapshot.db to local temp dir
        if no snapshot.db on peer:
            create empty snapshot locally
            peer.auto_subordinate = True   # no snapshot = auto subordinate

    if len(peers) >= 2 and no peer has snapshot data and no canon peer:
        exit(1, "First sync? Mark the authoritative peer with a leading +")
    if len(peers) >= 2 and no contributing peer reachable:
        exit(1, "No contributing peer reachable — cannot make sync decisions")

    # Purge old tombstones
    for each peer's snapshot:
        delete rows where deleted_time IS NOT NULL and deleted_time older than --td days
        delete rows where deleted_time IS NULL and (last_seen older than --td days or last_seen IS NULL)

    # Run the walk
    sync_directory(reachable_peers, root_path)

    # Wait for all enqueued file copies to complete
    wait(copy_queue)

    # Upload updated snapshots back (via TMP staging + atomic rename)
    for peer in reachable_peers:
        upload snapshot to .kitchensync/TMP/<timestamp>/<uuid>/snapshot.db
        rename to .kitchensync/snapshot.db

    log(info, "done")
    exit(0)
```

## Combined-Tree Walk

The traversal is **pre-order**: every entry in a directory is decided and acted on before recursing into any subdirectory. A directory marked for displacement is renamed (with its entire subtree) before its children are ever visited. There is no separate "file pass" or "directory pass" — each entry is fully handled before moving to the next.

```python
def sync_directory(peers, path, parent_ignore_rules=None):
    # Phase 1: List all peers in parallel
    listings = {}
    errors = {}
    parallel for peer in peers:
        result = peer.list_dir(path)     # returns {name: Entry} or error
        if error:
            errors[peer] = result
        else:
            listings[peer] = result

    # Drop peers with listing errors (excluded from this entire subtree)
    for peer in errors:
        log(error, f"listing failed for {peer} at {path}, excluding from subtree")
    active = [p for p in peers if p not in errors]

    # Phase 2: Union entry names
    contributing = [p for p in active if not p.is_subordinate]
    subordinates = [p for p in active if p.is_subordinate]
    all_names = union(listings[p].keys() for p in contributing)
    all_names |= union(listings[p].keys() for p in subordinates)

    # Phase 2b: Resolve .syncignore FIRST
    ignore_rules = parent_ignore_rules or []
    if ".syncignore" in all_names:
        # Decide winning .syncignore using normal decision rules
        decide_and_act(".syncignore", ...)
        # Read winning version, merge with parent rules
        content = read_file(winning_peer, path / ".syncignore")
        if content:
            ignore_rules = merge(parent_ignore_rules, parse_gitignore(content))
        all_names.remove(".syncignore")

    # Filter by ignore rules
    all_names = {n for n in all_names if not matches(ignore_rules, n)}

    # Phase 3: Decide and act on each entry (pre-order)
    dirs_to_recurse = []  # collect (peers, subpath) pairs

    for name in all_names:
        entry_path = path / name
        states = gather_states(contributing, listings, name)
        snap = snapshot_lookup_per_peer(entry_path)
        decision = decide(states, snap)

        if decision.type == DIRECTORY:
            recursion_peers = []
            for peer in active:
                if peer has wrong type at entry_path:
                    displace(peer, entry_path)           # inline, immediate
                if decision.action == DELETE and peer has dir:
                    displace(peer, entry_path)           # moves entire subtree
                    cascade_tombstones(peer, entry_path) # mark children deleted in snapshot
                elif decision.action == CREATE and peer lacks dir:
                    peer.create_dir(entry_path)          # inline, immediate
                    set last_seen = now on peer's snapshot row
                    recursion_peers.append(peer)
                else:
                    recursion_peers.append(peer)         # peer keeps this dir
            update_snapshot(entry_path, decision)
            if recursion_peers:
                dirs_to_recurse.append((recursion_peers, entry_path))

        elif decision.type == FILE:
            for peer in active:
                if peer has directory at entry_path:
                    displace(peer, entry_path)           # type conflict, inline
            update_snapshot(entry_path, decision)
            for dst_peer that needs the file:
                enqueue_copy(decision.src_peer, entry_path, dst_peer)
            for peer where file should be deleted:
                displace(peer, entry_path)               # inline

    # Phase 4: BAK/TMP cleanup at this level
    for peer in active:
        ks_dir = path / ".kitchensync"
        if peer.exists(ks_dir):
            cleanup_expired(peer, ks_dir / "BAK", max_age=options.bd)
            cleanup_expired(peer, ks_dir / "TMP", max_age=options.xd)

    # Phase 5: Recurse into subdirectories (pre-order: all entries handled above first)
    for recursion_peers, subpath in dirs_to_recurse:
        sync_directory(recursion_peers, subpath, ignore_rules)
```

**Key invariant**: `displace()` is always a same-filesystem rename — it runs inline during the walk, never queued. A displaced directory is moved as a single rename, preserving its entire subtree. Because we handle every entry before recursing, a displaced directory's children are never visited individually.

## Entry Classification

For each **file** entry, compare each contributing peer's filesystem state to that peer's snapshot row:

| Peer State               | Snapshot Row | `deleted_time` | Classification              |
| ------------------------ | ------------ | -------------- | --------------------------- |
| Live, same mod_time      | Exists       | NULL           | Unchanged                   |
| Live, different mod_time | Exists       | NULL           | Modified                    |
| Live                     | Exists       | NOT NULL       | Resurrection (clear tombstone) |
| Live                     | No row       | —              | New                         |
| Absent                   | Exists       | NOT NULL       | Deleted (estimate = `deleted_time`) |
| Absent                   | Exists       | NULL           | Absent-unconfirmed (rule 4b) |
| Absent                   | No row       | —              | No opinion (never existed here) |

"Same mod_time" means within 5-second tolerance.

## Decision Rules

### With a canon peer (`+`)

Canon wins unconditionally:
- Canon has file -> push to all others
- Canon lacks file -> delete everywhere else (displace to BAK/)
- Canon unreachable -> exit at startup (never reaches here)

### Without a canon peer

Only contributing (non-subordinate) peers vote. After the decision, subordinate peers are brought into conformance.

```python
def decide(states, snap):
    # states: {peer: (classification, mod_time, byte_size)} for contributing peers only
    # Peers with no row and absent state have no opinion — skip them

    voters = {p: s for p, s in states.items() if s.classification != NO_OPINION}

    if not voters:
        # No contributing peer has this entry. Subordinates with it get displaced.
        return Decision(action=DELETE_SUBORDINATES_ONLY)

    live = {p: s for p, s in voters.items() if s.is_live}
    deleted = {p: s for p, s in voters.items() if s.classification == DELETED}
    absent_unconfirmed = {p: s for p, s in voters.items() if s.classification == ABSENT_UNCONFIRMED}

    # Rule 1: All unchanged -> no action
    if all(s.classification == UNCHANGED for s in voters.values()):
        return Decision(action=NONE)

    # Handle absent-unconfirmed (rule 4b) before main decision
    for peer, s in absent_unconfirmed.items():
        max_live_mtime = max(s.mod_time for s in live.values()) if live else None
        if max_live_mtime and s.last_seen > max_live_mtime + TOLERANCE:
            # Confirmed deletion: last_seen exceeds all live mod_times by > 5s
            deleted[peer] = s._replace(classification=DELETED, deletion_estimate=s.last_seen)
        else:
            # Failed copy or never received — re-enqueue, no deletion vote
            live[peer] = s  # treat as needing the file

    if live and not deleted:
        # Rules 2/3: Pick winner by mod_time (newest wins)
        max_mtime = max(s.mod_time for s in live.values())
        # Tolerance: anyone within 5s of max is tied with max
        tied = {p: s for p, s in live.items() if max_mtime - s.mod_time <= TOLERANCE}
        if len(tied) > 1:
            # Rule 5: same mod_time, larger file wins
            winner = max(tied.items(), key=lambda ps: ps[1].byte_size)
        else:
            winner = max(live.items(), key=lambda ps: ps[1].mod_time)
        return Decision(action=PUSH, src=winner, targets=peers_that_differ)

    if deleted and not live:
        # Everything deleted -> delete on all peers
        return Decision(action=DELETE)

    if live and deleted:
        # Rule 4: Compare deletion estimate vs existing mod_time
        max_deletion_estimate = max(s.deletion_estimate for s in deleted.values())
        max_live_mtime = max(s.mod_time for s in live.values())
        if max_deletion_estimate > max_live_mtime + TOLERANCE:
            # Deletion is newer -> delete everywhere
            return Decision(action=DELETE)
        else:
            # Rule 6: ties favor existence (mod_time >= deletion estimate)
            # Existing file wins -> push to peers that lack it
            winner = pick_winner_from_live(live)  # by mod_time, then size
            return Decision(action=PUSH, src=winner, targets=peers_that_lack_it)
```

**Timestamp tolerance**: 5 seconds in either direction. Applies to: classification (mod_time vs snapshot), decision comparisons (mod_time vs mod_time, deletion estimate vs mod_time), and rule 4b (last_seen vs max mod_time).

**Skip unnecessary copies**: If the winning entry already exists on a peer with matching mod_time (within tolerance) and matching byte_size, no copy is performed — only the snapshot row is updated.

## Directory Decisions

Directories do not use mod_time for decisions. Directory mod_times are filesystem bookkeeping (they change when children are added/removed) and vary across filesystem types.

Existence-based only:
- Any contributing peer has it -> create on peers that lack it
- All contributing peers deleted it (tombstone + absent) -> delete everywhere (displace to BAK/)
- Canon overrides as usual

## Type Conflicts

Same path is a file on one peer and a directory on another:
- Canon peer present -> canon's type wins
- No canon -> file wins. Directory is displaced to BAK/, then the file is synced normally.

## Snapshot Updates

Updated during traversal, as soon as a decision is made — before file copies execute. The snapshot reflects decided state, not physical state.

```python
# Entry confirmed present on a peer:
upsert(id, parent_id, basename, mod_time, byte_size, last_seen=now, deleted_time=NULL)

# Entry confirmed absent, existing row with deleted_time NULL:
set deleted_time = last_seen    # conservative: deletion happened after last confirmation

# Entry confirmed absent, deleted_time already set:
no change                       # tombstone already recorded

# Decision: push to a peer (copy enqueued):
upsert(id, ..., mod_time=winner.mod_time, byte_size=winner.byte_size, deleted_time=NULL)
# Do NOT set last_seen — only set after copy completes (or after listing confirms presence)

# Copy completed successfully:
set last_seen = now             # the only post-traversal snapshot update

# Directory creation completed (inline):
set last_seen = now             # confirmed in one step

# Decision: delete from a peer:
set deleted_time = last_seen
# Then cascade to all descendants:
```

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

**Crash recovery**: If the app exits before copies finish, destination rows have `deleted_time = NULL` and `last_seen` unchanged (NULL for first-time targets). Next run sees absent-unconfirmed, applies rule 4b: `last_seen` is NULL or old, so the copy is re-enqueued.

## Operation Queue

### File Copy

File copies are enqueued during the walk and executed concurrently (subject to per-peer connection limits). Directory creation and displacement run inline.

Each transfer acquires one connection from the source peer's pool and one from the destination peer's pool before starting.

```python
def copy_file(src_peer, path, dst_peer):
    src_conn = src_peer.pool.acquire()
    dst_conn = dst_peer.pool.acquire()
    try:
        tmp_path = f"{path.parent}/.kitchensync/TMP/{timestamp()}/{uuid()}/{path.name}"

        # Pipelined transfer: reader and writer run concurrently via bounded channel
        ch = make_channel(buffer=N)
        go reader(src_conn, path, ch)       # reads chunks, sends to channel
        go writer(dst_conn, tmp_path, ch)   # receives chunks, writes to disk
        wait(reader, writer)

        # Displace existing file at target (if any) to BAK/
        if dst_peer.exists(path):
            bak_path = f"{path.parent}/.kitchensync/BAK/{timestamp()}/{path.name}"
            dst_conn.rename(path, bak_path)

        # Atomic swap: rename TMP -> final
        dst_conn.rename(tmp_path, path)

        # Set mod_time to the winning mod_time from the decision
        dst_conn.set_mod_time(path, decision.mod_time)

        # Clean up empty TMP dirs
        cleanup_empty_parents(dst_conn, tmp_path)

        # Post-copy snapshot update
        set last_seen = now on dst_peer's snapshot row

    except error:
        # On failure: clean up TMP staging, log, skip (re-discovered next run)
        delete tmp_path if exists
    finally:
        src_peer.pool.release(src_conn)
        dst_peer.pool.release(dst_conn)
```

### Displace to BAK

Each displacement is a `(peer, path)` pair executed inline during the walk:

```python
def displace(peer, path):
    bak_path = f"{path.parent}/.kitchensync/BAK/{timestamp()}/{path.name}"
    peer.rename(path, bak_path)   # single rename, preserves subtree for directories
```

## Logging

**All output goes to stdout.** No output to stderr. No logging frameworks that default to stderr.

Every file copy and every deletion is logged at `info` level:
- Copy: `C <relative-path>`
- Delete: `X <relative-path>`

Logged once per decision, not per peer. Example: `C photos/vacation/img001.jpg`

Connection pool changes logged at `trace` level: `url=sftp://host/path connections=2/10`

## Offline Peers

Unreachable peers are excluded entirely — they do not participate in listings or decisions. Their snapshot rows are not modified. On the next run, discrepancies between filesystem state and snapshot drive sync decisions, bringing them up to date. Failure to connect to one peer is non-fatal — exit 0 if at least one sync completes.

## Subordinate Peers

A subordinate peer (`-` prefix) participates in listing and receives file operations, but does not contribute to decisions:
- Its entries are not included in `gather_states` — decisions are made as if it doesn't exist
- After decisions, it is brought into conformance: unwanted files displaced, missing files copied, directories created/removed
- Its snapshot is still downloaded, updated, and uploaded. On future runs without `-`, it participates normally.

Any peer without a snapshot is automatically subordinate (unless it's the canon peer).

## Errors

- **Argument errors** (no peers, multiple `+`, invalid values) -> print error + help text to stdout, exit 1
- **No snapshots and no canon** (multi-peer mode) -> print suggestion (`+`), exit 1
- **Unreachable peer** -> skip, log warning, continue with others
- **Canon peer unreachable** -> exit 1
- **Fewer than two reachable** -> exit 1
- **Transfer failure** -> log, skip file (re-discovered next run)
- **Displacement failure** -> log error, skip (file remains). If part of a copy sequence, skip the copy too (clean up TMP)
- **TMP staging failure** -> treat as transfer failure
- **Snapshot upload failure** -> log error, leave TMP for `--xd` cleanup

## Peer Filesystem Interface

All sync logic operates through a single interface that both `file://` and `sftp://` implement. No protocol-specific code exists outside the interface implementations.

| Operation                  | Description                                                       |
| -------------------------- | ----------------------------------------------------------------- |
| `ListDir(path)`            | List immediate children (name, isDir, modTime, byteSize). byteSize is file size for files, -1 for directories |
| `Stat(path)`               | Return modTime, byteSize, isDir; or "not found"                  |
| `ReadFile(path)` -> Reader | Open file for streaming read                                      |
| `WriteFile(path, Reader)`  | Create/overwrite file from stream, creating parent dirs as needed |
| `Rename(src, dst)`         | Same-filesystem rename (for TMP -> final swap and BAK displacement) |
| `DeleteFile(path)`         | Remove a file                                                     |
| `CreateDir(path)`          | Create directory (and parents as needed)                          |
| `DeleteDir(path)`          | Remove empty directory                                            |
| `SetModTime(path, time)`   | Set file/directory modification time                              |

`ListDir` returns only regular files and directories. Symbolic links, special files (devices, FIFOs, sockets), and any other non-regular entries are silently omitted. `Stat` returns "not found" for symlinks and special files.

All operations return the same error types regardless of transport: not found, permission denied, I/O error. Network failures surface as I/O errors.

SFTP connections must use OS hostname resolution (e.g., Go's `net.Dial`), not numeric-only socket address parsing. `sftp://user@localhost/path` must work.

## Case Sensitivity

Filenames are preserved exactly as the filesystem reports them. Syncing between case-sensitive (Linux) and case-insensitive (Windows/macOS) filesystems may collapse or duplicate files that differ only in case. Deleted files are recoverable from BAK/.
