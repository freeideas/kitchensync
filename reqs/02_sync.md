# Sync Run

Startup sequence, run phases, and operation queue for a sync execution.

## $REQ_SYNC_001: Config File Resolution
**Source:** ./specs/sync.md (Section: "Startup")

On startup, the config file path is resolved according to the rules in help.md.

## $REQ_SYNC_002: Database Open on Startup
**Source:** ./specs/sync.md (Section: "Startup")

On startup, the database is opened in WAL mode and the schema is executed.

## $REQ_SYNC_003: Instance Check
**Source:** ./specs/sync.md (Section: "Startup")

On startup, an instance check is performed. If another instance is using this database, the application prints `Already running against <config-file-path>` and exits with code 0.

## $REQ_SYNC_004: Parallel Peer Connection
**Source:** ./specs/sync.md (Section: "Startup")

On startup, connections to all peers are established in parallel. Unreachable peers are skipped with a logged warning.

## $REQ_SYNC_005: Minimum Reachable Peers
**Source:** ./specs/sync.md (Section: "Startup")

At runtime, at least two reachable peers are required. With `--canon`, one reachable peer (the canon peer itself) is sufficient.

## $REQ_SYNC_006: Startup Tombstone Purge
**Source:** ./specs/sync.md (Section: "Run")

Before the tree walk, snapshot tombstones (rows where `deleted_time IS NOT NULL`) with `deleted_time` older than `tombstone-retention-days` are purged.

## $REQ_SYNC_007: Startup Stale Row Purge
**Source:** ./specs/sync.md (Section: "Run")

Before the tree walk, stale snapshot rows where `deleted_time IS NULL` and `last_seen` is older than `tombstone-retention-days` (or `last_seen` is NULL) are purged — these are orphaned rows from entries that vanished from all peers.

## $REQ_SYNC_008: Startup Log Purge
**Source:** ./specs/sync.md (Section: "Run")

Before the tree walk, expired log entries are purged.

## $REQ_SYNC_009: Combined Tree Walk
**Source:** ./specs/sync.md (Section: "Run")

The sync executes a combined-tree walk as described in multi-tree-sync.md.

## $REQ_SYNC_010: Inline Directory Operations
**Source:** ./specs/sync.md (Section: "Run")

Directory creation and displacement to BACK/ run inline during the tree walk.

## $REQ_SYNC_011: Concurrent File Copies
**Source:** ./specs/sync.md (Section: "Run")

File copies are enqueued during the tree walk and executed concurrently, subject to per-peer connection limits.

## $REQ_SYNC_012: Wait for Copies
**Source:** ./specs/sync.md (Section: "Run")

After the tree walk, the application waits for all enqueued file copies to complete.

## $REQ_SYNC_013: Disconnect and Exit
**Source:** ./specs/sync.md (Section: "Run")

After all copies complete, all peers are disconnected, completion is logged, and the process exits.

## $REQ_SYNC_014: Copy Logging
**Source:** ./specs/sync.md (Section: "Logging")

Every file copy is logged at `info` level with the format `C <relative-path>`, logged once per decision (not per peer).

## $REQ_SYNC_015: Delete Logging
**Source:** ./specs/sync.md (Section: "Logging")

Every deletion (displacement to BACK/) is logged at `info` level with the format `X <relative-path>`, logged once per decision (not per peer).

## $REQ_SYNC_016: Peer Filesystem Abstraction
**Source:** ./specs/sync.md (Section: "Peer Filesystem Abstraction")

All sync logic operates through a single trait (interface) that both `file://` and `sftp://` implement. No protocol-specific code exists outside the trait implementations.

## $REQ_SYNC_017: Filesystem Abstraction Operations
**Source:** ./specs/sync.md (Section: "Required Operations")

The peer filesystem abstraction provides these operations: `list_dir`, `stat`, `read_file` (streaming), `write_file` (streaming, creating parent dirs as needed), `rename`, `delete_file`, `create_dir` (with parents), `delete_dir`, and `set_mod_time`.

## $REQ_SYNC_018: Uniform Error Types
**Source:** ./specs/sync.md (Section: "Error Semantics")

All filesystem operations return the same error types regardless of transport: not found, permission denied, I/O error. The sync logic never matches on transport-specific errors.
