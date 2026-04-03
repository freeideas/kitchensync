# Watch Mode

After initial sync, monitor local filesystems for changes and trigger targeted syncs.

## $REQ_WATCH_001: Initial Sync Then Watch
**Source:** ./specs/watch.md (Section: "Startup Sequence")

When `--watch` is specified, filesystem watchers are registered before the initial sync. Events queue during the initial sync, and queued events are processed after the initial sync completes.

## $REQ_WATCH_002: Watcher Registration
**Source:** ./specs/watch.md (Section: "Startup Sequence")

Filesystem watchers are registered on every `file://` peer's root directory (recursive). If the OS rejects a watch, a warning is logged and the program continues. If no watches succeed, an error is printed and the program exits 1.

## $REQ_WATCH_003: Single-Peer Watch Warning
**Source:** ./specs/watch.md (Section: "Startup Sequence")

With a single peer, `--watch` keeps the snapshot continuously up to date but performs no syncing. A warning is logged at startup: `--watch with one peer: snapshot only`.

## $REQ_WATCH_004: Snapshot-Based Debouncing
**Source:** ./specs/watch.md (Section: "Event Processing")

Watch events are debounced by comparing current file state against the snapshot. If the file matches its snapshot (same mod_time within tolerance), the event is a no-op.

## $REQ_WATCH_005: Per-Entry Decision Algorithm
**Source:** ./specs/watch.md (Section: "Event Processing")

Watch events trigger the normal decision algorithm for a single entry, not the full-tree sync. Live state is gathered from watched local peers via `stat()`; snapshot state is used for unwatched/SFTP peers.

## $REQ_WATCH_006: Self-Triggered Event Suppression
**Source:** ./specs/watch.md (Section: "Self-Triggered Event Suppression")

Filesystem events caused by KitchenSync's own writes to watched peers (file copies, displacements, directory creation/deletion) are suppressed and do not trigger re-syncing.

## $REQ_WATCH_007: Watch Events Filtered by Ignore Rules
**Source:** ./specs/watch.md (Section: "Ignore Rules")

Watch events are filtered through the same `.syncignore` rules as the normal walk. Events inside `.kitchensync/` directories are always suppressed.

## $REQ_WATCH_008: Watch Shutdown Sequence
**Source:** ./specs/watch.md (Section: "Shutdown")

On shutdown (Ctrl+C / SIGINT / SIGTERM / `POST /shutdown`): stop accepting new events, wait for in-progress copies up to 30 seconds (abort remaining after timeout), upload final snapshots, exit 0.

## $REQ_WATCH_009: Watch with Canon
**Source:** ./specs/watch.md (Section: "Interaction with Other Flags")

`--watch` with canon (`+`) works normally. If the canon peer is a watched local peer, its changes always win.

## $REQ_WATCH_010: Watch with Subordinate
**Source:** ./specs/watch.md (Section: "Interaction with Other Flags")

`--watch` with subordinate (`-`): subordinate local peers are watched but their changes trigger decisions where they don't vote -- external changes to a subordinate are overwritten by the group's state.

## $REQ_WATCH_011: Watch with Dry-Run
**Source:** ./specs/watch.md (Section: "Interaction with Other Flags")

`--dry-run` with `--watch` performs the initial sync in dry-run mode, then watches and logs what would happen for each change without executing.

## $REQ_WATCH_012: Watch-Triggered Sync Logging
**Source:** ./specs/watch.md (Section: "Logging")

Watch-triggered syncs are logged at `info` level with a `W` prefix to distinguish from initial-sync operations (e.g., `W C photos/vacation/img001.jpg`, `W X documents/draft.txt`).

## $REQ_WATCH_013: Watch Mode Snapshot Checkpoints
**Source:** ./specs/watch.md (Section: "Snapshot Checkpoints")

The `--si` checkpoint interval applies during watch mode. Snapshots are uploaded periodically as changes accumulate, protecting against connection loss during long watch sessions.

## $REQ_WATCH_014: Watcher Registration Logging
**Source:** ./specs/watch.md (Section: "Logging")

Successful watcher registration is logged at `info` level (e.g., `watching file:///c:/photos`).
