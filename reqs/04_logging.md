# Logging

Log output destination, verbosity levels, and format conventions.

## $REQ_LOG_001: All Output to Stdout
**Source:** ./specs/algorithm.md (Section: "Logging")

All output goes to stdout. No output to stderr. No logging frameworks that default to stderr.

## $REQ_LOG_002: Verbosity Levels
**Source:** ./README.md (Section: "Global Options")

The `-vl` flag controls verbosity with levels: error, warn, info, debug, trace. Default is `info`.

## $REQ_LOG_003: Copy Logged at Info
**Source:** ./specs/algorithm.md (Section: "Logging")

Every file copy is logged at `info` level as `C <relative-path>`. Logged once per decision, not per peer.

## $REQ_LOG_004: Delete Logged at Info
**Source:** ./specs/algorithm.md (Section: "Logging")

Every deletion/displacement is logged at `info` level as `X <relative-path>`. Logged once per decision, not per peer.

## $REQ_LOG_005: Connection Pool Trace Logging
**Source:** ./specs/concurrency.md (Section: "Trace Logging")

At verbosity `trace`, every pool acquire and release is logged as: `url=sftp://user@host/path connections=N/M`.

## $REQ_LOG_006: Pipeline Trace Logging
**Source:** ./specs/concurrency.md (Section: "Trace Logging")

At verbosity `trace`, pipelined transfer goroutine lifecycle is logged: `pipe reader-start`, `pipe writer-start`, `pipe reader-done`, `pipe writer-done` with source/destination URLs and file paths.

## $REQ_LOG_007: Watch Event Logging
**Source:** ./specs/watch.md (Section: "Logging")

Watch-triggered syncs are logged at `info` level with a `W` prefix: `W C <path>` for copies, `W X <path>` for deletions. The action letter comes from the sync decision, not the filesystem event type.

## $REQ_LOG_008: Watcher Registration Logging
**Source:** ./specs/watch.md (Section: "Logging")

Watcher registration is logged at `info`: `watching file:///path`. Failed watches are logged at `warn`.

## $REQ_LOG_009: Done Message
**Source:** ./specs/algorithm.md (Section: "Startup")

After sync completes successfully, `done` is logged at `info` level.

## $REQ_LOG_010: Timestamp Format in Logs
**Source:** ./specs/database.md (Section: "Timestamps")

Log output uses the same timestamp format as everywhere else: `YYYY-MM-DD_HH-mm-ss_ffffffZ`.
