# Quartz Lifecycle

Startup, logging, and instance management for quartz apps.

## Instance Check

The singleton unit is the config directory (default `~/.kitchensync/`). Only one instance may run against a given config directory at a time.

On startup:

1. Open/create SQLite database, set WAL mode, enforce foreign keys, execute schema.
2. Read `config` table for `"serving-port"`. If found, POST `/app-path` on `127.0.0.1:{port}`. If the returned canonical path matches the current instance's config directory, print `Already running` and exit(0). On failure (no row, connection refused, path mismatch), continue.
3. Bind `127.0.0.1:0` (OS-assigned port), upsert `"serving-port"`, log startup.

If the process crashes, the OS releases the port. The next instance detects connection refused and takes over.

## Endpoints

Localhost only.

**POST /app-path** — request body is empty. Returns the canonical path to the config directory (JSON string).

**POST /shutdown** — body: `{"timestamp": "YYYY-MM-DD_HH-mm-ss_ffffffZ"}`, must be within 5 seconds in either direction of server time. Responds `{"shutting_down": true}`, flushes the response, then exits the process immediately (exit code 0) regardless of any in-progress work. If the timestamp is absent, malformed, or outside the ±5-second window, responds HTTP 403 with `{"error": "invalid timestamp"}`. If the body is not valid JSON, responds HTTP 400.

## Post-Completion Linger

After all work is complete, the app continues serving HTTP endpoints for 5 seconds, then the process exits with code 0. The process must terminate on its own — it must not hang or wait for a `/shutdown` request. A valid `/shutdown` request received during the linger period causes immediate exit (code 0).

## Logging

All output goes to the `applog` table. On every insert, purge entries older than `log-retention-days`.

| Level   | Use                           |
| ------- | ----------------------------- |
| `error` | Failures                      |
| `info`  | Lifecycle (startup, shutdown) |
| `debug` | Operational output            |
| `trace` | Fine-grained diagnostics      |

### Log Level

The `config` table stores `"log-level"` (one of `error`, `info`, `debug`, `trace`). Default: `info`. Messages below the configured level are discarded — not written to `applog`. On startup, if no `"log-level"` row exists, insert one with value `info`.

KitchenSync exceptions (CLI tool, not a service):
- Does not print the port on startup (step 3).
- Configuration errors are printed to stdout and cause immediate exit.
- `info` and `error` log messages are also printed to stdout (in addition to being written to `applog`).
- On startup, before loading the config file, print the resolved config file path to stdout: `config: <absolute-path-to-kitchensync-conf.json>`.
