# classify Architecture

`classify` is a private child of `sync` that builds the normalized decision
input for one surviving candidate path. It combines caller-supplied live entry
metadata with caller-supplied per-peer snapshot lookup results, records
subordinate peers only as possible effect targets, and returns the state
records consumed by sync's private reconciliation rules.

The module is intentionally leaf-sized. It should not be split into child
modules unless future generated source gives it independent storage,
transport, or rule-selection responsibilities.

## Responsibilities

`classify` owns the per-candidate normalization step between traversal and
decision-making:

- accept the candidate relative path, the active peer set for the current
  directory, the preserved transport-supplied basename spelling, live entry
  observations gathered by traversal, and already-looked-up snapshot rows or
  lookup-result values;
- classify active contributing peers as live file, live directory, tombstone,
  absent-unconfirmed file history, absent directory history, deletion-vote, or
  no-vote states as required by sync decision rules;
- identify canon peer state when an active canon peer exists;
- record subordinate peers as destination or displacement targets without
  letting subordinate live entries or snapshot history influence the group
  outcome;
- preserve transport-supplied path spelling and live metadata so later
  operations and copy tasks act on the intended peer path;
- return an owned, deterministic classification record for exactly one
  surviving candidate path.

`classify` does not choose winners, compare file candidates, apply
file-vs-directory conflict rules, schedule copies, request operations, mutate
snapshot rows, apply excludes, list directories, retry transport failures, emit
diagnostics, or recurse into child directories.

## Data Flow

Traversal supplies `classify` with one candidate name that has already
survived exclude filtering and subtree failure rules. For each active peer at
the current directory, the caller also supplies the live listing observation
for that name when present and the corresponding snapshot lookup result when
lookup is allowed. `classify` treats those inputs as closed facts; it does not
list, look up, retry, or recover anything.

`classify` first separates peers by effective role. Contributing and canon
peers are eligible to create group decision input. Subordinate peers are
recorded only in target records that later dispatch code may use when the
chosen group outcome requires a create, copy, deletion, or displacement on that
peer.

For contributing peers, `classify` combines live metadata and snapshot row
state into normalized peer observations:

- live files carry `EntryMeta` file metadata and any matching snapshot context
  needed to distinguish unchanged, modified, resurrected, and new files;
- live directories carry directory existence and preserve directory
  modification time for later snapshot updates, without treating that time as
  decision-significant;
- tombstone snapshot rows with no live entry become deletion-vote facts that
  expose `deleted_time` without choosing an outcome;
- non-tombstone file snapshot rows with no live entry become
  absent-unconfirmed facts that expose the previous `last_seen` without
  deciding whether the absence becomes a deletion vote;
- directory snapshot rows with no live entry become absent directory history;
- peers with no live entry and no snapshot row become no-vote observations.

The result flows only to sync's private decision and dispatch code. Decision
code interprets the record to choose the group outcome. Dispatch code can then
use the subordinate and per-peer target records to request operations, copy
tasks, or snapshot updates through the public contracts owned outside
`classify`.

## Dependencies

`classify` depends only on contracts already visible inside `sync`:

- `RelPath` for the candidate path;
- `PeerId`, `PeerSession`, and `EffectivePeerRole` for peer identity and role;
- `EntryMeta`, `EntryKind`, and `Timestamp` for live metadata and deletion
  estimates;
- `SnapshotRow` and `SnapshotEntryKind` values already returned by snapshot
  lookup.

The module must treat `SnapshotStore` as an already-consumed lookup source. It
may receive snapshot rows or lightweight lookup results, but it must not know
SQLite schema details, path hashes, database paths, or snapshot mutation
methods. It must not call `TransportHandle`, `OperationExecutor`,
`CopyScheduler`, diagnostics, or progress sinks.

## Internal Design

The central internal type should be a classification result for one path. It
should contain:

- the candidate `RelPath` and preserved live name spelling;
- zero or one canon observation;
- contributing observations keyed or indexed by `PeerId`;
- subordinate target records keyed or indexed by `PeerId`;
- compact booleans or counters for common predicates such as "has live file",
  "has live directory", "has deletion vote", and "has unconfirmed absence"
  when these keep decision rules simple.

Peer observations should be typed enums rather than loosely related flags.
This keeps illegal combinations such as a simultaneous live file and tombstone
unrepresentable. Snapshot-derived states should keep only the row facts needed
by downstream rules, such as previous kind, previous live metadata, last-seen
time, and deleted time.

The live-file classifier applies sync's five-second timestamp tolerance only
to decide whether a live file matches an existing non-tombstone file snapshot.
A matching size and modification time produces an unchanged live-file
observation. Otherwise, prior snapshot history produces a modified or
resurrected observation, and the absence of a row produces a new-file
observation. The module does not compare that file against other peers.

Classification should be deterministic for a fixed input. It should not sort
candidate names, because traversal owns candidate ordering, but any peer lists
inside the result should preserve the stable run peer order or use explicit
`PeerId` keys so tie handling remains repeatable.

## Boundary Rules

`classify` is below the public `sync` API and exports no root-visible contract.
Other first-layer modules must not depend on its types or module path. If a
classification record ever needs to cross the `sync` boundary, that means the
shared contract belongs at `sync` or the root ancestor instead of inside this
private module.

The module must stay behaviorally narrow. It may answer "what normalized
states exist for this candidate path?" It must not answer "which state wins?"
or "which effect should be performed?"
