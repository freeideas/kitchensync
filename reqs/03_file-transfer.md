# File Transfer

File copy pipeline, XFER staging, pipelined streaming, and mod_time preservation.

## $REQ_XFER_001: Transfer Tuple
**Source:** ./specs/sync.md (Section: "File Copy")

Each file transfer is a `(src_peer, path, dst_peer, path)` pair.

## $REQ_XFER_002: XFER Staging Path
**Source:** ./specs/sync.md (Section: "File Copy")

Files are first transferred to XFER staging on the destination: `<target-parent>/.kitchensync/XFER/<timestamp>/<uuid>/<basename>`.

## $REQ_XFER_003: Displace Existing Before Swap
**Source:** ./specs/sync.md (Section: "File Copy")

If the destination already has a file at the target path, it is displaced to `<file-parent>/.kitchensync/BACK/<timestamp>/<basename>` before the swap.

## $REQ_XFER_004: Atomic Swap
**Source:** ./specs/sync.md (Section: "File Copy")

The staged file is renamed from XFER to the final path (same filesystem, atomic).

## $REQ_XFER_005: Set Mod Time After Copy
**Source:** ./specs/sync.md (Section: "File Copy")

After the swap, the destination file's modification time is set to the source file's mod_time.

## $REQ_XFER_006: Clean Up Empty XFER Directories
**Source:** ./specs/sync.md (Section: "File Copy")

After a successful transfer, empty XFER directories are cleaned up.

## $REQ_XFER_007: Streaming Content Transfer
**Source:** ./specs/sync.md (Section: "File Copy")

Content is streamed, not buffered entirely in memory.

## $REQ_XFER_008: Pipelined Reader and Writer
**Source:** ./specs/sync.md (Section: "File Copy")

Each transfer spawns two concurrent tasks connected by a bounded channel: a reader task that reads chunks from the source and pushes them into the channel, and a writer task that pulls chunks and writes to the destination. The reader and writer operate simultaneously with backpressure (reader blocks when channel is full, writer blocks when empty).

## $REQ_XFER_009: Failure Cleanup
**Source:** ./specs/sync.md (Section: "File Copy")

On transfer failure, the XFER staging file/directory for that transfer is deleted before returning connections to the pool.

