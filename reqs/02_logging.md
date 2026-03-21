# Logging

Log levels, applog table management, and output behavior.

## $REQ_LOG_001: All Output to Applog
**Source:** ./specs/quartz-lifecycle.md (Section: "Logging")

All log output goes to the `applog` table in the database.

## $REQ_LOG_002: Log Purge on Insert
**Source:** ./specs/quartz-lifecycle.md (Section: "Logging")

On every insert to the `applog` table, entries older than `log-retention-days` are purged.

## $REQ_LOG_003: Log Levels
**Source:** ./specs/quartz-lifecycle.md (Section: "Logging")

Four log levels are supported: `error` (failures), `info` (lifecycle — startup, shutdown), `debug` (operational output), and `trace` (fine-grained diagnostics).

## $REQ_LOG_004: Log Level Filtering
**Source:** ./specs/quartz-lifecycle.md (Section: "Log Level")

Messages below the configured log level are discarded and not written to `applog`.

## $REQ_LOG_005: Log Level Stored in Config Table
**Source:** ./specs/quartz-lifecycle.md (Section: "Log Level")

The `config` table stores `"log-level"` (one of `error`, `info`, `debug`, `trace`). Default: `info`.

## $REQ_LOG_006: Default Log Level Initialization
**Source:** ./specs/quartz-lifecycle.md (Section: "Log Level")

On startup, if no `"log-level"` row exists in the `config` table, one is inserted with value `info`.

## $REQ_LOG_007: Info and Error to Stdout
**Source:** ./specs/quartz-lifecycle.md (Section: "KitchenSync exceptions")

`info` and `error` log messages are also printed to stdout, in addition to being written to `applog`.

## $REQ_LOG_008: Config Errors to Stdout
**Source:** ./specs/quartz-lifecycle.md (Section: "KitchenSync exceptions")

Configuration errors are printed to stdout and cause immediate exit.

## $REQ_LOG_009: Startup Logged at Info
**Source:** ./specs/quartz-lifecycle.md (Section: "Logging")

A startup event is logged at `info` level in the `applog` table.

## $REQ_LOG_010: Completion Logged at Info
**Source:** ./specs/sync.md (Section: "Run")

A completion/shutdown event is logged at `info` level in the `applog` table.

## $REQ_LOG_011: Copy Logged at Info
**Source:** ./specs/sync.md (Section: "Logging")

Every file copy is logged at `info` level with the format `C <relative-path>`. Logged once per decision, not per peer.

## $REQ_LOG_012: Deletion Logged at Info
**Source:** ./specs/sync.md (Section: "Logging")

Every deletion (displacement to BACK/) is logged at `info` level with the format `X <relative-path>`. Logged once per decision, not per peer.
