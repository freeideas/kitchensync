# runtime API

Rust module path: `kitchensync::runtime`.

The `runtime` module exports the run-time coordination surface for copy
scheduling, retry accounting, progress rendering, diagnostic rendering, and
copy-slot trace events. It does not export sync decision logic, transport I/O,
snapshot mutation, safe replacement sequencing, CLI parsing, or process exit
code selection.

## Consumed Root Contracts

The public API uses these root-owned shared contracts without redefining them:

- `RunConfig`
- `CopyTask`
- `CopyResult`
- `DiagnosticEvent`
- `ProgressEvent`
- `TransferPhase`
- `TransportError`
- `RelPath`
- `PeerUrl`

Callers must treat these values as the canonical representations of run
configuration, peer identity, relative paths, copy attempts, progress,
diagnostics, transfer phases, and transport error categories. Runtime must not
introduce public alternate forms for those concepts.

## Public Types

### `CopyScheduler`

`CopyScheduler` is the public scheduler for file-copy work.

Required behavior:

- accepts `CopyTask` values incrementally while traversal is still running;
- enforces `RunConfig.max_copies` as one global active file-copy limit;
- counts only active file-copy attempts against that limit;
- dispatches each started attempt to caller-supplied operations code;
- releases exactly one copy slot after each started attempt finishes;
- tracks a per-task try count;
- treats `RunConfig.retries_copy` as the maximum total tries, including the
  first try;
- requeues retryable failed tasks at the back of the queue until the configured
  try limit is reached;
- returns completion only after all accepted and requeued tasks have succeeded
  or failed terminally.

`CopyScheduler` must not expose worker queues, channel implementations,
threading primitives, transport handles, snapshot handles, or renderer state.

### `SchedulerConfig`

`SchedulerConfig` is the runtime-owned configuration extracted from
`RunConfig` when constructing a scheduler.

Fields:

- `max_copies: usize` - global maximum active file-copy attempts.
- `retries_copy: usize` - maximum total tries per logical `CopyTask`.

The root or orchestration layer is responsible for validation before this type
is constructed. Runtime treats these values as authoritative.

### `SchedulerSummary`

`SchedulerSummary` is returned when the scheduler has drained all accepted
copy work.

Fields:

- `succeeded: usize` - number of logical copy tasks that completed
  successfully.
- `failed: usize` - number of logical copy tasks that reached terminal
  failure.

The summary is runtime-visible accounting only. It does not select a process
exit code and does not imply snapshot mutation.

### `CopyAttemptFailure`

`CopyAttemptFailure` describes the normalized failure details supplied by an
operation attempt and recorded by runtime.

Fields:

- `phase: TransferPhase` - transfer phase that failed.
- `error: TransportError` - normalized transport error category.
- `message: Option<String>` - optional human-readable context supplied by the
  lower layer.

Runtime uses this data for diagnostics and retry accounting. It must not match
on transport implementation-specific error types.

### `CopyAttemptOutcome`

`CopyAttemptOutcome` is the operation callback result consumed by the scheduler.

Variants:

- `Success(CopyResult)` - the attempt completed successfully.
- `Failure(CopyAttemptFailure)` - the attempt failed with normalized phase and
  error information.

The successful `CopyResult` remains the root-owned shared contract. Runtime
does not add snapshot-specific completion semantics to it.

### `RuntimeOutputMode`

`RuntimeOutputMode` selects stdout progress rendering behavior.

Variants:

- `Interactive` - live terminal status may be rendered.
- `LineOriented` - plain progress lines are rendered with no terminal control
  sequences.

Output mode affects rendering only. It must not affect scheduling, retry
behavior, diagnostics generation, or copy results.

## Public Traits

### `CopyOperation`

`CopyOperation` is the callback interface used by `CopyScheduler` to execute one
copy attempt.

Required method:

```rust
fn execute_copy_attempt(&self, task: &CopyTask, progress: &dyn ProgressSink) -> CopyAttemptOutcome;
```

Ownership rules:

- `CopyScheduler` owns queued `CopyTask` values.
- The callback receives a shared borrow of the task for a single attempt.
- The callback owns file replacement behavior and dry-run mutation suppression.
- The callback reports byte progress through the provided `ProgressSink`.
- The callback returns only normalized success or failure data to runtime.

Runtime must not require this trait to expose transport handles, peer sessions,
snapshot stores, or operation internals.

### `ProgressSink`

`ProgressSink` is the public event sink for traversal and copy progress.

Required method:

```rust
fn publish(&self, event: ProgressEvent);
```

Required behavior:

- accepts scan-directory and active-copy progress events from any module passed
  the sink by orchestration;
- tracks the currently scanned directory;
- renders the root scan directory as `Scanning: .`;
- renders non-root scan directories as slash-separated paths relative to the
  sync root;
- tracks active copy progress by destination task, basename, transferred bytes,
  and optional total bytes;
- coalesces visible progress updates so stdout is refreshed at most once per
  second;
- suppresses info-level progress output at `error` verbosity;
- emits plain line-oriented progress in `LineOriented` mode with no terminal
  control sequences.

The sink owns rendering policy. Callers publish structured events and do not
branch on terminal capability or verbosity.

### `DiagnosticSink`

`DiagnosticSink` is the public event sink for diagnostics and trace output.

Required method:

```rust
fn publish(&self, event: DiagnosticEvent);
```

Required behavior:

- writes all rendered diagnostics and trace events to stdout only;
- applies cumulative verbosity filtering:
  - `error` renders error-level diagnostics only;
  - `info` renders error-level diagnostics and info-level progress;
  - `debug` is observationally identical to `info` until a debug-specific
    contract exists;
  - `trace` additionally renders copy-slot trace events;
- renders trace copy-slot events exactly as
  `copy-slots active=<n>/<max>`;
- renders failed transfer diagnostics with relative path, destination peer URL,
  failed `TransferPhase`, and normalized `TransportError` category when
  available.

Diagnostics are durable output. They must remain visible after any live
progress screen finishes.

## Public Constructors and Functions

### `CopyScheduler::new`

```rust
fn new(
    config: SchedulerConfig,
    diagnostics: impl DiagnosticSink + Send + Sync + 'static,
    progress: impl ProgressSink + Send + Sync + 'static,
) -> CopyScheduler;
```

Creates a scheduler bound to one run's copy limits, diagnostic sink, and
progress sink.

### `CopyScheduler::submit`

```rust
fn submit(&self, task: CopyTask);
```

Accepts one logical copy task. Submission must be valid while traversal is still
discovering more work.

### `CopyScheduler::close`

```rust
fn close(&self);
```

Signals that no more tasks will be submitted. Already accepted and requeued
tasks must still run to success or terminal failure.

### `CopyScheduler::run_until_complete`

```rust
fn run_until_complete(&self, operation: &dyn CopyOperation) -> SchedulerSummary;
```

Runs accepted copy work until all accepted tasks have reached a terminal state.
This function returns only after the scheduler is drained.

### `stdout_diagnostic_sink`

```rust
fn stdout_diagnostic_sink(config: &RunConfig, mode: RuntimeOutputMode) -> impl DiagnosticSink;
```

Creates the standard stdout diagnostic renderer for the run.

### `stdout_progress_sink`

```rust
fn stdout_progress_sink(config: &RunConfig, mode: RuntimeOutputMode) -> impl ProgressSink;
```

Creates the standard stdout progress renderer for the run.

## Ownership and Visibility Rules

- Public runtime values must be `Send` and `Sync` when they are intended to be
  shared across traversal and copy workers.
- Runtime owns scheduler queues, active-copy accounting, retry counters,
  progress renderer state, diagnostic renderer state, and runtime-visible copy
  summaries.
- Callers own peer sessions, transport handles, snapshot stores, operation
  executors, sync traversal state, and process exit handling.
- `CopyTask`, `CopyResult`, `DiagnosticEvent`, and `ProgressEvent` cross the
  API boundary as owned values unless a method signature explicitly borrows
  them for a single attempt.
- Runtime may clone or internally identify submitted copy tasks as needed, but
  it must not expose internal task IDs as a required sibling-module contract.
- Implementation modules, worker types, channels, permit guards, terminal
  renderer buffers, and retry-loop state are private to `runtime`.

## Error Contract

The runtime module does not define transport-specific error types. Copy attempt
failures are represented publicly with `CopyAttemptFailure`, which contains the
shared `TransferPhase` and `TransportError` contracts.

Scheduler completion failures are reflected in `SchedulerSummary.failed` and
the terminal `CopyResult` values made available through the root-owned copy
result contract. Runtime does not panic or abort the process for ordinary
copy-attempt failure, exhausted retries, or progress rendering suppression.

## Non-API Behavior

Other modules must not depend on:

- queue ordering beyond retry requeueing to the back of the logical queue;
- worker count, thread count, or async runtime choice;
- stdout refresh timing more precise than the at-most-once-per-second visible
  refresh rule;
- terminal control sequence details;
- formatting of diagnostics except where exact text is specified for public
  trace events;
- any internal representation of copy slots, progress rows, or completed task
  records.
