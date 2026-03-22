# Quartz Lifecycle

Startup, logging, and instance management.

## Schema

```sql
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS applog (
    id INTEGER PRIMARY KEY,
    timestamp TEXT NOT NULL,
    level TEXT NOT NULL,
    message TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_applog_timestamp ON applog(timestamp);
CREATE INDEX IF NOT EXISTS idx_applog_level ON applog(level);
```

## Instance Check

The singleton unit is the config directory. Only one instance may run against a given config directory at a time.

On startup:

1. Open/create SQLite database (`quartz.db`), set WAL mode, enforce foreign keys, execute schema.
2. Read `config` table for `"serving-port"`. If found, POST `/app-path` on `127.0.0.1:{port}`. The config directory path is OS-canonicalized (resolving symlinks and normalizing `.`/`..`) before comparison. If the returned canonical path matches the current instance's config directory, print `Already running` and exit(0). On failure (no row, connection refused, path mismatch), continue.
3. Bind `127.0.0.1:{port}` where `{port}` is an app-specified port or `0` (OS-assigned) if not specified in another specification. Upsert `"serving-port"`, log startup.

If the process crashes, the OS releases the port. The next instance detects connection refused and takes over.

## Command-Line Arguments

- **`--cfgdir <path>`** — Config/data directory. `<path>` is required when the flag is used. If `<path>` does not already end with `.<app-name>/` or `.<app-name>`, then `.<app-name>/` is appended. `<app-name>` is the stem of the binary or script (e.g. `clublog.exe` → `clublog`, `clublog.linux` → `clublog`, `myapp.py` → `myapp`). Default (when `--cfgdir` is omitted entirely): `~/.<app-name>/`. Created if it does not exist. Other specs may further refine the path processing (e.g. accepting an app-specific directory name).

There may be more command-line arguments specified in other specs.

## Endpoints

Localhost only, unless specified otherwise elsewhere.

**POST /app-path** — request body is empty. Returns the OS-canonicalized path to the config directory (JSON string).

**POST /shutdown** — body: `{"timestamp": "YYYY-MM-DD_HH-mm-ss_ffffffZ"}`, must be within 5 seconds in either direction of server time. Responds `{"shutting_down": true}`, flushes the response, then exits the process immediately (exit code 0) regardless of any in-progress work. If the timestamp is absent, malformed, or outside the ±5-second window, responds HTTP 403 with `{"error": "invalid timestamp"}`. If the body is not valid JSON, responds HTTP 400.

## Post-Completion Linger

Unless the app is the kind that runs until shut down, it should linger for 5 seconds after all work is complete — continuing to serve HTTP endpoints — then exit with code 0. The process must terminate on its own; it must not hang or wait for a `/shutdown` request. A valid `/shutdown` request received during the linger period causes immediate exit (code 0).

Apps that run until shut down run indefinitely, serving endpoints until terminated by a valid `/shutdown` request or an OS signal.

## Logging

All output goes to the `applog` table. Before every insert, purge entries older than 32 days (unless configured otherwise in a different specification).

| Level   | Use                           |
| ------- | ----------------------------- |
| `error` | Failures                      |
| `info`  | Lifecycle (startup, shutdown) |
| `debug` | Operational output            |
| `trace` | Fine-grained diagnostics      |

### Log Level

The `config` table stores `"log-level"` (one of `error`, `info`, `debug`, `trace`). Default: `info`. Messages below the configured level are discarded — not written to `applog`. On startup, if no `"log-level"` row exists, insert one with value `info`.

## KitchenSync Exceptions

- Does not print the port on startup (step 3).
- Configuration errors are printed to stdout and cause immediate exit.
- `info` and `error` log messages are also printed to stdout (in addition to being written to `applog`).
- On startup, before loading the config file, print the resolved config file path to stdout: `config: <absolute-path-to-kitchensync-conf.json>`.
- Log entry purge age is controlled by the `log-retention-days` setting (default: 32) instead of the hardcoded 32 days.
