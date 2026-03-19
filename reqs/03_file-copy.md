# File Copy

Worker threads, XFER staging, BACK displacement, and transfer mechanics.

## $REQ_COPY_001: Worker Threads
**Source:** ./specs/sync.md (Section: "Run")

File copies are processed by worker threads. The number of workers is set by the `workers` config setting (default: 10).

## $REQ_COPY_002: Transfer to XFER Staging
**Source:** ./specs/sync.md (Section: "File Copy (Worker Thread)")

Files are first transferred to XFER staging on the destination: `<target-parent>/.kitchensync/XFER/<timestamp>/<uuid>/<basename>`.

## $REQ_COPY_003: Displace Existing to BACK
**Source:** ./specs/sync.md (Section: "File Copy (Worker Thread)")

Before the swap, the existing file at the destination is displaced to `<file-parent>/.kitchensync/BACK/<timestamp>/<basename>`.

## $REQ_COPY_004: Atomic Swap
**Source:** ./specs/sync.md (Section: "File Copy (Worker Thread)")

The file is renamed from XFER staging to the final path. This is a same-filesystem atomic rename.

## $REQ_COPY_005: XFER Directory Cleanup
**Source:** ./specs/sync.md (Section: "File Copy (Worker Thread)")

Empty XFER directories are cleaned up after the swap.

## $REQ_COPY_006: Streaming Transfer
**Source:** ./specs/sync.md (Section: "File Copy (Worker Thread)")

File content is streamed, not buffered in memory.

## $REQ_COPY_008: Displaced Directory Moved Whole
**Source:** ./specs/sync.md (Section: "BACK Directory")

A displaced directory is moved as a single rename, preserving its entire subtree.

## $REQ_COPY_009: Transfer Failure Handling
**Source:** ./specs/sync.md (Section: "Errors")

Transfer failures are logged. The file is skipped and re-discovered on the next run.

## $REQ_COPY_011: Copy Queue Drain
**Source:** ./specs/sync.md (Section: "Run")

After traversal completes, the program waits for the copy queue to drain before disconnecting peers and exiting.
