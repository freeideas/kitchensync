# 006_snapshot-file-lifecycle: Snapshot database download, recovery, and upload

## Behavior
This concern derives from `specs/database.md` opening section,
`specs/sync.md` sections "Startup", "Run", and "Rename Compatibility", and
`plan/sqlite-snapshot.md`. It covers the peer-side
`.kitchensync/snapshot.db` file, rollback-journal single-file storage, startup
snapshot SWAP recovery, local temporary snapshot copies, creation of a new
local snapshot when none exists, normal-run upload through SWAP staging, upload
failure states, and the requirement that database work is complete and closed
before transport upload reads `snapshot.db`.

## $REQ_IDs

- `006.1` -- For each peer, KitchenSync stores the peer-side snapshot database at `.kitchensync/snapshot.db` under that peer's root.
- `006.2` -- Each snapshot database that KitchenSync creates or updates uses SQLite rollback-journal mode.
- `006.3` -- KitchenSync uploads only `snapshot.db` as peer snapshot database state.
- `006.4` -- KitchenSync does not upload SQLite sidecar files as peer snapshot database state.
- `006.5` -- During normal startup, KitchenSync recovers incomplete `.kitchensync/SWAP/snapshot.db/` state before downloading that peer's snapshot database.
- `006.6` -- During snapshot SWAP recovery, if SWAP `old` and live `snapshot.db` both exist, KitchenSync leaves live `snapshot.db` in place.
- `006.7` -- During snapshot SWAP recovery, if SWAP `old` and live `snapshot.db` both exist, KitchenSync removes SWAP `old`.
- `006.8` -- During snapshot SWAP recovery, if SWAP `old`, SWAP `new`, and live `snapshot.db` all exist, KitchenSync removes SWAP `new`.
- `006.9` -- During snapshot SWAP recovery, if SWAP `old` and SWAP `new` both exist and live `snapshot.db` is missing, KitchenSync makes SWAP `new` the live `snapshot.db`.
- `006.10` -- During snapshot SWAP recovery, if SWAP `old` and SWAP `new` both exist and live `snapshot.db` is missing, KitchenSync removes SWAP `old`.
- `006.11` -- During snapshot SWAP recovery, if SWAP `old` exists while SWAP `new` and live `snapshot.db` are both missing, KitchenSync makes SWAP `old` the live `snapshot.db`.
- `006.12` -- During snapshot SWAP recovery, if SWAP `new` and live `snapshot.db` both exist while SWAP `old` is missing, KitchenSync leaves live `snapshot.db` in place.
- `006.13` -- During snapshot SWAP recovery, if SWAP `new` and live `snapshot.db` both exist while SWAP `old` is missing, KitchenSync removes SWAP `new`.
- `006.14` -- During snapshot SWAP recovery, if SWAP `new` exists while SWAP `old` and live `snapshot.db` are both missing, KitchenSync makes SWAP `new` the live `snapshot.db`.
- `006.15` -- During startup, if snapshot SWAP recovery fails for a peer, KitchenSync excludes that peer from the reachable set.
- `006.16` -- During normal startup, KitchenSync downloads each reachable peer's existing `.kitchensync/snapshot.db` to a local temporary `{tmp}/{uuid}/snapshot.db` file.
- `006.17` -- During normal startup, if a reachable peer's `.kitchensync/snapshot.db` is not found, KitchenSync creates a new empty local snapshot database for that peer.
- `006.18` -- During startup, if snapshot download fails for a peer with any error other than not found, KitchenSync excludes that peer from the reachable set.
- `006.19` -- During a run, KitchenSync reads snapshot data through the peer's local temporary `snapshot.db` copy.
- `006.20` -- During a run, KitchenSync writes snapshot data through the peer's local temporary `snapshot.db` copy.
- `006.21` -- In a normal run, KitchenSync waits until all enqueued file copies have completed before starting snapshot uploads.
- `006.22` -- In a normal run, KitchenSync uploads each peer's updated local temporary snapshot database back to that peer.
- `006.23` -- For a normal snapshot upload, KitchenSync writes the replacement database to `.kitchensync/SWAP/snapshot.db/new`.
- `006.24` -- For a normal snapshot upload, KitchenSync closes `.kitchensync/SWAP/snapshot.db/new` before replacing the live snapshot.
- `006.25` -- For a normal snapshot upload when a live `.kitchensync/snapshot.db` exists, KitchenSync moves the live snapshot to `.kitchensync/SWAP/snapshot.db/old` before moving SWAP `new` into place.
- `006.26` -- For a normal snapshot upload, KitchenSync moves `.kitchensync/SWAP/snapshot.db/new` into `.kitchensync/snapshot.db`.
- `006.27` -- After a normal snapshot upload has moved SWAP `new` into live `.kitchensync/snapshot.db`, KitchenSync removes `.kitchensync/SWAP/snapshot.db/old`.
- `006.28` -- Normal snapshot upload replaces an existing peer snapshot on transports whose `rename(src, dst)` rejects an existing destination.
- `006.29` -- If a normal snapshot upload fails before `.kitchensync/SWAP/snapshot.db/old` exists, the existing live `.kitchensync/snapshot.db` remains in place.
- `006.30` -- If a normal snapshot upload fails before `.kitchensync/SWAP/snapshot.db/old` exists, any retained SWAP `new` is handled by startup recovery on the next normal run.
- `006.31` -- If a normal snapshot upload fails after `.kitchensync/SWAP/snapshot.db/old` exists, the peer retains snapshot SWAP state for the next normal startup recovery.
- `006.32` -- Before uploading a local temporary `snapshot.db` to a peer, KitchenSync commits or rolls back every transaction against that local file.
- `006.33` -- Before uploading a local temporary `snapshot.db` to a peer, KitchenSync finalizes every statement against that local file.
- `006.34` -- Before uploading a local temporary `snapshot.db` to a peer, KitchenSync finalizes every cursor against that local file.
- `006.35` -- Before uploading a local temporary `snapshot.db` to a peer, KitchenSync finalizes every reader against that local file.
- `006.36` -- Before transport upload reads a local temporary `snapshot.db`, KitchenSync closes every SQLite connection to that local file.
- `006.37` -- Transport upload reads the closed local temporary `snapshot.db` file rather than a live SQLite connection.
- `006.38` -- The uploaded peer-side `.kitchensync/snapshot.db` is usable as a self-contained SQLite database without SQLite sidecar files.
- `006.39` -- When overlapping normal runs upload snapshots to the same peer, the peer-side `.kitchensync/snapshot.db` reflects the last completed snapshot upload.

## Notes
This file covers whole-file snapshot handling. The table schema belongs to
`007_snapshot-schema.md`; row updates during sync belong to
`015_snapshot-row-updates-and-cleanup.md`. Dry-run-specific peer write
prohibitions belong to `018_dry-run.md`. Peer-role effects of a missing
startup snapshot belong to `012_peer-roles.md`.
