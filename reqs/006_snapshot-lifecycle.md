# 006_snapshot-lifecycle: Snapshot download, upload, and recovery

## Behavior
This concern derives from `specs/database.md` section "Database" and snapshot SWAP recovery rules, plus `specs/sync.md` sections "Startup", "Run", "Rename Compatibility", and "Errors". It covers startup recovery of `.kitchensync/SWAP/snapshot.db/`, downloading peer snapshots into local temporary databases, creating empty local snapshots for new peers, dry-run snapshot handling, upload through SWAP staging, upload failure behavior, and the observable correctness rule for overlapping runs.

## $REQ_IDs
- `006.1` -- In a normal run, KitchenSync recovers incomplete peer `.kitchensync/SWAP/snapshot.db/` state before downloading that peer's snapshot.
- `006.2` -- In a normal run, KitchenSync completes peer `.kitchensync/SWAP/snapshot.db/` recovery before deciding whether that peer has snapshot history.
- `006.3` -- In `--dry-run`, KitchenSync skips peer-side `.kitchensync/SWAP/snapshot.db/` recovery.
- `006.4` -- In `--dry-run`, KitchenSync downloads the peer's live `.kitchensync/snapshot.db` as it exists at startup.
- `006.5` -- When snapshot SWAP recovery finds `old` and `snapshot.db` present, it deletes `new` if `new` is present.
- `006.6` -- When snapshot SWAP recovery finds `old` and `snapshot.db` present, it deletes `old`.
- `006.7` -- When snapshot SWAP recovery finds `old` and `new` present and `snapshot.db` missing, it renames `new` to `snapshot.db`.
- `006.8` -- When snapshot SWAP recovery finds `old` and `new` present and `snapshot.db` missing, it deletes `old` after restoring `snapshot.db`.
- `006.9` -- When snapshot SWAP recovery finds `old` present and both `new` and `snapshot.db` missing, it renames `old` to `snapshot.db`.
- `006.10` -- When snapshot SWAP recovery finds `new` and `snapshot.db` present and `old` missing, it deletes `new`.
- `006.11` -- When snapshot SWAP recovery finds `new` present and both `old` and `snapshot.db` missing, it renames `new` to `snapshot.db`.
- `006.12` -- For each peer with an existing live snapshot, KitchenSync downloads `.kitchensync/snapshot.db` to a local temporary `{tmp}/{uuid}/snapshot.db`.
- `006.13` -- If a peer has no existing `.kitchensync/snapshot.db`, KitchenSync creates a new empty snapshot database locally for that peer.
- `006.14` -- If snapshot SWAP recovery or snapshot download for a peer fails with an error other than `not found`, KitchenSync logs an error-level diagnostic for that peer.
- `006.15` -- If snapshot SWAP recovery or snapshot download for a peer fails with an error other than `not found`, KitchenSync excludes that peer from sync decisions and peer updates for that run.
- `006.16` -- If snapshot SWAP recovery or snapshot download failures leave fewer than two reachable peers, KitchenSync exits 1.
- `006.17` -- If snapshot SWAP recovery or snapshot download failure excludes the canon peer, KitchenSync exits 1.
- `006.18` -- During a run, KitchenSync reads each peer's snapshot state from that peer's local temporary snapshot database.
- `006.19` -- During a run, KitchenSync writes each peer's snapshot updates to that peer's local temporary snapshot database.
- `006.20` -- In a normal run, KitchenSync waits for all enqueued file copies to complete before uploading updated snapshots to peers.
- `006.21` -- In a normal run, KitchenSync uploads updated snapshot databases back to peers through `.kitchensync/SWAP/snapshot.db/` staging.
- `006.22` -- In `--dry-run`, KitchenSync does not upload updated local temporary snapshot databases back to peers.
- `006.23` -- Snapshot upload writes and closes the replacement database at `.kitchensync/SWAP/snapshot.db/new` before replacing the live `.kitchensync/snapshot.db`.
- `006.24` -- Snapshot upload renames an existing live `.kitchensync/snapshot.db` to `.kitchensync/SWAP/snapshot.db/old` before making `new` the live snapshot.
- `006.25` -- Snapshot upload renames `.kitchensync/SWAP/snapshot.db/new` to the live `.kitchensync/snapshot.db`.
- `006.26` -- Snapshot upload deletes `.kitchensync/SWAP/snapshot.db/old` after the replacement database is live.
- `006.27` -- Snapshot upload succeeds on transports whose `rename(src, dst)` rejects an existing `dst` when ordinary create, write, delete, and rename-to-new-path operations succeed.
- `006.28` -- If snapshot upload fails before `.kitchensync/SWAP/snapshot.db/old` exists, KitchenSync logs an error-level diagnostic.
- `006.29` -- If snapshot upload fails before `.kitchensync/SWAP/snapshot.db/old` exists, any pre-existing live `.kitchensync/snapshot.db` remains in place.
- `006.30` -- If snapshot upload fails after `.kitchensync/SWAP/snapshot.db/old` exists, KitchenSync logs an error-level diagnostic.
- `006.31` -- If snapshot upload fails after `.kitchensync/SWAP/snapshot.db/old` exists, KitchenSync leaves the snapshot SWAP state in place for startup recovery.
- `006.32` -- KitchenSync allows overlapping runs against the same peers without rejecting a run solely because another run is active.
- `006.33` -- When overlapping runs upload snapshots to the same peer, the peer's final live `.kitchensync/snapshot.db` is the snapshot uploaded by whichever run uploads last.
- `006.34` -- After overlapping runs leave one run's decisions absent from the final live snapshot, a later normal run rediscovers the missing decisions and preserves sync correctness for the current peer contents.
- `006.35` -- KitchenSync syncs peer snapshot state using `.kitchensync/snapshot.db` without syncing SQLite sidecar files.

## Notes
This category owns moving snapshot database files between peers and local working state. It does not own the schema contents or per-entry update rules.
