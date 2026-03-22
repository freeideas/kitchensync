# Sync Operations

File copy workflow, TMP staging, BAK displacement, logging, and the run sequence.

## $REQ_SYNCOP_001: Run Step - Purge Tombstones
**Source:** ./specs/sync.md (Section: "Run")

At the start of a run, snapshot tombstones older than `--td` days are purged. Stale rows where `deleted_time IS NULL` and `last_seen` is older than `--td` days (or `last_seen` is NULL) are also purged.

## $REQ_SYNCOP_002: Run Step - Combined Tree Walk
**Source:** ./specs/sync.md (Section: "Run")

The combined-tree walk is executed (see multi-tree-sync.md), with directory creation and displacement inline, file copies enqueued, and snapshots updated during traversal.

## $REQ_SYNCOP_003: Run Step - Wait for Copies
**Source:** ./specs/sync.md (Section: "Run")

After the tree walk, all enqueued file copies are waited on until complete.

## $REQ_SYNCOP_004: Run Step - Upload Snapshots
**Source:** ./specs/sync.md (Section: "Run")

Updated snapshots are written back to peers atomically: upload as `snapshot-new.db`, rename to `snapshot.db`.

## $REQ_SYNCOP_005: Run Step - Disconnect and Exit
**Source:** ./specs/sync.md (Section: "Run")

After snapshot upload, all peers are disconnected. Completion is logged and the program exits 0.

## $REQ_SYNCOP_006: Concurrent File Copy Execution
**Source:** ./specs/sync.md (Section: "Operation Queue")

File copies are enqueued during the tree walk and executed concurrently, subject to per-peer connection limits.

## $REQ_SYNCOP_008: File Copy to TMP Staging
**Source:** ./specs/sync.md (Section: "File Copy")

File copy step 1: transfer the file to TMP staging on the destination at `<target-parent>/.kitchensync/TMP/<timestamp>/<uuid>/<basename>`.

## $REQ_SYNCOP_009: Displace Existing Before Swap
**Source:** ./specs/sync.md (Section: "File Copy")

File copy step 2: if the destination already has a file at the target path, displace it to `<file-parent>/.kitchensync/BAK/<timestamp>/<basename>`.

## $REQ_SYNCOP_010: Atomic Swap from TMP
**Source:** ./specs/sync.md (Section: "File Copy")

File copy step 3: rename from TMP to the final path (same filesystem, atomic).

## $REQ_SYNCOP_011: Set mod_time After Copy
**Source:** ./specs/sync.md (Section: "File Copy")

File copy step 4: set the destination file's modification time to the winning mod_time from the decision (not re-read from the source).

## $REQ_SYNCOP_012: Clean Up Empty TMP Directories
**Source:** ./specs/sync.md (Section: "File Copy")

File copy step 5: clean up empty TMP directories after the swap.

## $REQ_SYNCOP_013: Streaming Transfer
**Source:** ./specs/sync.md (Section: "File Copy")

Content is streamed, not buffered entirely in memory.

## $REQ_SYNCOP_031: Concurrent Reader and Writer
**Source:** ./specs/sync.md (Section: "File Copy")

Each file transfer uses two concurrent tasks connected by a bounded channel: a reader task streams chunks from the source into the channel while a writer task simultaneously pulls chunks and writes them to the destination. The channel provides backpressure. A single-loop read-then-write pattern is not acceptable.

## $REQ_SYNCOP_014: TMP Cleanup on Transfer Failure
**Source:** ./specs/sync.md (Section: "File Copy")

On transfer failure, the TMP staging file/directory for that transfer is deleted before returning connections to the pool.

## $REQ_SYNCOP_015: Displace to BAK
**Source:** ./specs/sync.md (Section: "Displace to BAK")

Each displacement renames the entry at `path` to `<parent>/.kitchensync/BAK/<timestamp>/<basename>`. A displaced directory is moved as a single rename, preserving its entire subtree.

## $REQ_SYNCOP_016: BAK Timestamps
**Source:** ./specs/sync.md (Section: "BAK Directory")

The `<timestamp>` in BAK/ paths uses the format `YYYY-MM-DD_HH-mm-ss_ffffffZ`.

## $REQ_SYNCOP_017: BAK Cleanup After --bd Days
**Source:** ./specs/sync.md (Section: "BAK Directory")

Displaced entries in BAK/ are cleaned after `--bd` days (default: 90).

## $REQ_SYNCOP_018: TMP Timestamps
**Source:** ./specs/sync.md (Section: "TMP Staging")

The `<timestamp>` in TMP paths uses the format `YYYY-MM-DD_HH-mm-ss_ffffffZ`.

## $REQ_SYNCOP_019: TMP Cleanup After --xd Days
**Source:** ./specs/sync.md (Section: "TMP Staging")

Stale TMP staging directories are cleaned after `--xd` days (default: 2).

## $REQ_SYNCOP_020: No File Ever Destroyed
**Source:** ./specs/sync.md (Section: "BAK Directory")

No file is ever destroyed. Displaced entries are recoverable from BAK/.

## $REQ_SYNCOP_021: Log Copy at Info Level
**Source:** ./specs/sync.md (Section: "Logging")

Every file copy is logged at `info` level with format: `C <relative-path>`. Logged once per decision, not per peer.

## $REQ_SYNCOP_022: Log Deletion at Info Level
**Source:** ./specs/sync.md (Section: "Logging")

Every deletion (displacement to BAK/) is logged at `info` level with format: `X <relative-path>`. Logged once per decision, not per peer.

## $REQ_SYNCOP_023: All Output to Stdout
**Source:** ./specs/sync.md (Section: "Logging")

All output goes to stdout.

## $REQ_SYNCOP_024: BAK/TMP Cleanup During Traversal
**Source:** ./specs/multi-tree-sync.md (Section: "BAK/TMP Cleanup During Traversal")

After processing entries at each directory level, each peer's `.kitchensync/` directory at the current path is checked. If present, `BAK/` entries older than `--bd` days and `TMP/` entries older than `--xd` days are purged. The `<timestamp>` component of each subdirectory name determines its age.

## $REQ_SYNCOP_025: Peer Filesystem Abstraction
**Source:** ./specs/sync.md (Section: "Peer Filesystem Abstraction")

All sync logic operates through a single trait (interface) that both `file://` and `sftp://` implement. No protocol-specific code exists outside the trait implementations.

## $REQ_SYNCOP_026: Filesystem Trait Operations
**Source:** ./specs/sync.md (Section: "Required Operations")

The filesystem trait provides: `list_dir`, `stat`, `read_file` (stream), `write_file` (stream), `rename`, `delete_file`, `create_dir`, `delete_dir`, and `set_mod_time`.

## $REQ_SYNCOP_027: list_dir Omits Non-Regular Entries
**Source:** ./specs/sync.md (Section: "Required Operations")

`list_dir` returns only regular files and directories. Symbolic links, special files (devices, FIFOs, sockets), and other non-regular entry types are silently omitted.

## $REQ_SYNCOP_028: stat Returns Not Found for Symlinks
**Source:** ./specs/sync.md (Section: "Required Operations")

If the path is a symlink or special file, `stat` returns "not found."

## $REQ_SYNCOP_029: Uniform Error Types
**Source:** ./specs/sync.md (Section: "Error Semantics")

All filesystem operations return the same error types regardless of transport: not found, permission denied, I/O error. The sync logic never matches on transport-specific errors.

## $REQ_SYNCOP_030: Skip Copy When Already Matching
**Source:** ./specs/multi-tree-sync.md (Section: "Decision Rules")

If the winning entry already exists on a peer with a matching mod_time (within 5-second tolerance) and matching byte_size, no copy is performed for that peer — only the snapshot row is created/updated.
