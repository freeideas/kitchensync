# 02_snapshot-download: Snapshot download at startup

## Behavior

After connections are established, each reachable peer's `.kitchensync/snapshot.db` is downloaded to a local temp directory. Missing snapshots are created locally; download failures other than not-found promote the peer to unreachable. Derived from `specs/sync.md` §"Startup" step 5 and `specs/database.md`.

## $REQ_IDs
- `02.10` — Each reachable peer's `{peer-root}/.kitchensync/snapshot.db` is downloaded to a local temp directory before the tree walk begins.
- `02.11` — If a peer has no existing `snapshot.db` (transport returns not-found), a new empty database is created locally for that peer.
- `02.12` — If snapshot download fails for any reason other than not-found (I/O error, permission denied), that peer is treated as unreachable and the reachability and canon checks are re-evaluated.

## Notes
The first-sync canon-required check and the post-subordination contributing-peer check are in `02_startup-connect.md`. Auto-subordination of snapshotless peers is in `03_subordinate-peer.md`. End-of-run snapshot upload is in `02_run-completion.md`; upload-failure handling is in `04_error-handling.md`.
