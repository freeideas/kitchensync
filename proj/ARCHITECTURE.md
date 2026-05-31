# KitchenSync Architecture

KitchenSync should be split into a small first product module layer. The
semantic source defines a native Rust command-line executable, peer addressing
and connection behavior, a transport contract shared by local filesystem and
SFTP peers, a SQLite snapshot database, a combined-tree sync algorithm, safe
file replacement and staging rules, and global copy/progress controls. Keeping
these as one module would hide contracts that the specs require to be
consistent across transports, snapshot decisions, and file operations.

## Root Responsibilities

The root product owns executable assembly and the narrow contracts shared
between first-layer modules. It wires the CLI result into startup, peer
connection, snapshot lifecycle, sync traversal, queued copy execution, snapshot
upload, progress output, and process exit status.

Root glue responsibilities:

- preserve the command shape `kitchensync [options] <peer> <peer> [<peer>...]`
  and the stdout-only process contract;
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

## Child Modules

### cli

Owns command-line parsing, validation, help text, peer operand syntax, global
option syntax, and conversion into a typed run request. It is carved because the
source defines the CLI as the public process surface with exact command shape,
help output, validation behavior, stdout diagnostics, and exit statuses.

### peer

Owns peer identity, URL normalization, fallback URL selection, startup
connectivity, peer role application, and construction of connected peer
sessions. It is carved because addressing, authentication, root reachability,
fallback selection, canon roles, and subordinate roles are startup concerns
that must be resolved before traversal and before transport operations are used
by other modules.

### transport

Owns the language-native filesystem operation boundary used by the rest of the
product, with local `file://` and SSH/SFTP implementations presenting matching
observable behavior. It is carved because the source requires both schemes to
provide the same operation set and normalized error categories while relying on
external filesystem, SSH/SFTP, known-host, and Rust stream/read/write
standards.

### snapshot

Owns the per-peer SQLite snapshot database format, local temporary snapshot
copies, snapshot SWAP recovery/upload, row mutation APIs, path identifiers,
tombstones, and timestamp generation. It is carved because the schema, path
hashes, tombstones, timestamp format, local copy lifecycle, and rollback-journal
SQLite behavior are durable metadata contracts used by startup, traversal,
decisions, and copy completion.

### sync

Owns the combined-tree traversal and decision engine: listing active peers at
each directory level, applying excludes, classifying peer states from snapshot
rows, choosing outcomes, and scheduling or requesting effects. It is carved
because the source separates deciding the intended group state from safely
mutating peer files and from runtime scheduling.

### operations

Owns peer mutations other than abstract decision-making: safe file copy
replacement, SWAP recovery for user entries, inline displacement to nearby
BAK, TMP and BAK retention cleanup, and dry-run suppression of peer-side
mutations. It is carved because the source defines ordered safety sequences for
SWAP, BAK, TMP, rename-without-overwrite compatibility, and recoverability that
must be composed over the transport API.

### runtime

Owns copy scheduling, retry accounting, global active-copy limits, progress
events, verbosity filtering, live terminal or line-oriented rendering, and
stdout-only diagnostics. It is carved because copy-slot accounting, retries,
trace output, and progress rendering cut across traversal and transfers without
owning sync decisions or file replacement semantics.

## Boundary Anchors

The child boundaries are anchored in the semantic source rather than in
requirement-file granularity:

- `cli`: `specs/sync.md` command line, peers, fallback URLs, per-URL settings,
  global options, startup validation, and errors; `specs/help.md`; requirements
  `001_cli-interface`, `002_help-screen`, and the syntax portions of
  `003_peer-addressing`.
- `peer`: `specs/sync.md` URL schemes, authentication, startup, canon peer, and
  subordinate peer; `specs/database.md` URL normalization; `specs/concurrency.md`
  connection establishment; requirements `003_peer-addressing`,
  `004_peer-connectivity`, and `017_peer-roles-and-startup-state`.
- `transport`: `specs/sync.md` peer transports, required operations, error
  semantics, and case sensitivity; `specs/TESTING-GUIDELINES.md`; requirement
  `015_transport-operations`; external standards for host filesystem APIs,
  SSH/SFTP behavior, known-host verification, and Rust streaming abstractions.
- `snapshot`: `specs/database.md`; snapshot lifecycle and replacement rules in
  `specs/sync.md`; snapshot updates and orphan cleanup in
  `specs/multi-tree-sync.md`; requirements `005_snapshot-storage`,
  `006_snapshot-lifecycle`, `009_snapshot-updates`, and
  `016_snapshot-paths-and-timestamps`; external SQLite rollback-journal
  behavior.
- `sync`: `specs/multi-tree-sync.md`; run, operation queue, errors, and case
  sensitivity in `specs/sync.md`; requirements `007_traversal-and-excludes`,
  `008_decision-making`, `009_snapshot-updates`, and the decision portions of
  `017_peer-roles-and-startup-state`.
- `operations`: dry-run, rename compatibility, file copy, displacement, TMP,
  SWAP, and BAK rules in `specs/sync.md`; traversal recovery and cleanup rules
  in `specs/multi-tree-sync.md`; requirements `010_file-transfer-safety`,
  `011_displacement-and-staging-cleanup`, and `012_dry-run-mode`.
- `runtime`: `specs/concurrency.md`; operation queue, logging, and error output
  in `specs/sync.md`; requirements `013_concurrency-controls` and
  `014_logging-and-progress`.

## Root-Owned Shared Contracts

These contracts live at the root because they are consumed by multiple sibling
modules. They should stay behavioral and narrow.

- `RunConfig`: dry-run flag, copy and list retry counts, max active copies,
  timeouts, retention days, verbosity, and command-line excludes. Consumed by
  `peer`, `snapshot`, `sync`, `operations`, and `runtime`.
- `RunRequest`: parsed non-help invocation with peer specs and run config.
  Produced by `cli`; consumed by root startup glue and `peer`.
- `RelPath`: validated slash-separated relative path with no leading slash,
  trailing slash, empty segment, `.` segment, `..` segment, backslash, or NUL.
  Consumed by `cli`, `transport`, `snapshot`, `sync`, and `operations`.
- `PeerSpec`, `PeerUrl`, `PeerRole`, and `PeerId`: parsed peer operands,
  normalized peer identity, canon/subordinate/normal role, and stable per-run
  peer handle identity. Consumed by `cli`, `peer`, `snapshot`, `sync`, and
  `runtime`.
- `PeerSession`: reachable peer state containing normalized identity, selected
  URL, effective role, transport handle, and snapshot-existence metadata.
  Produced by `peer`; consumed by `snapshot`, `sync`, `operations`, and
  `runtime`.
- `TransportRootMode`: selected peer-root construction policy for transport
  connection. `peer` chooses require-existing mode for dry runs and
  create-missing mode for normal runs; `transport` performs the scheme-specific
  check or creation before returning a rooted handle.
- `TransportHandle`: rooted filesystem operation API for listing, stat,
  streaming reads and writes, rename-without-overwrite, create/delete, and
  modification-time updates. Produced by `transport`; consumed by `peer`,
  `snapshot`, `sync`, and `operations`.
- `EntryMeta` and `EntryKind`: listed or stated filesystem metadata containing
  name, file/directory kind, modification time, and byte size, with directories
  represented as byte size `-1`. Produced by `transport`; consumed by
  `snapshot`, `sync`, and `operations`.
- `Timestamp`: UTC `YYYY-MM-DD_HH-mm-ss_ffffffZ` value. The timestamp generator
  implementation belongs to `snapshot`, but the value type is shared by
  `transport`, `snapshot`, `sync`, `operations`, and `runtime`.
- `TransportError`: normalized `not_found`, `permission_denied`, and `io_error`
  categories. Produced by `transport`; consumed by `peer`, `snapshot`, `sync`,
  `operations`, and `runtime`.
- `SnapshotStore`: per-peer local SQLite snapshot API for lookup,
  upsert-present, mark-intended-copy, mark-copy-complete, mark-absent,
  cascade-displaced-directory, stale-row cleanup, download/create-local, and
  upload-through-SWAP. Produced by `snapshot`; consumed by `sync` and
  `operations`.
- `OperationExecutor`: safe mutation API for recover-directory-swaps, displace,
  create-directory, cleanup-retention, and execute-copy-attempt. Produced by
  `operations`; consumed by `sync` and `runtime`.
- `TransferPhase`: one of `read_source`, `write_swap_new`,
  `move_existing_to_swap_old`, `rename_final`, `set_mod_time`, `archive_old`,
  or `cleanup`. Produced by `operations`; consumed by `runtime` diagnostics.
- `CopyTask` and `CopyResult`: source peer/path, destination peer/path, winning
  metadata, attempt result, and failure phase/category. Shared only between
  `sync`, `operations`, and `runtime`.
- `CopyScheduler`: queue and retry API enforcing the global active-copy limit.
  Produced by `runtime`; consumed by `sync`, with workers invoking
  `operations` for each attempt.
- `DiagnosticEvent`, `ProgressEvent`, `DiagnosticSink`, and `ProgressSink`:
  stdout-renderable events and sinks for argument errors, skipped peers,
  listing failures, transfer failures, copy slot changes, active copy progress,
  scanned directory, and completion. Produced across modules and rendered by
  `runtime`; the sinks are consumed by `peer`, `snapshot`, `sync`, and
  `operations`.

No shared contract should include unrelated product behavior. `TransportHandle`
provides filesystem operations but does not decide sync outcomes;
`SnapshotStore` stores and mutates rows but does not choose winners;
`CopyScheduler` limits and retries work but does not know SWAP sequencing.

## Boundary Rationale

The first-layer split is the smallest split that preserves the required sibling
contracts without carving every requirement file into its own implementation
module.

- CLI behavior is a public process surface with exact help text and validation
  output, so it is isolated from sync execution.
- Peer addressing and startup connectivity are separate from lower transport
  operations because fallback URL selection, authentication, and role assignment
  occur before traversal.
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
