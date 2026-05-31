# decision Architecture

The `decision` module owns the side-effect-free reconciliation rules for one
classified sync path. It receives already-classified peer state for a path and
returns the selected group outcome for the parent `sync` orchestration to
dispatch.

This module is private implementation inside `sync`. It does not expose a
public product API, and sibling first-layer modules must not call it directly.

## Responsibilities

`decision` selects one outcome for a classified path:

- canon file, directory, or absence result when an active canon peer controls
  the path;
- non-canon file result using modification-time tolerance, size tie-breaking,
  existing-data preference, and deletion estimates;
- directory result when contributing live directories establish existence;
- absence when contributing state and snapshot history establish deletion or no
  group entry;
- type-conflict result according to the file-over-directory conflict rule;
- skipped work when the classified input says the parent traversal cannot make
  a decision for the path;
- invalid input when required classified facts are missing or contradictory.

The module does not list peers, read or write snapshot stores, call transports,
request inline operations, enqueue copies, recurse into directories, or render
diagnostics. Those effects remain owned by parent `sync` traversal, snapshot
flow, dispatch, operations, and runtime contracts.

## Inputs

The parent `sync` implementation provides a private classified-path record.
That record contains only normalized facts needed for the rule set:

- the relative path being decided;
- active peer ids and effective roles for the current directory scope;
- whether a canon peer is active for the path;
- per-peer live state as file, directory, or absence;
- file metadata needed for comparisons: modification time and byte size;
- snapshot-derived absence and deletion-estimate facts for contributing peers;
- subtree or path skip reasons already determined by traversal failure rules;
- enough validation state to detect caller-contract failures, such as multiple
  active canon peers, canon control without a canon live-or-absent state, no
  active contributing peers, or live file candidates missing comparison
  metadata.

Subordinate peer facts may be present so the returned outcome can be applied to
them by the parent, but subordinate peers never contribute live entries or
snapshot history to the selected group outcome.

## Outputs

The module returns a private decision result that identifies the selected group
state and the winning source facts needed by dispatch. The result is data only:
it may name the winning source peer, winning file metadata, directory
existence, confirmed absence, type conflict, skip reason, or invalid-input
reason.

Decision reason data is part of the output contract. It must let parent `sync`
code and tests distinguish canon outcomes, normal file outcomes, normal
directory outcomes, deletion/absence outcomes, no-vote absence, non-canon
type-conflict file outcomes, skipped work, and invalid classified input.

The result must not contain operation executors, scheduler handles, snapshot
store handles, transport handles, diagnostic sinks, or mutable traversal state.
Parent `sync` code translates the result into inline operations, copy tasks,
snapshot mutations, recursion decisions, diagnostics, and report accounting.

## Rule Flow

Decision evaluation is deterministic and ordered by rule precedence:

1. Validate the classified input. Contradictory or incomplete facts return an
   invalid-input outcome rather than panicking or falling back to another rule.
2. If the classified input is marked skipped, return the skip outcome without
   evaluating peer votes.
3. If an active canon peer exists, return the canon peer's live file, live
   directory, or absence as the authoritative outcome.
4. If contributing live files and directories conflict without canon control,
   resolve the group outcome to a file and choose the winning file using normal
   non-canon file rules.
5. For non-canon file candidates, choose the newest contributing live file by
   modification time, treating timestamps within five seconds as tied. For tied
   live files with different sizes, choose the larger file. When deletion and
   existing data tie, keep existing data. A deletion estimate wins only when it
   is more than five seconds newer than the newest live file candidate.
6. For directory candidates without a file-conflict override, ignore directory
   modification time. Any contributing live directory selects directory
   existence.
7. If no contributing live entry selects existence and snapshot facts establish
   absence or tombstones, return absence.
8. If no contributing peer has a live vote or snapshot row for the path, return
   absence.

The module should keep comparison helpers small and local to this rule flow so
future tests can exercise canon precedence, timestamp tolerance, size
tie-breaking, deletion estimates, absence, and type conflicts independently.

## Dependencies

`decision` may depend only on private `sync` data structures and the narrow
root-owned value contracts already imported by `sync`, such as `PeerId`,
`EffectivePeerRole`, `RelPath`, `EntryKind`, `EntryMeta`, and `Timestamp`.

It must not depend on `SnapshotStore`, `TransportHandle`,
`OperationExecutor`, `CopyScheduler`, diagnostic or progress sinks, SQLite
schema details, filesystem operation APIs, or runtime scheduling types. If a
new rule needs extra facts from those systems, the parent `sync` classifier
must convert them into plain classified input before calling `decision`.

## Internal Design

The implementation should be a small pure rule engine:

- a classified input type owned by parent `sync` or a nearby private classifier;
- a compact outcome enum for file, directory, absence, type conflict, and skip;
- an invalid-input outcome variant with narrow reason codes for caller-contract
  failures;
- helper functions for canon selection, file winner comparison, deletion
  comparison, directory existence, and absence fallback;
- focused unit tests that build classified inputs directly and assert returned
  outcomes.

No helper should perform external I/O, mutate snapshots, enqueue work, emit
events, or inspect traversal stacks. Rule helpers should return values rather
than callbacks or executable plans.

## Visibility

All types in this module are private to `sync` unless parent `sync`
implementation needs `pub(super)` visibility to share them with neighboring
private modules. Nothing here is part of the `kitchensync::sync` public API.

Parent `sync` remains the boundary that converts decisions into public
observable behavior through `run`, `SyncReport`, diagnostics, operations, copy
scheduling, and snapshot updates.

## Leaf Scope

This scope should remain a leaf. The rule set is narrow, pure, and cohesive;
splitting it into child modules would make the decision precedence harder to
audit without creating a useful independent contract.
