# Quartz Lifecycle

Generic self-managing startup, logging, and shutdown behavior for quartz apps.

## Why Quartz Lifecycle

Working with quartz apps is intentionally low-friction:

- **Location is identity.** To start or stop a quartz app, you only need to know where it lives on the filesystem -- no service registry, no config files, no environment variables.
- **Starting is idempotent.** Running the app when an instance is already up just prints the existing port and exits immediately. Scripts and tests can always "ensure it's running" without checking first.
- **Port management is automatic.** Apps bind to an OS-assigned ephemeral port and self-register it. Callers read it from stdout. No port collisions, no hardcoded values, no coordination overhead.
- **Everything lives in one directory.** The SQLite database lives in the sync root's `.kitchensync/` directory. No separate database server, no connection strings, no deployment configuration.
- **Stdout and stderr are silent.** Apps emit exactly one line to stdout (a human-readable port announcement) and nothing to stderr. Log output goes into the database instead.
- **Logging is compact and self-cleaning.** Log entries are timestamped in UTC and stored in SQLite. Rows older than `log-retention-days` (default: 32) are purged automatically.
- **Signals are unnecessary.** Stopping a quartz app via `POST /shutdown` is clean, cross-platform, and participates in the app's normal shutdown sequence.

## Binary Identity

The **app path** is the canonical (absolute, symlink-resolved) path to the binary or script. For scripts, this is the script's own location (e.g. Python's `Path(__file__).resolve()`), not the interpreter path.

## Database Location

By default, the database is `{app-stem}.db` in the same directory as the running binary or script. For example, if the binary is `/usr/local/bin/myapp`, the database is `/usr/local/bin/myapp.db`.

**KitchenSync override:** The database is `.kitchensync/kitchensync.db` within the sync root, not next to the binary. Example: if the sync root is `/home/bilbo/documents`, the database is `/home/bilbo/documents/.kitchensync/kitchensync.db`.

## Startup Sequence

1. **Init database** -- open/create the SQLite file, set WAL mode (`PRAGMA journal_mode=WAL`), enforce foreign keys (`PRAGMA foreign_keys=ON`), execute the embedded schema. Why WAL mode? Allows concurrent reads during writes, better crash recovery, and improved performance for the mixed read/write workload of sync operations.

2. **Check for running instance** -- read the `config` table for key `"serving-port"`. If found, `POST /app-path` on `http://127.0.0.1:{port}`. If the returned canonical path matches this app's canonical path, print the port number to stdout and `exit(0)`. If anything fails (no row, connection refused, path mismatch), continue to step 3.

3. **Start HTTP server** -- bind to `127.0.0.1:0` (OS-assigned ephemeral port), upsert `"serving-port"` in the `config` table with the assigned port, print the port number to stdout.

Apps insert app-specific initialization between these steps as needed.

### Why Not a PID-Based Lockfile?

A traditional lockfile stores a PID. This is unreliable because:

1. **Stale locks after crashes.** The process dies without cleaning up; the PID in the lockfile no longer exists, or worse, has been reused by an unrelated process.
2. **PID reuse.** Operating systems recycle PIDs. A stale lockfile containing PID 12345 might match a completely unrelated process that was later assigned the same PID.

The port-based approach avoids both problems: if the process dies, the OS releases the port, and any connection attempt to it will fail. The `serving-port` row in the database becomes stale but harmless -- the next instance's identity challenge (canonical path match via `POST /app-path`) will fail and take over.

### Why Port 0?

1. **No collisions.** The OS guarantees the assigned port is currently unused.
2. **No configuration.** No need to choose a port range or handle "port already in use" retries.
3. **Firewall-friendly.** Ephemeral ports from the OS are typically in a range that localhost traffic can use without special rules.

## Stdout Contract

The app prints exactly one line to stdout: a human-readable message containing the port number (e.g. `Listening on port 12345`). Nothing else is ever written to stdout or stderr.

The line must contain exactly one sequence of consecutive digits (the port number). Readers extract the port by scanning for that sequence.

**KitchenSync override:** KitchenSync is a user-facing CLI tool, not a service controlled by other programs. The stdout port announcement will not be printed.

## Logging

All log output is inserted into the `applog` table with a `stamp` in `YYYYMMDDTHHmmss.ffffffZ` format and a level. On every insert, rows older than `log-retention-days` (default: 32) are deleted. Why 32 days default? Long enough to diagnose issues that span multiple sync sessions; short enough to keep the database compact. Roughly one month of history.

**Levels:**

| Level   | Use                                                  |
| ------- | ---------------------------------------------------- |
| `error` | Failures, exceptions (replaces stderr)               |
| `info`  | Lifecycle events (startup, shutdown)                 |
| `debug` | Routine operational output (replaces stdout)         |
| `trace` | Fine-grained detail for diagnosing specific behavior |

**Required messages:**

- Log `info` on startup after the HTTP server is listening (not when detecting an existing instance and exiting).
- Log `info` on shutdown before exiting.

**KitchenSync override:** Configuration errors are printed to stdout and cause immediate exit. This includes: no peers configured, peers.conf malformed, authentication failures (wrong password, key rejected), and missing peers.conf file. Normal operational errors (peers going offline, connection drops, transfer failures) are logged to the database but not printed -- these are transient conditions, not fatal problems.

## Endpoints

### POST /app-path

Returns the canonical path to the running app.

**Input:** empty body (or ignored)

**Response:** A JSON string (the path value enclosed in quotes):
```json
"/home/bilbo/.local/bin/kitchensync"
```

### POST /shutdown

Gracefully shut down the app.

**Input:**
```json
{
  "timestamp": "20260215T120000.000000Z"
}
```

The `timestamp` must be within 5 seconds of the server's current UTC time (absolute difference). Timestamps too far in the past OR future are rejected with HTTP 400. If the timestamp is missing or invalid, the server responds with HTTP 400 and does not shut down. Why require a timestamp? Prevents replay attacks and accidental shutdowns from stale requests. Why 5 seconds? Generous enough for network latency and minor clock drift; tight enough to reject genuinely stale requests.

**Behavior:** On valid request, the server responds with `{"shutting_down": true}` and begins shutdown.

## Schema

The local database contains three tables: `config` and `applog` for quartz-lifecycle, plus `snapshot` for sync state.

```sql
-- Quartz lifecycle tables
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS applog (
    log_id INTEGER PRIMARY KEY,
    stamp TEXT NOT NULL,
    level TEXT NOT NULL,
    message TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_applog_stamp ON applog(stamp);

-- Snapshot table (see database.md for details)
CREATE TABLE IF NOT EXISTS snapshot (
    id BLOB PRIMARY KEY,
    parent_id BLOB NOT NULL,
    basename TEXT NOT NULL,
    mod_time TEXT,
    byte_size INTEGER,
    del_time TEXT
);

CREATE INDEX IF NOT EXISTS idx_snapshot_parent ON snapshot(parent_id);
CREATE INDEX IF NOT EXISTS idx_snapshot_del ON snapshot(del_time);
```
