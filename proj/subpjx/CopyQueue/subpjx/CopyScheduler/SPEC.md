# CopyScheduler:

## Purpose

CopyScheduler is the run-global execution engine behind the copy queue. It
accepts queued file copies, runs them concurrently, and holds the single limit
on how many file copies may be active at one instant across the whole run. It
also runs the queue's non-copy work items concurrently without letting them
consume copy slots, so listing, snapshot transfer, directory creation, and
staging cleanup never wait on a full copy limit. For each queued copy it tracks a
per-copy try budget, retries failures up to that budget by moving the copy to the
back of the queue, and reports a final per-copy outcome.

CopyScheduler does not perform the transfer itself: when a copy holds a slot it
drives SwapTransfer to carry out that one copy through the SWAP staging path.
CopyScheduler does not decide which files to copy, does not branch on peer
scheme, and does not own the progress-line text; it manages concurrency and
retry bookkeeping and reports outcomes.

## Responsibilities

### Run-global copy-slot limit

- Hold one limit on the number of file copies that are active at one instant
  across the whole run. The limit comes from `--max-copies`, defaulting to 10
  when the caller supplies no value (020.1, 020.2).
- Apply the limit to every file copy regardless of the source and destination
  peer schemes -- `file://` to `file://`, `file://` to `sftp://`, `sftp://` to
  `file://`, and `sftp://` to `sftp://` each count the same single slot (020.3).
- Charge a slot only for file copies. Non-copy work items submitted to the
  scheduler -- directory listing, snapshot download and upload, directory
  creation, and BAK/TMP/SWAP cleanup -- run concurrently and proceed even while
  the copy limit is already full (020.4).

### Incremental, concurrent execution

- Accept newly enqueued copies while earlier copies are still running, so copy
  work for an already scanned directory begins while later directories are still
  being scanned. The scheduler never waits for a whole-tree scan before starting
  copy work (020.5).
- Run the directory listings for all reachable peers at a given directory level
  concurrently rather than one after another, as a batch of non-copy work items
  that do not consume copy slots (020.6).

### Per-copy try budget

- Treat the copy try limit as the maximum total number of tries for one queued
  copy, counting the first try. The limit comes from `--retries-copy`, defaulting
  to 3 when the caller supplies no value (020.7, 020.8).
- When a copy try fails before that copy has reached its try limit, move the copy
  to the back of the queue and continue running other queued work (020.9).
- When a copy's try count reaches the try limit, mark the copy failed for the run
  and do not requeue it (020.10).
- Track tries independently per copy. One copy's failed tries never reduce the
  tries available to any other copy (020.11). The try limit applies identically
  to local copies, SFTP copies, and mixed-scheme copies (020.12).

### Dry-run

- In a dry-run the scheduler behaves the same way with respect to scheduling and
  retries: queued copies still acquire copy slots subject to the run-global
  active-copy limit (024.7), and the `--retries-copy` try limit still governs how
  many times each queued copy is tried (024.8). Whether a single try mutates peer
  state is SwapTransfer's concern, not the scheduler's.

## Boundaries

### Operations exposed across the boundary

- Configure the run's copy-slot limit and per-copy try limit from the values the
  caller derived from `--max-copies` and `--retries-copy`.
- Enqueue a file copy to be scheduled. The scheduler acquires a copy slot for it,
  drives SwapTransfer to perform the one transfer, applies the try budget on
  failure, and reports the per-copy outcome: succeeded, or failed for the run
  after exhausting its tries.
- Submit non-copy work to be run concurrently without a copy slot, including a
  batch of per-peer directory listings for one directory level issued together.
- Wait for all enqueued copies and submitted work to finish.

### Error obligations

- A copy try that fails is surfaced to the scheduler so it can decide between
  requeueing to the back of the queue (try count below the limit) and marking the
  copy failed for the run (try count at the limit). The scheduler never silently
  drops a copy: every enqueued copy ends as either succeeded or failed-for-the-run.
- The transfer's own failure recovery -- deleting a staged SWAP `new`, leaving an
  in-progress SWAP state for later recovery, setting the destination mod_time --
  belongs to SwapTransfer; the scheduler only observes try success or failure.
- Copy-slot trace events (a slot being acquired and released) are surfaced to the
  scheduler's caller (the CopyQueue facade), which routes them to the output
  component; the scheduler never writes them directly and keeps stderr empty.

### Invariants

- At most the configured number of file copies are active at any instant across
  the whole run, independent of peer scheme, peer count, and connection count.
- Non-copy work is never blocked by a full copy-slot limit.
- Each copy is tried at most its try-limit times in total, and try budgets are
  independent across copies.

### Not in scope

- Performing the transfer, the ordered SWAP `new`/`old` replacement sequence,
  setting the destination modification time, and SWAP recovery belong to
  SwapTransfer; the scheduler drives it but does not reimplement it.
- The uniform per-peer filesystem operations belong to the transport component;
  the scheduler never calls them directly and never branches on scheme.
- Deciding which files to copy and which modification time wins belongs to the
  sync engine.
- The exact `C`/`X` progress-line and copy-slot trace text belong to the logging
  concern, and routing them to the output component belongs to the CopyQueue
  facade; the scheduler surfaces trace events to its caller but does not own
  their wording and does not emit them directly.
