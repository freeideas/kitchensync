# Quartz Lifecycle

Generic self-managing startup, logging, and shutdown behavior for quartz apps.

## Why Quartz Lifecycle

Working with quartz apps is intentionally low-friction:

- **Location is identity.** To start or stop a quartz app, you only need to know where it lives on the filesystem — no service registry, no config files, no environment variables.
- **Starting is idempotent.** Running the app when an instance is already up just prints the existing port and exits immediately. Scripts and tests can always "ensure it's running" without checking first. This also makes port re-discovery trivial: if a client loses its connection to a quartz app, it simply re-runs the binary to get the current port — whether the app was still running or crashed and restarted, the answer is always correct.
- **Port management is automatic.** Apps bind to an OS-assigned ephemeral port and self-register it. Callers read it from stdout. No port collisions, no hardcoded values, no coordination overhead.
- **Everything lives in one directory.** The SQLite database lives in the sync root's `.kitchensync/` directory. No separate database server, no connection strings, no deployment configuration.
- **Stdout and stderr are silent.** Apps emit exactly one line to stdout (a human-readable port announcement) and nothing to stderr. Log output goes into the database instead, so there's nothing to pipe, redirect, or discard.
- **Logging is compact and self-cleaning.** Log entries are timestamped in UTC and stored in SQLite. Rows older than 32 days are purged automatically — no log rotation, no runaway files.
- **Signals are unnecessary.** Stopping a quartz app via `POST /shutdown` is clean, cross-platform, and participates in the app's normal shutdown sequence. Process signals are flaky across platforms (especially on Windows, where they can broadcast to the entire process group) and bypass cleanup logic. With quartz-lifecycle, you rarely need them.

## Binary Identity

The **app path** is the canonical (absolute, symlink-resolved) path to the binary or script. For scripts, this is the script's own location (e.g. Python's `Path(__file__).resolve()`), not the interpreter path (`python3`, `uv`, etc.).

## Database Location

SQLite file at `.kitchensync/kitchensync.sqlite` within the sync root. This database stores config and logging only.

Example: if the sync root is `/home/ace/documents`, the database is `/home/ace/documents/.kitchensync/kitchensync.sqlite`.

## Startup Sequence

1. **Init database** — open/create the SQLite file at the derived path, set WAL mode (`PRAGMA journal_mode=WAL`), enforce foreign keys (`PRAGMA foreign_keys=ON`; note: this is a per-connection SQLite setting, not persisted to the file), execute the embedded schema.

2. **Check for running instance** — read the `config` table for key `"serving-port"`. If found, `POST /app-path` on `http://127.0.0.1:{port}`. If the returned canonical path (JSON string) matches this app's canonical path, print the port number to stdout and `exit(0)`. If anything fails (no row, connection refused, path mismatch), continue to step 3.

3. **Start HTTP server** — bind to `127.0.0.1:0` (OS-assigned ephemeral port), upsert `"serving-port"` in the `config` table with the assigned port, print the port number to stdout.

Apps insert app-specific initialization between these steps as needed (e.g. generating secrets after step 1).

## Stdout Contract

The app prints exactly one line to stdout: a human-readable message containing the port number (e.g. `Listening on port 12345`). Nothing else is ever written to stdout or stderr.

The line must contain exactly one sequence of consecutive digits (the port number). Readers extract the port by scanning for that sequence — they must not match on any specific prefix or format, as the wording may vary between apps.

## Logging

All log output is inserted into the `applog` table with a `stamp` in `YYYYMMDDTHHmmss.ffffffZ` format and a level. On every insert, rows older than 32 days are deleted.

**Levels:**

| Level   | Use                                                  |
| ------- | ---------------------------------------------------- |
| `error` | Failures, exceptions (replaces stderr)               |
| `info`  | Lifecycle events (startup, shutdown)                 |
| `debug` | Routine operational output (replaces stdout)         |
| `trace` | Fine-grained detail for diagnosing specific behavior |

`debug` and `trace` messages are ephemeral — add them while diagnosing a problem, then remove them once it's resolved. A production app should have no `debug` or `trace` calls.

**Required messages:**

- Log `info` on startup after the HTTP server is listening (not when detecting an existing instance and exiting).
- Log `info` on shutdown before exiting.

## Endpoints

### POST /app-path

Returns the canonical path to the running app as a JSON string. For scripts, returns the script's canonical path, not the interpreter.

**Input:** empty body (or ignored)

### POST /shutdown

Gracefully shut down the app.

**Input:**
```json
{
  "timestamp": "20260215T120000.000000Z"
}
```

The `timestamp` must be within 5 seconds of the server's current UTC time. If the timestamp is missing or stale, the server responds with HTTP 400 and does not shut down.

**Behavior:** On valid request, the server responds with `{"shutting_down": true}` and begins shutdown. Apps with active work (threads, connections) may perform brief cleanup before exiting with code 0. Simple apps may exit immediately.

## Schema

```sql
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS applog (
    log_id INTEGER PRIMARY KEY,
    stamp TEXT NOT NULL,
    level TEXT NOT NULL, -- e.g. 'info', 'error'
    message TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_applog_stamp ON applog(stamp);
```
