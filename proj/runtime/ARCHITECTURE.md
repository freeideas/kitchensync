# Runtime Architecture

The `runtime` module owns cross-cutting execution support for KitchenSync:
copy concurrency limits, copy retry accounting, diagnostic events, progress
events, verbosity filtering, live terminal status, and line-oriented stdout
progress. It does not decide sync outcomes, perform transport-specific I/O,
mutate snapshots, classify transport failures beyond rendering normalized
categories, own peer connection behavior, or implement safe replacement rules.
Those behaviors belong to sibling modules; `runtime` only coordinates when copy
attempts run and how execution state is reported.

This module should remain a leaf module. Its scope is narrow enough that child
modules would add navigation overhead before there is an implementation need.
Internal files may be split by concern, but they should stay private to
`runtime`.

## Responsibilities

`runtime` provides the execution helpers used by root orchestration, sync
traversal, and transfer attempts while a run is in progress. The important
public surface is a small set of language-native Rust APIs:

- `CopyScheduler` accepts copy work from sync orchestration, enforces the
  configured maximum active copy count, dispatches attempts, retries failed
  attempts within configured limits, and returns per-task completion state.
- `DiagnosticSink` accepts stdout-renderable diagnostics and trace events from
  runtime users, filters them by verbosity, and renders them through the
  process output contract.
- `ProgressSink` accepts scan and transfer progress events, coalesces live
  status when appropriate, and emits stable line-oriented output when live
  terminal rendering is not available.
- Copy failure accounting records runtime-visible attempt and terminal copy
  summaries so callers can report completion state without inspecting runtime
  internals.

The module should keep these contracts behavioral. It may expose task handles,
result summaries, and sink traits or structs, but it should not expose worker
queues, renderer state, retry loop internals, or synchronization primitives as
part of the public API.

## Internal Design

The scheduler is the central runtime component. It owns an execution queue,
active-copy permits, and per-task attempt counters. Each queued copy task
contains the root-owned copy contract data needed by the worker callback:
source, destination, expected metadata, and enough identity to report progress
and errors. The scheduler does not know how a file is copied. For each attempt,
it invokes the operation supplied by the caller and records only the returned
success or normalized failure data.

The active-copy permit count is one global run-wide count. Every started
file-copy attempt consumes one permit regardless of source or destination
scheme, and non-copy work such as listing, snapshot upload, directory creation,
displacement, and cleanup never consumes a copy permit. Runtime may choose any
private worker, task, or channel structure, but that structure must preserve the
single configured `max_copies` limit as the only user-visible transfer limit.

Retry accounting is attached to the logical task rather than to a transport
operation. A failed attempt records the failed phase and normalized error
category, emits a diagnostic event, releases the active-copy slot, and is either
requeued or finalized according to the configured retry count. Attempts that
fail during source reads, destination writes, renames, timestamp updates, or
cleanup are all handled through the same retry path; the phase is diagnostic
data, not scheduler control flow.

Progress state is separate from scheduling decisions. The scheduler reports copy
slot acquisition, attempt start, byte progress, attempt completion, and terminal
task status through `ProgressSink`. Traversal code may also report scanned
directories or listing progress through the same sink. The sink owns rendering
policy so callers do not branch on terminal capabilities or verbosity.

Diagnostics and progress share the stdout-only output rule but remain distinct
streams internally. Diagnostics are durable user-facing messages such as
argument errors, skipped peers, listing failures, transfer failures, and trace
logging. Progress is transient or summary execution status. Keeping them
separate lets quiet, normal, verbose, and trace modes filter output without
changing copy execution behavior.

Copy accounting is append-only during a run. Runtime records structured attempt
and terminal copy summaries and exposes them to the root or sync orchestration.
The summaries should contain enough information to print required completion
output and inform the caller's final result handling, but they should not
contain transport handles, snapshot handles, or renderer implementation state.

## Output Rendering

All runtime-rendered diagnostics, progress, trace events, and completion
messages go to stdout. Runtime does not write to stderr. The renderer applies
cumulative verbosity filtering: `error` emits only error diagnostics; `info`
emits error diagnostics and info progress; `debug` matches `info` until a
debug-specific contract exists; and `trace` also emits copy-slot trace events.
Trace copy-slot events are emitted after each acquire or release using exactly
`copy-slots active=<n>/<max>`, where `<n>` is the active file-copy count after
the change and `<max>` is the configured global limit.

Interactive progress rendering is a private renderer behind `ProgressSink`.
At `info`, `debug`, and `trace`, it renders one active file-copy row per active
copy up to `max_copies`, uses the destination basename at the start of each
row, advances byte progress bars when byte totals are available, and keeps the
currently scanned directory on the bottom line. The root scan directory renders
as `Scanning: .`; other directories render as their full slash-separated
relative path from the sync root. A completed copy row must be able to show a
complete bar before the row is removed or reused.

Refresh throttling belongs to the renderer. Progress events may arrive faster
than the terminal is updated, but visible progress output is limited to at most
once per second. In non-interactive stdout mode the same sink emits plain
line-oriented progress, includes the current scan directory, respects the same
refresh limit, and emits no terminal control sequences. Durable diagnostics and
final completion messages remain visible after any live progress screen ends.

## Data Flow

1. The root constructs runtime using the run configuration: maximum active
   copies, retry limits, verbosity, and output mode.
2. Sync traversal submits copy tasks to `CopyScheduler` and passes an attempt
   callback that performs the actual file replacement through the appropriate
   sibling implementation.
3. `CopyScheduler` waits for an available copy slot, starts an attempt, and
   emits progress and trace events.
4. The attempt callback returns a copy result containing success or a normalized
   failure with transfer phase and transport error category.
5. Runtime updates task state, records diagnostics, retries when allowed, and
   finalizes the task when it succeeds or exhausts retries.
6. Sync receives completed copy results and continues traversal or final
   snapshot update flow according to its own rules.
7. At run completion, the root and sync orchestration use runtime summaries and
   their own domain state to derive process status and any required summary
   output.

Listing and connection code may emit diagnostics or progress through the runtime
sinks when passed those APIs by orchestration, but they should not use
`CopyScheduler` unless they are executing copy tasks governed by the global
active-copy limit.

## Dependencies

`runtime` consumes only narrow root-owned contracts and standard Rust runtime
facilities:

- `RunConfig` values for retry counts, maximum active copies, verbosity, and
  output-related behavior.
- `CopyTask` and `CopyResult` for scheduler input and output.
- `DiagnosticEvent`, `ProgressEvent`, and `TransferPhase` for structured
  reporting.
- `TransportError` categories for retry diagnostics and failure summaries.
- Rust synchronization, channels, task spawning, timing, and stdout writing
  facilities selected by the implementation.

`runtime` must not depend on transport-specific clients, SQLite types, CLI
parser structures, or sync decision internals. Sibling modules communicate with
runtime through the public scheduler and sink APIs only.

## Boundary Rules

Runtime owns:

- copy-slot accounting;
- retry attempt counting;
- queueing and completion ordering for submitted copy tasks;
- progress event filtering and rendering;
- trace and diagnostic output filtering;
- structured aggregation of runtime-visible failures.

Runtime does not own:

- peer URL parsing or connection selection;
- filesystem listing, stat, read, write, rename, delete, or timestamp behavior;
- SQLite snapshot schema, row mutation, or upload recovery;
- traversal ordering, winner selection, excludes, tombstone decisions, or
  subordinate peer behavior;
- SWAP, TMP, or BAK sequencing for safe replacement;
- final CLI parsing or help text.

If future requirements need more runtime surface area, prefer adding narrow
methods to the existing scheduler or sink contracts before creating child
modules. Split into child modules only if there are independently specified
subsystems with separate public contracts.
