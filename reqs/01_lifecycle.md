# Lifecycle

Instance management, HTTP endpoints, post-completion linger, and application logging.

## $REQ_LIFE_001: Database Initialization on Startup
**Source:** ./specs/quartz-lifecycle.md (Section: "Instance Check")

On startup, the application opens or creates the SQLite database, sets WAL mode, enforces foreign keys, and executes the schema.

## $REQ_LIFE_002: Instance Check via Serving Port
**Source:** ./specs/quartz-lifecycle.md (Section: "Instance Check")

On startup, the application reads the `config` table for `"serving-port"`. If found, it POSTs to `/app-path` on `127.0.0.1:{port}`. If the returned canonical path matches the current instance's config file path, it prints `Already running against <config-file-path>` and exits with code 0.

## $REQ_LIFE_003: Instance Takeover on Failure
**Source:** ./specs/quartz-lifecycle.md (Section: "Instance Check")

If the instance check fails (no `serving-port` row, connection refused, or path mismatch), the new instance proceeds with startup.

## $REQ_LIFE_004: OS-Assigned Port Binding
**Source:** ./specs/quartz-lifecycle.md (Section: "Instance Check")

The application binds to `127.0.0.1:0` (OS-assigned port) and upserts `"serving-port"` in the `config` table.

## $REQ_LIFE_005: App Path Endpoint
**Source:** ./specs/quartz-lifecycle.md (Section: "Endpoints")

`POST /app-path` accepts an empty request body and returns the canonical path to the resolved config file as a JSON string. The endpoint is localhost only.

## $REQ_LIFE_006: Shutdown Endpoint - Valid Request
**Source:** ./specs/quartz-lifecycle.md (Section: "Endpoints")

`POST /shutdown` accepts a JSON body with a `timestamp` field in `YYYYMMDDTHHmmss.ffffffZ` format. If the timestamp is within ±5 seconds of server time, it responds `{"shutting_down": true}`, flushes the response, and exits the process immediately with code 0 regardless of in-progress work.

## $REQ_LIFE_007: Shutdown Endpoint - Invalid Timestamp
**Source:** ./specs/quartz-lifecycle.md (Section: "Endpoints")

If the shutdown request timestamp is absent, malformed, or outside the ±5-second window, the endpoint responds HTTP 403 with `{"error": "invalid timestamp"}`.

## $REQ_LIFE_008: Shutdown Endpoint - Invalid JSON
**Source:** ./specs/quartz-lifecycle.md (Section: "Endpoints")

If the shutdown request body is not valid JSON, the endpoint responds HTTP 400.

## $REQ_LIFE_009: Post-Completion Linger
**Source:** ./specs/quartz-lifecycle.md (Section: "Post-Completion Linger")

After all sync work is complete, the application continues serving HTTP endpoints for 5 seconds, then exits with code 0. The process terminates on its own — it does not wait for a `/shutdown` request.

## $REQ_LIFE_010: Shutdown During Linger
**Source:** ./specs/quartz-lifecycle.md (Section: "Post-Completion Linger")

A valid `/shutdown` request received during the linger period causes immediate exit with code 0.

## $REQ_LIFE_011: Logging to Applog Table
**Source:** ./specs/quartz-lifecycle.md (Section: "Logging")

All application output goes to the `applog` table in the database.

## $REQ_LIFE_012: Log Purge on Insert
**Source:** ./specs/quartz-lifecycle.md (Section: "Logging")

On every log insert, entries older than `log-retention-days` are purged from the `applog` table.

## $REQ_LIFE_013: Log Levels
**Source:** ./specs/quartz-lifecycle.md (Section: "Logging")

The application uses four log levels: `error` (failures), `info` (lifecycle — startup, shutdown), `debug` (operational output), and `trace` (fine-grained diagnostics).

## $REQ_LIFE_014: Configurable Log Level
**Source:** ./specs/quartz-lifecycle.md (Section: "Logging")

The `config` table stores `"log-level"` (one of `error`, `info`, `debug`, `trace`; default: `info`). Messages below the configured level are discarded — not written to `applog`. On startup, if no `"log-level"` row exists, one is inserted with value `info`.

## $REQ_LIFE_015: KitchenSync Stdout Logging
**Source:** ./specs/quartz-lifecycle.md (Section: "Logging")

`info` and `error` log messages are also printed to stdout, in addition to being written to the `applog` table.

## $REQ_LIFE_016: KitchenSync Config Error Output
**Source:** ./specs/quartz-lifecycle.md (Section: "Logging")

Configuration errors are printed to stdout and cause immediate exit.

## $REQ_LIFE_017: Single Instance Per Config
**Source:** ./specs/quartz-lifecycle.md (Section: "Logging")

Only one instance may run per config file. Multiple KitchenSync binaries may run simultaneously as long as they use different config files.

## $REQ_LIFE_018: Localhost-Only Endpoints
**Source:** ./specs/quartz-lifecycle.md (Section: "Endpoints")

All HTTP endpoints are served on localhost only.
