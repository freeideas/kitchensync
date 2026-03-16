# KitchenSync

Real-time directory synchronization across multiple filesystem targets.

## What It Does

KitchenSync keeps a directory in sync across peers. On startup it walks all devices (local and peers concurrently), compares their states, and transfers files to resolve differences. Depending on the mode, it either continues running or exits.

## Modes

**Watch mode** (default): walks all devices, compares and transfers via per-peer queues (10 queues per peer, connections opened on demand), then watches the local filesystem for ongoing changes. The watcher enqueues changed paths for each peer as changes are detected. Runs until stopped.

**Once mode** (`--once`): walks all devices, drains all peer queues, then exits. No filesystem watcher. Pushes local changes to all peers and pulls peer changes locally, but does not propagate changes between peers — a subsequent run completes the propagation. Useful for scripting, cron jobs, and testing.

**Help** (`--help`): prints the contents of `help.txt` and exits.

Peers that are switched off or unreachable are skipped with a log entry.

## No Destructive Writes

KitchenSync never deletes nor overwrites files. When a sync operation would replace or remove a file, the existing file is first moved into `.kitchensync/BACK/<timestamp>/<filename>`.

## Timestamps

All timestamps throughout KitchenSync use a single format: `YYYYMMDDTHHmmss.ffffffZ` — UTC with microsecond precision (e.g. `20260314T091523.847291Z`). This applies to tombstones, BACK directory names, XFER directory names, reconcile_time, log entries, and the `/shutdown` API.

## The `.kitchensync/` Directory

Each synced directory contains a `.kitchensync/` directory that stores all local metadata. This directory is excluded from synchronization.

| Path                         | Purpose                                                             |
| ---------------------------- | ------------------------------------------------------------------- |
| `peers.conf`                 | Peer URLs, one per line                                             |
| `manifest`                   | List of all known file paths on this device, one per line           |
| `reconcile_time`             | Timestamp of last successful walk                                   |
| `SNAP/`                      | Tombstones only — one file per deleted path (see `sync.md`)         |
| `XFER/<uuid>/<timestamp>/`   | Staging area for in-progress transfers (see `sync.md`)              |
| `BACK/<timestamp>/`          | Displaced files, organized by timestamp                             |
| `kitchensync.sqlite`         | SQLite database: config and logging (see `quartz-lifecycle.md`)     |

## Implementation

Written in Rust. Built binaries go into `./released/` using platform-specific names:

| Platform | Binary              |
| -------- | ------------------- |
| Linux    | `kitchensync.linux` |
| Windows  | `kitchensync.exe`   |
| macOS    | `kitchensync.mac`   |
