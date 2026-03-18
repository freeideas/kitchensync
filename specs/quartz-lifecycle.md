# Quartz Lifecycle

Startup, logging, and instance management for quartz apps.

## Instance Check

On startup:

1. Open/create SQLite database, set WAL mode, enforce foreign keys, execute schema.
2. Read `config` table for `"serving-port"`. If found, POST `/app-path` on `127.0.0.1:{port}`. If the returned canonical path matches this app's path, print `Already running against <config-file-path>` and exit(0). On failure (no row, connection refused, path mismatch), continue.
3. Bind `127.0.0.1:0` (OS-assigned port), upsert `"serving-port"`, log startup.

If the process crashes, the OS releases the port. The next instance detects connection refused and takes over.

## Endpoints

Localhost only.

**POST /app-path** — returns canonical path to running binary (JSON string).

**POST /shutdown** — body: `{"timestamp": "YYYYMMDDTHHmmss.ffffffZ"}`, must be within 5 seconds in either direction of server time. Responds `{"shutting_down": true}`.

## Logging

All output goes to the `applog` table. On every insert, purge entries older than `log-retention-days`.

| Level   | Use                           |
| ------- | ----------------------------- |
| `error` | Failures                      |
| `info`  | Lifecycle (startup, shutdown) |
| `debug` | Operational output            |
| `trace` | Fine-grained diagnostics      |

KitchenSync exceptions (CLI tool, not a service):
- Does not print the port on startup (step 3).
- Configuration errors are printed to stdout and cause immediate exit.
- `/app-path` returns the canonical path to the resolved config file (not the binary). The instance check compares config file paths — multiple KitchenSync binaries may run simultaneously as long as they use different config files.
