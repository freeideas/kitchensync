# Lifecycle

Instance management, HTTP endpoints, startup sequence, and logging.

## $REQ_LIFE_001: Database Initialization on Startup
**Source:** ./specs/quartz-lifecycle.md (Section: "Instance Check")

On startup, open or create the SQLite database, set WAL mode, enforce foreign keys, and execute the schema.

## $REQ_LIFE_002: Instance Check Via Serving Port
**Source:** ./specs/quartz-lifecycle.md (Section: "Instance Check")

On startup, read the `config` table for `"serving-port"`. If found, POST `/app-path` on `127.0.0.1:{port}`.

## $REQ_LIFE_003: Instance Check Path Comparison
**Source:** ./specs/quartz-lifecycle.md (Section: "KitchenSync exceptions")

If the returned canonical path matches this instance's config file path, print `Already running against <config-file-path>` and exit(0). KitchenSync compares config file paths (not binary paths), allowing multiple instances with different config files.

## $REQ_LIFE_004: Instance Check Failure Continues
**Source:** ./specs/quartz-lifecycle.md (Section: "Instance Check")

If the instance check fails (no `serving-port` row, connection refused, or path mismatch), startup continues.

## $REQ_LIFE_005: Port Binding and Registration
**Source:** ./specs/quartz-lifecycle.md (Section: "Instance Check")

On startup (after passing instance check), bind `127.0.0.1:0` (OS-assigned port) and upsert `"serving-port"` in the config table.

## $REQ_LIFE_007: App-Path Endpoint
**Source:** ./specs/quartz-lifecycle.md (Section: "Endpoints")

POST `/app-path` returns the canonical path to the resolved config file as a JSON string. Localhost only.

## $REQ_LIFE_008: Shutdown Endpoint Timestamp Validation
**Source:** ./specs/quartz-lifecycle.md (Section: "Endpoints")

POST `/shutdown` accepts body `{"timestamp": "YYYYMMDDTHHmmss.ffffffZ"}`. The timestamp must be within 5 seconds in either direction of server time.

## $REQ_LIFE_009: Shutdown Endpoint Response
**Source:** ./specs/quartz-lifecycle.md (Section: "Endpoints")

POST `/shutdown` with a valid timestamp responds with `{"shutting_down": true}` and initiates shutdown.

## $REQ_LIFE_010: Endpoints Localhost Only
**Source:** ./specs/quartz-lifecycle.md (Section: "Endpoints")

All HTTP endpoints are bound to localhost only.

## $REQ_LIFE_011: Logging to Applog Table
**Source:** ./specs/quartz-lifecycle.md (Section: "Logging")

All output goes to the `applog` table with levels: `error` (failures), `info` (lifecycle), `debug` (operational), `trace` (diagnostics).

## $REQ_LIFE_012: Log Purge on Insert
**Source:** ./specs/quartz-lifecycle.md (Section: "Logging")

On every log insert, entries older than `log-retention-days` are purged.

## $REQ_LIFE_013: Startup Log Entry
**Source:** ./specs/quartz-lifecycle.md (Section: "Instance Check")

Startup is logged (info level).

## $REQ_LIFE_014: Completion Log Entry
**Source:** ./specs/sync.md (Section: "Run")

After a sync run completes, completion is logged (info level).

## $REQ_LIFE_015: Configuration Errors Print and Exit
**Source:** ./specs/quartz-lifecycle.md (Section: "KitchenSync exceptions")

Configuration errors are printed to stdout and cause immediate exit.

## $REQ_LIFE_016: Post-Completion Exit
**Source:** ./specs/quartz-lifecycle.md (Section: "Post-Completion Linger")

After all work is complete, the app continues serving HTTP endpoints for 5 seconds, then the process exits with code 0. The process must terminate on its own — it must not hang or wait for a `/shutdown` request. A test that launches the binary with a valid config and two reachable `file://` peers must see the process exit within 15 seconds of launch.
