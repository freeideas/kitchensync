# Lifecycle

Instance management, HTTP endpoints, and post-completion behavior for the KitchenSync process.

## $REQ_LIFE_001: Singleton Per Config Directory
**Source:** ./specs/quartz-lifecycle.md (Section: "Instance Check")

Only one instance may run against a given config directory at a time. The singleton unit is the config directory.

## $REQ_LIFE_002: Database Initialization on Startup
**Source:** ./specs/quartz-lifecycle.md (Section: "Instance Check")

On startup, the application opens or creates the SQLite database, sets WAL mode, enforces foreign keys, and executes the schema.

## $REQ_LIFE_003: Instance Detection via Serving Port
**Source:** ./specs/quartz-lifecycle.md (Section: "Instance Check")

On startup, the application reads `"serving-port"` from the `config` table. If found, it POSTs to `/app-path` on `127.0.0.1:{port}`. If the returned canonical path matches the current instance's config directory, it prints `Already running` and exits with code 0.

## $REQ_LIFE_004: Instance Takeover on Stale Port
**Source:** ./specs/quartz-lifecycle.md (Section: "Instance Check")

If the serving-port check fails (no row, connection refused, or path mismatch), the application continues startup normally.

## $REQ_LIFE_005: OS-Assigned Port Binding
**Source:** ./specs/quartz-lifecycle.md (Section: "Instance Check")

The application binds to `127.0.0.1:0` (OS-assigned port) and upserts `"serving-port"` in the `config` table.

## $REQ_LIFE_006: App-Path Endpoint
**Source:** ./specs/quartz-lifecycle.md (Section: "Endpoints")

POST `/app-path` (empty request body) returns the canonical path to the config directory as a JSON string. The endpoint is localhost only.

## $REQ_LIFE_007: Shutdown Endpoint Valid Request
**Source:** ./specs/quartz-lifecycle.md (Section: "Endpoints")

POST `/shutdown` with body `{"timestamp": "YYYY-MM-DD_HH-mm-ss_ffffffZ"}` where the timestamp is within 5 seconds of server time responds with `{"shutting_down": true}`, flushes the response, and exits the process immediately with code 0 regardless of in-progress work.

## $REQ_LIFE_008: Shutdown Endpoint Invalid Timestamp
**Source:** ./specs/quartz-lifecycle.md (Section: "Endpoints")

POST `/shutdown` with a timestamp that is absent, malformed, or outside the ±5-second window responds with HTTP 403 and `{"error": "invalid timestamp"}`.

## $REQ_LIFE_009: Shutdown Endpoint Invalid JSON
**Source:** ./specs/quartz-lifecycle.md (Section: "Endpoints")

POST `/shutdown` with a body that is not valid JSON responds with HTTP 400.

## $REQ_LIFE_010: Post-Completion Linger
**Source:** ./specs/quartz-lifecycle.md (Section: "Post-Completion Linger")

After all work is complete, the application continues serving HTTP endpoints for 5 seconds, then exits with code 0. The process terminates on its own — it does not hang or wait for a `/shutdown` request.

## $REQ_LIFE_011: Shutdown During Linger
**Source:** ./specs/quartz-lifecycle.md (Section: "Post-Completion Linger")

A valid `/shutdown` request received during the post-completion linger period causes immediate exit with code 0.

## $REQ_LIFE_012: No Port Printed on Startup
**Source:** ./specs/quartz-lifecycle.md (Section: "KitchenSync exceptions")

KitchenSync does not print the port number on startup (unlike other quartz apps).
