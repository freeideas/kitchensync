# decision Module API

The `decision` module has no crate-public API. It is a private implementation
detail of `sync`, and no first-layer sibling module may depend on it directly.
Parent `sync` code remains the public boundary that turns decisions into copy
tasks, inline operations, snapshot updates, recursion choices, diagnostics, and
report accounting.

## Visibility

Rust items in this module must be private by default. Items may use
`pub(super)` only when the parent `sync` implementation needs to pass classified
path facts into the rule engine or inspect the returned outcome. The module
must not expose `pub` items through `kitchensync::sync`.

Sibling modules such as `snapshot`, `operations`, `transport`, and `runtime`
must rely on root-owned contracts and parent `sync` behavior instead of
importing `sync::decision` internals.

## Internal Entry Point

The parent `sync` implementation may rely on one pure decision entry point with
this shape:

```rust
pub(super) fn decide_path(input: ClassifiedDecisionInput) -> DecisionOutcome
```

The function:

- takes ownership of a fully classified path input;
- performs no I/O, scheduling, snapshot mutation, transport calls, operation
  calls, diagnostic rendering, or progress emission;
- never panics for malformed classified facts;
- returns an owned `DecisionOutcome`;
- is deterministic for identical input.

## Internal Input Contract

`ClassifiedDecisionInput` is a private or `pub(super)` Rust record supplied by
parent `sync` classification. It must contain only normalized facts needed by
the decision rules:

- the `RelPath` being decided;
- active peer identities and effective roles for the current scope;
- any active canon peer state;
- active contributing peer live states as file, directory, or absence;
- subordinate peer states needed later by parent dispatch;
- live file comparison metadata: source `PeerId`, path identity, `Timestamp`,
  byte size, and `EntryKind`;
- snapshot-derived file deletion estimates, tombstones, absent-unconfirmed
  history, directory absence history, and no-vote facts for contributing peers;
- skip reasons already determined by traversal or classification;
- validation facts needed to detect contradictory classified input.

Subordinate live entries and subordinate snapshot facts are effect-target data
only. They must not contribute to outcome selection.

## Internal Output Contract

`DecisionOutcome` is a private or `pub(super)` Rust enum that describes the
selected group state as data. It must include variants sufficient to represent:

- canon-selected file, directory, or absence;
- normal non-canon file;
- normal non-canon directory;
- absence selected from deletion or snapshot history;
- no-vote absence;
- non-canon file-over-directory type conflict with the winning file metadata;
- skipped work;
- invalid classified input.

File outcomes must preserve the winning file metadata exactly as classification
supplied it. Directory outcomes must not depend on directory modification time.
Invalid input outcomes must carry narrow reason data for caller-contract
failures, such as multiple active canon peers, canon control without a canon
state, no active contributing peer, or missing file comparison metadata.

The outcome must not contain executors, transport handles, snapshot stores,
copy scheduler handles, diagnostic sinks, progress sinks, callbacks, or mutable
traversal state.

## Ownership Rules

Inputs and outputs are owned values. Borrowed references must not escape the
decision call. The module may clone small root-owned value types such as
`PeerId`, `RelPath`, `EntryKind`, `EntryMeta`, and `Timestamp` when needed to
return an owned outcome.

The module must not normalize paths, re-stat files, generate timestamps, or
look up replacement metadata. Missing or contradictory required facts are
reported through `DecisionOutcome::InvalidInput` rather than repaired inside
`decision`.

## Dependency Limits

The module may depend only on private `sync` records and narrow root-owned value
contracts already imported by `sync`, including `PeerId`,
`EffectivePeerRole`, `RelPath`, `EntryKind`, `EntryMeta`, and `Timestamp`.

It must not depend on `SnapshotStore`, `TransportHandle`,
`OperationExecutor`, `CopyScheduler`, diagnostic or progress sinks, SQLite
schema details, filesystem operation APIs, or runtime scheduling types.
