# Plan Overview

KitchenSync has external risk points in the Rust crates and host behaviors that
must match the specs before product code depends on them. Each item below is
backed by a runnable experiment under `plan/experiments/`.

- SFTP client behavior is probed in `sftp-client.md`. It verifies the `ssh2`
  calls for host-key checking, password authentication, Ed25519 public-key
  authentication, and the SFTP file operations needed by the transport.
- Snapshot database behavior is probed in `sqlite-snapshot.md`. It verifies
  `rusqlite` with bundled SQLite, rollback-journal mode, the exact snapshot
  table shape, indexes, and the recursive subtree tombstone update.
- Snapshot path IDs are probed in `path-ids.md`. It verifies `xxhash-rust`
  `xxh64` with seed 0 and the 11-character base62 encoding used by
  `snapshot.id` and `snapshot.parent_id`.
- URL parsing and normalization support is probed in `url-normalization.md`.
  It verifies the `url`, `percent-encoding`, and `whoami` calls needed for SFTP
  URLs, default ports, query settings, current-user insertion, file URLs, and
  percent-decoded passwords.
- Local filesystem modification times are probed in `local-file-metadata.md`.
  It verifies `filetime` can set and read file and directory mtimes with
  microsecond precision on this machine.

