# Startup and Sync Flow

End-to-end flow for a KitchenSync run: parse arguments, connect, lock, sync, upload, exit.

## Startup

```python
def startup(args):
    options, peers = parse_args(args)
    # Validation: at least 1 peer, at most 1 canon (+), all option values valid
    # Option values --mc, --ct, --si are positive integers (>= 1)
    # Option values --xd, --bd, --td are non-negative integers (>= 0); 0 means never
    # -vl is one of: error, warn, info, debug, trace
    # --dry-run / -n is a boolean flag (no value)
    # --watch is a boolean flag (no value)
    # On any validation error: print error + help text to stdout, exit 1
    # No args / -h / --help / /? : print help to stdout, exit 0

    # Connect to all peers in parallel
    for peer in peers:
        for url in peer.urls:          # fallback URLs tried in order
            conn = try_connect(url)    # SFTP: use OS hostname resolution, not numeric-only parsing
            if conn:
                peer.active_url = url
                break
        if not peer.active_url:
            log(warn, f"peer unreachable: {peer}")
            peer.reachable = False

    # Auto-create peer root dirs (both file:// and sftp://) on connect
    # If root creation fails for a file:// URL without fallbacks, log error and mark peer unreachable

    # Single-peer mode: the normal algorithm works -- decisions are trivially
    # no-ops (zero targets), but snapshot updates fire correctly (present files
    # get last_seen=now, absent files get tombstoned). No special case needed.

    if len(reachable) == 0:
        exit(1, "no peers reachable")
    if canon_peer and not canon_peer.reachable:
        exit(1, "canon peer unreachable")
    if len(reachable) == 1 and len(peers) >= 2:
        log(warn, "only one peer reachable -- running in snapshot-only mode")
        # Proceed: the single reachable peer gets a snapshot update, no sync decisions

    # Instance lock check (see instance-lock component spec)
    # 1. Read .kitchensync/lock from each reachable peer (via established connections)
    # 2. Check for overlapping instances (POST to lock port, compare peer lists)
    # 3. Bind lock listener on 127.0.0.1:0
    # 4. Write lock port to each reachable peer's .kitchensync/lock
    # 5. Re-read and verify (race condition mitigation)

    # Download snapshots
    for peer in reachable_peers:
        download peer's .kitchensync/snapshot.db to local temp dir
        if no snapshot.db on peer:
            create empty snapshot locally
            if not peer.is_canon:
                peer.auto_subordinate = True   # no snapshot = auto subordinate (canon is exempt)
        if snapshot download fails (corrupt, permission denied, I/O error):
            log(warn, "snapshot download failed for {peer}, treating as new peer")
            create empty snapshot locally
            if not peer.is_canon:
                peer.auto_subordinate = True

    # A peer is_subordinate if it has the `-` prefix OR has auto_subordinate=True.
    # is_subordinate = peer.explicit_subordinate or peer.auto_subordinate

    if len(peers) >= 2 and no peer has snapshot rows (excluding the sentinel) and no canon peer:
        exit(1, "First sync? Mark the authoritative peer with a leading +")
    if len(peers) >= 2 and no contributing peer reachable:
        exit(1, "No contributing peer reachable -- cannot make sync decisions")

    # Purge old tombstones (skip entirely when --td is 0)
    if options.td > 0:
        for each peer's snapshot:
            delete rows where deleted_time IS NOT NULL and deleted_time older than --td days
            # Also purge stale non-tombstone rows that haven't been seen in --td days
            # (but only when last_seen is set -- rows with last_seen=NULL are pending copies)
            delete rows where deleted_time IS NULL and last_seen IS NOT NULL and last_seen older than --td days

    # Run the walk
    sync_directory(reachable_peers, root_path)

    # Wait for all enqueued file copies to complete (up to 60 seconds;
    # abort remaining after timeout)
    wait(copy_queue, timeout=60s)

    # Upload final snapshots (via TMP staging + atomic rename)
    # WAL checkpoint before reading the .db file (see database spec):
    # PRAGMA wal_checkpoint(TRUNCATE)
    for peer in reachable_peers:
        upload snapshot to .kitchensync/TMP/<timestamp>/<uuid>/snapshot.db
        rename to .kitchensync/snapshot.db

    log(info, "done")
    exit(0)
```

## Snapshot Checkpoints

During long syncs, snapshots are periodically uploaded to peers so that progress is preserved if the connection drops. The interval is controlled by `--si` (default: 30 minutes).

A process-global timer tracks elapsed time since the last snapshot upload (or since sync start). After each completed file copy, if the timer has exceeded `--si` minutes, upload all peers' snapshots using the same TMP staging + atomic rename as the final upload (including WAL checkpoint before reading the `.db` file). The upload uses each peer's listing connection (not the transfer pool). Reset the timer after each checkpoint.

This is safe because the snapshot always reflects decided state. Pending copies have `last_seen=NULL`, so rule 4b (absent-unconfirmed) re-enqueues them on the next run. A checkpoint snapshot is always in a valid recovery state.

Checkpoints are skipped in dry-run mode (no mutations).

## Offline Peers

Unreachable peers are excluded entirely -- they do not participate in listings or decisions. Their snapshot rows are not modified. On the next run, discrepancies between filesystem state and snapshot drive sync decisions, bringing them up to date. Failure to connect to one peer is non-fatal -- exit 0 if at least one sync completes, or if single-peer snapshot completes successfully.
