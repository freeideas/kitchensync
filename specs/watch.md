# Watch Mode

When `--watch` is specified, KitchenSync performs an initial sync, then continues running and monitors local filesystems for changes. Each detected change triggers a targeted sync for that entry using the normal decision algorithm.

## Startup Sequence

1. Parse args and connect to all peers (normal startup)
2. Register filesystem watchers on every `file://` peer's root directory (recursive). If the OS rejects a watch (permissions, unsupported filesystem, too many watches), log a warning and continue. If no watches succeed, print an error and exit 1
3. Watcher events begin queuing immediately
4. Run the normal initial sync (full tree walk, decisions, copies)
5. After the initial sync completes, begin processing queued events

With a single peer, `--watch` keeps the snapshot continuously up to date but performs no syncing. Log a warning at startup: `--watch with one peer: snapshot only`.

## Event Processing

OS events are queued immediately (after filtering `.kitchensync/` paths and in-flight self-writes). A worker drains the queue and processes each event:

```python
def handle_watch_event(event_path):
    rel_path = relative_to_peer_root(event_path)

    # Check current state against snapshot -- this is the debounce
    current_stat = stat(event_path)  # may be absent (deletion)
    snap_row = snapshot_lookup(watching_peer, rel_path)

    # If the file matches its snapshot, nothing to do (handles rapid-fire
    # events naturally: the first event syncs, the rest are no-ops)
    if current_stat and snap_row and times_match(current_stat.mod_time, snap_row.mod_time):
        return

    # Run the normal decision algorithm for this single entry
    # Gather live state from all watched local peers
    # Use snapshot state for unwatched/SFTP peers
    states = gather_states_for_entry(all_peers, rel_path)
    decision = decide(states, snapshots)

    # Execute the decision (copies, displacements, snapshot updates)
    # Log with "W" prefix: "W C <path>" for copies, "W X <path>" for deletions
    execute_decision(decision, rel_path)
```

**Important: watch event handling must NOT call the full-tree `SyncDirectory` function.** The initial sync uses `SyncDirectory` with its own copy queue (a channel that is closed after the initial sync completes). If watch events reuse that function, they will send to the closed channel and panic. Instead, watch events must use their own per-entry decision and execution path. This may share the decision logic, but the copy execution must use a separate mechanism (e.g., a long-lived copy queue for the watch session, or synchronous per-event copies).

## Self-Triggered Event Suppression

KitchenSync modifies local peer filesystems during copies and displacements. These changes would trigger watcher events that must not be re-processed.

Maintain a process-global set of in-flight paths. Before any write to a watched peer (file copy arrival, displacement, directory creation/deletion), add the path to the set. Remove it after the operation completes.

Watcher events for paths in the in-flight set are dropped before queuing. After removal from the set, subsequent events for that path are queued normally -- if the file's mod_time matches the snapshot, the snapshot comparison catches it.

## Gathering State for Watched Events

For a single-entry decision triggered by a watch event:

- **Watched local peers**: `stat()` the entry to get live state (present with mod_time/size, or absent)
- **Unwatched peers (SFTP, failed-watch local)**: use the snapshot row as the peer's state. The snapshot reflects what was true at last sync. This is the same data the normal algorithm would use if the peer were offline

This avoids listing remote directories on every local change. The tradeoff: changes made directly on SFTP peers between syncs are not detected until the next full sync. This is expected -- `--watch` monitors local filesystems, not remote ones.

## Debouncing

File change events often arrive in bursts (editor save, build tools). Debouncing is handled implicitly by the snapshot comparison: when rapid events fire for the same path, the first event to be processed syncs the file and updates the snapshot. Subsequent queued events for that path find the file already matches its snapshot and are skipped as no-ops.

No per-path timers are needed. The snapshot is the source of truth.

Events on different paths are independent and may be processed concurrently. Concurrent event processing may produce redundant decisions (e.g., two events both decide to push the same file). This is safe -- copies are idempotent, and snapshot writes are serialized by SQLite. No application-level locking is needed beyond what SQLite provides.

## Ignore Rules

Watch events are filtered through the same `.syncignore` rules as the normal walk. Built-in excludes (`.kitchensync/`, symlinks, special files) also apply. Events inside `.kitchensync/` directories are always suppressed.

## Snapshot Checkpoints

The `--si` checkpoint interval applies during watch mode. Snapshots are uploaded periodically as changes accumulate, protecting against connection loss during long watch sessions.

## Interaction with Other Flags

- `--dry-run` with `--watch`: performs the initial sync in dry-run mode, then watches and logs what *would* happen for each change without executing. Useful for previewing watch behavior
- `--watch` with canon (`+`): works normally. If the canon peer is a watched local peer, its changes always win. If the canon peer is remote, local changes are still pushed (canon's snapshot state shows what it has)
- `--watch` with subordinate (`-`): subordinate local peers are watched but their changes trigger decisions where they don't vote -- effectively, external changes to a subordinate are overwritten by the group's state

## Shutdown

The process runs until interrupted (Ctrl+C / SIGINT / SIGTERM / `POST /shutdown`). On shutdown:

1. Stop accepting new watcher events
2. Wait for in-progress copies to complete (up to 30 seconds; abort remaining after timeout)
3. Upload final snapshots to all peers
4. Exit 0

## Logging

Watch-triggered syncs are logged at `info` level with a `W` prefix to distinguish from initial-sync operations. The action letter (`C` for copy, `X` for deletion/displacement) comes from the sync decision -- not from the filesystem event type. Log after the decision is made, not before:

```
W C photos/vacation/img001.jpg
W X documents/draft.txt
```

Watcher registration is logged at `info`:

```
watching file:///c:/photos
watching file:///d:/backup/photos
```

Failed watches are logged at `warn`:

```
watch failed: /mnt/nfs/share (filesystem does not support watching)
```
