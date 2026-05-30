# runtime:

## Purpose

Own run-time coordination surfaces for file-copy scheduling, copy retry
accounting, the global active-copy limit, diagnostic events, progress events,
verbosity filtering, live terminal status, and line-oriented stdout progress.
The module provides the copy scheduler and output sinks used by root
orchestration while traversal and transfer work are in progress.

Runtime is not a sync decision engine and is not a filesystem mutation module.
It decides when queued copy attempts may run, how copy slots and try counts are
reported, and how run events are rendered. It does not decide which entries
should exist, how a replacement is made safe, or how peer transports perform
I/O.

## Responsibilities

### Copy Scheduler

- Expose a `CopyScheduler` API that accepts `CopyTask` values from traversal as
  soon as they are discovered.
- Start eligible copy work incrementally while traversal continues; the
  scheduler must not require the whole tree to be scanned before the first copy
  can run.
- Enforce `RunConfig.max_copies` as one global limit across the entire run.
  Each active file-copy attempt consumes exactly one copy slot regardless of
  whether the source and destination are `file://`, `sftp://`, or mixed
  schemes.
- Use the default max-copy value supplied by root configuration when the user
  does not override `--max-copies`; the runtime module treats the configured
  value as authoritative and does not parse CLI flags itself.
- Count only file-copy attempts against the active-copy limit. Directory
  listing, snapshot download, snapshot upload, directory creation,
  displacement, BAK cleanup, TMP cleanup, and SWAP cleanup are outside the copy
  slot count.
- Provide no user-visible per-peer, per-host, per-scheme, or per-connection
  transfer limit. Any internal worker or connection reuse strategy must preserve
  the single global active-copy rule.
- For each started copy attempt, call the operations layer to execute that
  attempt. Runtime owns scheduling and accounting around the call; operations
  owns the read/write, SWAP, BAK, cleanup, dry-run mutation suppression, and
  phase-specific transfer result.
- Release the copy slot exactly once after each copy attempt finishes, whether
  the attempt succeeds or fails.
- Wait for all accepted and requeued copy tasks to reach success or terminal
  failure before reporting scheduler completion to root.

### Retry Accounting

- Store an independent try count for every queued `CopyTask`.
- Treat `RunConfig.retries_copy` as the maximum total tries for a copy task,
  including the first try.
- Increment a task's try count after every failed copy attempt.
- If a failed attempt is eligible for retry and its try count is still below
  `RunConfig.retries_copy`, move that task to the back of the queue so other
  queued work can proceed.
- If a failed attempt has reached `RunConfig.retries_copy`, mark that task
  failed for the run and do not requeue it.
- Apply the same retry behavior to local, SFTP, and mixed-scheme copies.
- In dry-run mode, still schedule copy tasks, acquire copy slots, call
  operations for the dry-run copy attempt, and apply the same retry accounting.
  Runtime must not skip copy scheduling merely because `RunConfig.dry_run` is
  true.
- Surface terminal copy failures through `CopyResult` and diagnostics without
  mutating snapshot rows itself.

### Progress Sink

- Expose a `ProgressSink` API for traversal and copy workers to publish
  `ProgressEvent` values.
- Track the currently scanned directory. The root directory is rendered as
  `Scanning: .`; a non-root directory is rendered as its full slash-separated
  relative path from the sync root.
- Track active copy progress by destination task, basename, transferred bytes,
  and total bytes when a total is available.
- In interactive stdout mode at `info`, `debug`, or `trace` verbosity, render a
  live text status screen with:
  - one row per active file copy, up to the configured `max_copies`;
  - each active-copy row beginning with the copied file's basename, not the
    full path;
  - a progress bar that advances toward completion as bytes are copied;
  - a completed transfer's bar reaching completion before that row disappears
    or is replaced;
  - the currently scanned directory on the bottom line.
- If completed counts, failed counts, or an overall percentage are displayed,
  they must not displace active-copy rows or the bottom scanning line.
- Limit visible progress refreshes to at most once per second. Faster internal
  events are coalesced into the next refresh.
- In non-interactive stdout mode, render plain line-oriented progress at no
  more than once per second and emit no terminal control sequences.
- Include the currently scanned directory in non-interactive progress output.
- Keep error diagnostics and final completion messages visible after the live
  screen finishes.
- Suppress info-level progress output at `error` verbosity.

### Diagnostic Sink

- Expose a `DiagnosticSink` API for all modules to publish `DiagnosticEvent`
  values that runtime renders to stdout.
- Write all runtime-rendered diagnostics, progress, trace events, and
  completion messages to stdout only. Runtime must not write to stderr.
- Apply cumulative verbosity filtering:
  - `error` emits error-level diagnostics only;
  - `info` emits error-level diagnostics and info-level progress;
  - `debug` is observationally identical to `info` until debug-specific events
    are defined;
  - `trace` emits error-level diagnostics, info-level progress, and trace-level
    copy-slot events.
- At `trace` verbosity, emit a copy-slot event whenever a copy slot is acquired
  or released. The event text is exactly
  `copy-slots active=<n>/<max>`, where `<n>` is the global active file-copy
  count after the acquire or release and `<max>` is the configured global
  limit.
- Trace copy-slot events describe file-copy slots, not open network
  connections, SFTP sessions, workers, or peer handles.
- Render failed transfer diagnostics with the relative path, destination peer
  URL, failed transfer phase, and normalized transport error category when one
  is available.
- Render transfer phases only with the shared `TransferPhase` values:
  `read_source`, `write_swap_new`, `move_existing_to_swap_old`,
  `rename_final`, `set_mod_time`, `archive_old`, or `cleanup`.
- Preserve the required startup and run diagnostics passed through the sink,
  including unreachable peer messages, listing failures, final transfer
  failures, transfer failures after SWAP `old` exists, archive-old failures,
  displacement failures, staging failures, set-mod-time failures, snapshot
  upload failures, first-sync guidance, no-contributing-peer failures, dry-run
  notice text, and successful completion.

### Completion Reporting

- Return scheduler completion state to root only after all queued and requeued
  copy work has either succeeded or failed terminally for the run.
- Report copy success and terminal failure counts through progress or summary
  events when those counts are available.
- Do not choose the process exit code. Root orchestration maps startup,
  traversal, copy, snapshot upload, and completion outcomes to process exit
  status.

## Boundaries

- Runtime consumes `RunConfig`, `CopyTask`, `CopyResult`, `DiagnosticEvent`,
  `ProgressEvent`, `TransferPhase`, and `TransportError` from root-owned
  contracts. It should not introduce alternate representations of peer identity,
  relative paths, transport errors, or transfer phases.
- Runtime calls operations for a copy attempt but does not implement file
  replacement, bounded streaming, local-copy optimization, SWAP recovery, BAK
  archive, TMP cleanup, modification-time updates, or dry-run peer mutation
  suppression.
- Runtime does not parse command-line flags or validate option values. CLI
  parsing owns syntax and root passes validated `RunConfig` values to runtime.
- Runtime does not establish peer connections, select fallback URLs, apply SFTP
  timeouts, authenticate, verify host keys, or decide peer reachability.
- Runtime does not issue directory listings, retry directory listings, build
  traversal unions, apply excludes, or decide listing-error subtree behavior.
  Those are sync traversal responsibilities, though traversal may publish the
  currently scanned directory through `ProgressSink`.
- Runtime does not decide canon, subordinate, file, directory, deletion,
  modification, timestamp-tie, or type-conflict outcomes.
- Runtime does not create, update, cascade, clean, download, or upload snapshot
  rows or snapshot databases. It may report copy completion so the owning
  module can perform the required snapshot update.
- Runtime does not classify transport-specific errors. It renders only the
  normalized `TransportError` categories and phase information supplied by
  lower layers.
- Runtime does not own root process assembly, peer disconnection, or final exit
  code selection. It supplies scheduler and output results that root uses when
  completing the run.
