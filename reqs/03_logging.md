# Logging

Log output format, destinations, and verbosity levels.

## $REQ_LOG_001: All Output to Stdout
**Source:** ./specs/algorithm.md (Section: "Logging")

All output goes to stdout. No output to stderr. No logging frameworks that default to stderr.

## $REQ_LOG_002: Copy Logged at Info
**Source:** ./specs/algorithm.md (Section: "Logging")

Every file copy is logged at `info` level with format: `C <relative-path>`.

## $REQ_LOG_003: Delete Logged at Info
**Source:** ./specs/algorithm.md (Section: "Logging")

Every deletion is logged at `info` level with format: `X <relative-path>`.

## $REQ_LOG_004: Logged Once Per Decision
**Source:** ./specs/algorithm.md (Section: "Logging")

Copy and delete operations are logged once per decision, not once per peer.

## $REQ_LOG_005: Pool Changes at Trace
**Source:** ./specs/concurrency.md (Section: "Trace Logging")

At verbosity `trace`, every pool acquire and release is logged with format: `url=<url> connections=<current>/<max>`.

## $REQ_LOG_006: Pipelined Transfer Lifecycle at Trace
**Source:** ./specs/concurrency.md (Section: "Trace Logging")

At verbosity `trace`, pipelined transfer goroutines log their lifecycle: `pipe reader-start`, `pipe writer-start`, `pipe reader-done`, `pipe writer-done` with source/destination and file path. Concurrent operation is confirmed when `writer-start` appears before `reader-done` for the same file.

## $REQ_LOG_007: Done Message
**Source:** ./specs/algorithm.md (Section: "Startup")

On successful completion, `done` is logged at `info` level.
