# KitchenSync Architecture

KitchenSync should be split into a small first product module layer. The
semantic source defines a native Rust command-line executable, peer addressing
and connection behavior, a transport contract shared by local filesystem and
SFTP peers, a SQLite snapshot database, a combined-tree sync algorithm, safe
file replacement and staging rules, and global copy/progress controls. Keeping
these as one module would hide important contracts that the specs require to be
consistent across transports, snapshot decisions, and file operations.

## Root Responsibilities

The root product owns the executable assembly and the narrow contracts shared
between first-layer modules. It wires the CLI result into startup, peer
connection, snapshot lifecycle, sync traversal, queued copy execution, snapshot
upload, progress output, and process exit status.

Root glue responsibilities:

- preserve the command shape `kitchensync [options] <peer> <peer> [<peer>...]`
  and stdout-only process contract;
- pass validated run configuration, peer operands, and excludes to startup;
- create one run context containing dry-run mode, retry limits, copy limit,
  retention settings, verbosity, and timeout defaults;
- connect reachable peers, load local snapshot copies, run traversal and copy
  workers, upload snapshots in normal mode, disconnect peers, and map terminal
  outcomes to exit codes;
- keep sibling modules communicating through explicit Rust APIs at the root,
  not through each other's implementation details.

The root does not own sync decisions, transport-specific I/O, SQLite queries,
safe replacement sequencing, or progress rendering details.

## First-Layer Modules

### cli

Owns command-line parsing, validation, help text, peer operand syntax, global
option syntax, and conversion into a typed run request.

Anchors:

- `specs/sync.md` sections "Command Line", "Peers", "Fallback URLs",
  "Per-URL Settings", "Global Options", and startup validation;
- `specs/help.md` exact help output;
- requirements `001_cli-interface`, `002_help-screen`, and the syntax portions
  of `003_peer-addressing`.

Consumes root contracts: `RunConfig`, `PeerSpec`, `PeerRole`, `PeerUrl`,
`RelPath`, and `Verbosity`.

Exposes: a parsed invocation result: help, validation error plus help, or a
valid `RunRequest`.

### peer

Owns peer identity, URL normalization, fallback URL selection, startup
connectivity, peer role application, and construction of connected peer handles.
It decides which URL wins for a peer during a run and which reachable peers are
canon, normal contributing peers, or subordinate peers after snapshot existence
is known.

Anchors:

- `specs/sync.md` sections "URL Schemes", "Authentication", "Startup",
  "Canon Peer", and "Subordinate Peer";
- `specs/database.md` section "URL Normalization";
- `specs/concurrency.md` section "Connection Establishment";
- requirements `003_peer-addressing`, `004_peer-connectivity`, and
  `017_peer-roles-and-startup-state`.

Consumes root contracts: `RunConfig`, `PeerSpec`, `PeerRole`, `PeerId`,
`PeerUrl`, `TransportFactory`, `TransportRootMode`, `TransportHandle`, and
`DiagnosticSink`.

Exposes: reachable `PeerSession` values with normalized identity, selected URL,
role, transport handle, and snapshot-existence metadata.

### transport

Owns the language-native filesystem operation boundary used by the rest of the
product. It provides local `file://` and SSH/SFTP implementations with matching
observable behavior and normalized error categories.

Anchors:

- `specs/sync.md` sections "Peer Transports", "Required Operations",
  "Error Semantics", and "Case Sensitivity";
- `specs/TESTING-GUIDELINES.md` for local SFTP fixture expectations;
- requirement `015_transport-operations`;
- external standards: local host filesystem APIs, SSH/SFTP protocol behavior,
  known-host verification, and Rust stream/read/write abstractions.

Consumes root contracts: `PeerUrl`, `RelPath`, `EntryMeta`, `Timestamp`,
`TransportError`, `TransportRootMode`, and `RunConfig` timeout settings.

Exposes: `TransportHandle` operations for listing, stat, streaming reads and
writes, rename-without-overwrite, create/delete, and modification-time updates.
Sync logic must match only on the root `TransportError` categories, not on
implementation-specific errors.

### snapshot

Owns the per-peer SQLite snapshot database format, local temporary snapshot
copies, snapshot SWAP recovery/upload, row mutation APIs, path identifiers,
tombstones, and timestamp generation.

Anchors:

- `specs/database.md` in full;
- `specs/sync.md` sections "Startup", "Run", "Rename Compatibility", "TMP
  Staging", and snapshot-related errors;
- `specs/multi-tree-sync.md` sections "Snapshot Updates" and "Orphaned
  Snapshot Rows";
- requirements `005_snapshot-storage`, `006_snapshot-lifecycle`,
  `009_snapshot-updates`, and `016_snapshot-paths-and-timestamps`;
- external standard: SQLite rollback-journal database behavior.

Consumes root contracts: `PeerSession`, `RelPath`, `EntryMeta`, `Timestamp`,
`TransportHandle`, `TransportError`, and `RunConfig` retention settings.

Exposes: `SnapshotStore` APIs for lookup, upsert-present, mark-intended-copy,
mark-copy-complete, mark-absent, cascade-displaced-directory, stale-row cleanup,
download/create-local, and upload-through-SWAP.

### sync

Owns the combined-tree traversal and decision engine. It lists active peers at
each directory level, applies built-in and command-line excludes, classifies
peer states using snapshot rows, chooses file/directory/type-conflict outcomes,
updates snapshots at the points required by the source, and asks operations and
the copy scheduler to perform effects.

Anchors:

- `specs/multi-tree-sync.md` in full;
- `specs/sync.md` sections "Run", "Operation Queue", "Errors", and "Case
  Sensitivity";
- requirements `007_traversal-and-excludes`, `008_decision-making`,
  `009_snapshot-updates`, and the decision portions of
  `017_peer-roles-and-startup-state`.

Consumes root contracts: `RunConfig`, `PeerSession`, `RelPath`, `EntryMeta`,
`SnapshotStore`, `OperationExecutor`, `CopyScheduler`, `DiagnosticSink`, and
`ProgressSink`.

Exposes: a traversal API that runs one sync over connected peers and returns
copy work completion/failure state without exposing traversal internals.

### operations

Owns peer mutations other than abstract decision-making: safe file copy
replacement, SWAP recovery for user entries, inline displacement to nearby
BAK, TMP and BAK retention cleanup, and dry-run suppression of peer-side
mutations. This module is where the transport operation contract is composed
into the required safety sequences.

Anchors:

- `specs/sync.md` sections "Dry Run", "Rename Compatibility", "File Copy",
  "Displace to BAK", "TMP Staging", "SWAP Directory", and "BAK Directory";
- `specs/multi-tree-sync.md` sections "SWAP Recovery During Traversal",
  "BAK/TMP Cleanup During Traversal", and "Directory deletion";
- requirements `010_file-transfer-safety`, `011_displacement-and-staging-cleanup`,
  and `012_dry-run-mode`.

Consumes root contracts: `RunConfig`, `PeerSession`, `RelPath`, `EntryMeta`,
`Timestamp`, `TransportHandle`, `TransportError`, `DiagnosticSink`, and
`ProgressSink`.

Exposes: `OperationExecutor` APIs for recover-directory-swaps, displace,
create-directory, cleanup-retention, and execute-copy-attempt.

### runtime

Owns copy scheduling, retry accounting, global active-copy limits, progress
events, verbosity filtering, live terminal or line-oriented rendering, and
stdout-only diagnostics. It is a runtime coordination module, not the owner of
sync decisions or file replacement semantics.

Anchors:

- `specs/concurrency.md` in full;
- `specs/sync.md` sections "Operation Queue", "Logging", and error output;
- requirements `013_concurrency-controls` and `014_logging-and-progress`.

Consumes root contracts: `RunConfig`, `CopyTask`, `CopyResult`,
`DiagnosticEvent`, `ProgressEvent`, `TransferPhase`, and `TransportError`.

Exposes: `CopyScheduler`, `DiagnosticSink`, and `ProgressSink` APIs. Copy
workers call `operations` for each copy attempt; runtime owns when attempts run,
how tries are counted, and what progress/trace events are emitted.

## Root-Owned Shared Contracts

These contracts live at the root because they are consumed by multiple sibling
modules. They should stay behavioral and narrow.

- `RunConfig`: dry-run flag, copy and list retry counts, max active copies,
  timeouts, retention days, verbosity, and command-line excludes. Consumed by
  `peer`, `snapshot`, `sync`, `operations`, and `runtime`.
- `RelPath`: validated slash-separated relative path with no leading slash,
  trailing slash, empty segment, `.` segment, `..` segment, backslash, or NUL.
  Consumed by `cli`, `transport`, `snapshot`, `sync`, and `operations`.
- `PeerSpec`, `PeerUrl`, `PeerRole`, and `PeerId`: parsed peer operands,
  normalized peer identity, canon/subordinate/normal role, and stable per-run
  peer handle identity. Consumed by `cli`, `peer`, `snapshot`, `sync`, and
  `runtime`.
- `TransportRootMode`: selected peer-root construction policy for transport
  connection. `peer` chooses require-existing mode for dry runs and
  create-missing mode for normal runs; `transport` performs the scheme-specific
  check or creation before returning a rooted handle.
- `EntryMeta` and `EntryKind`: listed or stated filesystem metadata containing
  name, file/directory kind, modification time, and byte size, with directories
  represented as byte size `-1`. Consumed by `transport`, `snapshot`, `sync`,
  and `operations`.
- `Timestamp`: UTC `YYYY-MM-DD_HH-mm-ss_ffffffZ` value. The timestamp generator
  implementation belongs to `snapshot`, but the value type is shared by
  `transport`, `snapshot`, `sync`, `operations`, and `runtime`.
- `TransportError`: normalized `not_found`, `permission_denied`, and `io_error`
  categories. Produced by `transport`; consumed by `peer`, `snapshot`, `sync`,
  `operations`, and `runtime`.
- `TransferPhase`: one of `read_source`, `write_swap_new`,
  `move_existing_to_swap_old`, `rename_final`, `set_mod_time`, `archive_old`,
  or `cleanup`. Produced by `operations`; consumed by `runtime` diagnostics.
- `CopyTask` and `CopyResult`: source peer/path, destination peer/path, winning
  metadata, attempt result, and failure phase/category. Shared only between
  `sync`, `operations`, and `runtime`.
- `DiagnosticEvent` and `ProgressEvent`: stdout-renderable events for argument
  errors, skipped peers, listing failures, transfer failures, copy slot changes,
  active copy progress, scanned directory, and completion. Produced across
  modules and rendered by `runtime`.

No shared contract should include unrelated product behavior. For example,
`TransportHandle` provides filesystem operations but does not decide sync
outcomes; `SnapshotStore` stores and mutates rows but does not choose winners;
`CopyScheduler` limits and retries work but does not know SWAP sequencing.

## Boundary Rationale

The first-layer split follows the semantic source:

- CLI behavior is a public process surface with exact help text and validation
  output, so it is isolated from sync execution.
- Peer addressing and startup connectivity are separate from the lower
  transport operations because fallback URL selection, authentication, and
  role assignment occur before traversal.
- The transport boundary is required because `file://` and `sftp://` must
  present the same operations and error categories to the sync engine.
- Snapshot storage is separate because SQLite schema, path hashing, tombstones,
  local temp copies, and upload recovery are durable metadata contracts used by
  startup, traversal, decisions, and copy completion.
- Sync traversal and decisions are separate from operations because the source
  distinguishes deciding intended group state from safely mutating peer files.
- Operations are separate from runtime scheduling because `--max-copies` and
  retry accounting determine when copy attempts happen, while SWAP/BAK/TMP
  rules determine what each attempt does.
- Runtime output and scheduling are separate because progress, trace events,
  stdout-only diagnostics, and copy-slot accounting cut across traversal and
  transfers without owning their domain rules.

This module layer is the smallest split that preserves the required sibling
contracts without carving every requirement file into its own implementation
module.
