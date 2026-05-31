# traversal Architecture

`traversal` is a private child of `sync` that owns the recursive pre-order
combined-tree walk for one prepared sync run. It determines which peers are
active for each directory subtree, performs directory listings concurrently
with configured retry behavior, emits scanned-directory progress, orders live
candidates deterministically, and scopes listing failures to the affected
subtree.

The module is not a public API boundary. Parent `sync` remains responsible for
the exported `run` contract, per-path classification and decision rules,
snapshot mutation timing, operation dispatch, copy scheduling, report assembly,
and diagnostic rendering contracts. `traversal` supplies ordered directory
and candidate observations to those parent-owned flows through private Rust
types only.

## Responsibilities

`traversal` owns:

- recursive pre-order descent from the sync root through live directory
  candidates;
- per-directory active peer sets, including subtree-local removal of peers
  that exhaust listing or pre-listing recovery attempts;
- starting a listing attempt for every peer active at a directory before
  awaiting any listing result;
- retrying directory listing failures according to `RunConfig.retries_list`;
- requesting normal-run user-entry SWAP recovery before listing a directory;
- reporting scanned-directory progress, with the root rendered as `.` by the
  parent progress contract;
- collecting candidate names from successful live listings only;
- applying traversal-level skip rules for canon listing failure and absence of
  contributing listed peers;
- deterministic candidate ordering by case-insensitive lexicographic key with
  the original case-sensitive name as the tie-breaker;
- requesting normal-run BAK/TMP retention cleanup after traversal-owned
  directory processing points.

`traversal` does not choose winners, classify snapshot rows, mutate snapshots
directly, perform concrete file replacement, enqueue copy work, decide output
wording, or expose resumable traversal cursors.

## Data Flow

The parent `sync` run passes `traversal` the run configuration, the prepared
peer/snapshot pairs, operation executor, diagnostic sink, progress sink, and
private callbacks for path decision processing. `traversal` starts with all
reachable peers active at the root directory.

For each directory, `traversal` first reports scan progress and, in normal
mode, asks the operation executor to recover user-entry SWAP artifacts for
each active peer before listing that peer. It then starts all listing attempts
for the directory before consuming results. Failed listings are retried up to
the configured list retry count and are converted into private traversal
failures for the parent to include in `SyncFailure` and diagnostics.

Successful listings produce per-peer live entry maps keyed by listed child
name. Failed non-canon peers are removed only from the current directory
subtree. A failed canon peer causes the whole directory subtree to be skipped:
no candidate decisions, recursion, operation requests, cleanup-driven snapshot
changes, or snapshot row changes are allowed beneath it. If no active
contributing peer remains listed at the directory, subordinate-only entries
are ignored and the subtree is skipped.

After skip checks, `traversal` forms the candidate set from live listings.
Contributing peer names are always included. Subordinate peer names are
included only while at least one contributing peer remains active for that
directory, allowing parent decision logic to displace subordinate-only content
when the selected group state is absence. Exclude filtering is coordinated
with the parent `sync` flow before candidates are classified, decided, recursed
into, or updated in snapshots.

For each ordered candidate, `traversal` hands the parent-owned decision flow
the path, active peer subset, and available live metadata for that candidate.
If the parent determines the candidate is a directory that should be walked,
`traversal` recurses immediately using the subtree-scoped active peer set.
File work and inline operation work may be started by the parent while
traversal continues, but scheduler ownership remains outside this module.

## Dependencies

`traversal` depends only on contracts already imported by `sync`:

- `RunConfig` for dry-run state and listing retry count;
- `SyncPeer`, `PeerSession`, `PeerId`, and effective peer roles for the
  prepared peer set and canon/contributing/subordinate behavior;
- `RelPath` for current directory and candidate paths;
- `EntryMeta`, `EntryKind`, and `TransportError` for listed filesystem state
  and listing failure categories;
- `OperationExecutor` and `OperationError` for traversal-owned SWAP recovery
  and retention cleanup requests;
- `DiagnosticSink`, `ProgressSink`, and parent-owned report builders for
  observable failures and scanned-directory progress.

It must not depend on snapshot database internals, transport implementation
details, concrete safe-replacement sequencing, copy worker internals, or
runtime renderer state. Any helper type that becomes useful outside `sync`
belongs at the nearest shared ancestor instead of in this private module.

## Internal Design

The main internal abstraction is a traversal frame containing the current
directory path and the active peer set inherited by that subtree. Frames are
processed depth-first so parent paths are decided before their children.

Listing state is directory-scoped. It records one result per active peer:
successful child metadata, exhausted listing failure, or exhausted
pre-listing recovery failure. The listing state is discarded after the
directory's ordered candidates have been processed and any allowed recursion
has received its derived active peer set.

Candidate state is also directory-scoped. It contains the selected child name
and the per-peer live metadata observed for that name. It is intentionally not
a vote record or decision plan; parent `sync` logic owns classification,
outcome selection, snapshot mutation rules, and operation or copy requests.

Failure handling is subtree-scoped rather than global. A non-canon peer
failure changes only the active peer set passed into descendants of the failed
directory. A canon failure prevents all work under that directory for every
peer. These rules keep unaffected sibling subtrees available for normal
processing and reporting.

## Leaf Status

This scope should remain a leaf module. Its responsibilities are tightly
coupled around one traversal loop, and carving listing, ordering, recovery, or
active-peer tracking into child modules would expose private intermediate
records without creating a useful sibling contract. Future implementation work
should keep helpers internal to `traversal` unless a contract is needed by
another immediate child of `sync`.
