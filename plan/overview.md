# Plan Overview

KitchenSync has external risk points in four downloaded Rust crates and one
local operating-system behavior area.

- `sqlite-snapshot.md` proves `rusqlite` can create and update the peer snapshot
  database in rollback-journal mode, run the recursive subtree tombstone update,
  and leave a closed standalone `snapshot.db` ready for upload.
- `path-hashing.md` proves the `twox-hash` call for xxHash64 seed 0 and the
  base62 path IDs required by the snapshot schema.
- `timestamps.md` proves the timestamp parser and formatter with `chrono`, and
  records the needed manual handling of the six-digit microsecond field.
- `sftp-transport.md` proves `ssh2` against the bundled ephemeral SFTP server:
  known-host checking, Ed25519 key authentication, and the required SFTP file
  operations.
- `local-filesystem.md` proves the local filesystem behavior used by the file
  transport on this machine: setting file modification time, renaming a
  directory tree, and the observed overwrite behavior of `std::fs::rename`.

Every document above is backed by a runnable experiment under
`plan/experiments/` and indexed in `plan/PLAN.json`.
