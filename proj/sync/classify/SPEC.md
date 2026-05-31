# classify:

## Purpose

Own the normalized decision input for one surviving candidate path by combining
live entry metadata with per-peer snapshot lookup results from the active peers
at one directory level. Classification converts those raw observations into
typed file, directory, absence, tombstone, deletion-vote, canon, contributing,
and subordinate target records consumed by sync's private reconciliation logic.

This module is side-effect-free. It does not choose winners, request peer
mutations, enqueue copies, mutate snapshot rows, list directories, apply
excludes, recurse into directories, or emit diagnostics.

## Responsibilities

- Accept exactly one candidate `RelPath`, its preserved transport-supplied
  basename spelling, the active peer list for the current directory, the live
  listing observation for that candidate on each active peer when present, and
  that peer's already-looked-up `SnapshotRow` when lookup is allowed.
- Separate peers by effective role before creating decision input. Canon and
  normal peers are contributing decision inputs; subordinate peers are recorded
  only as effect targets.
- For each active contributing peer with a live file, produce a typed live-file
  observation containing the peer id, live `EntryMeta`, and the snapshot facts
  needed to decide whether the file is unchanged, modified, resurrected from a
  tombstone, or new.
- Treat a live file as unchanged only when a non-tombstone file snapshot row
  exists and both byte size and modification time match using sync's required
  five-second tolerance. Otherwise classify it as modified when prior snapshot
  history exists, including resurrection from a tombstone, or new when no row
  exists.
- For each active contributing peer with a live directory, produce a typed
  live-directory observation. Directory modification time must be preserved for
  later snapshot updates but must not be marked as decision-significant.
- For each active contributing peer with no live entry, classify snapshot state
  as one of: tombstone deletion vote with `deleted_time`; absent-unconfirmed
  with the previous `last_seen`; absent directory history; or no-vote when no
  row exists.
- Expose absent-unconfirmed file state without deciding whether it becomes a
  deletion vote. The later decision step owns comparing `last_seen` with the
  newest live file modification time.
- Identify the canon peer observation, when a canon peer is active, without
  comparing it against other peers.
- Preserve enough per-peer facts for downstream snapshot flow to update rows
  after confirmed present observations, intended copies, confirmed absences,
  successful directory creation, successful displacement, and completed copies.
- Return an owned, deterministic classification record for the candidate path.
  Peer records must preserve the stable run peer order or use explicit
  `PeerId` keys so downstream tie handling is repeatable.

## Boundaries

`classify` depends only on contracts already visible inside `sync`: `RelPath`,
`PeerId`, `PeerSession`, effective peer roles, `EntryMeta`, `EntryKind`,
`Timestamp`, `SnapshotRow`, and `SnapshotEntryKind`.

The module may receive snapshot rows or lookup-result values, but it must not
call `SnapshotStore` mutation methods and must not depend on SQLite table
names, row ids, path hashes, local database paths, or snapshot upload/download
lifecycle details.

The module must not call `TransportHandle`, `OperationExecutor`,
`CopyScheduler`, diagnostic sinks, progress sinks, or runtime output code. It
must not perform listing retries, SWAP recovery, BAK/TMP cleanup, directory
creation, displacement, file transfer, copy retry accounting, or dry-run
mutation suppression.

Traversal owns the candidate set, exclude filtering, directory ordering,
subtree failure handling, and recursion. Decision owns canon authority,
file-vs-directory conflict resolution, newest-file selection, deletion versus
existence comparison, size tie-breaking, and absence outcomes. Dispatch and
snapshot-flow own the effects and snapshot mutations that follow a decision.

Subordinate peer live entries and snapshot history must never influence the
group outcome through this module. They may appear only in subordinate target
records so later dispatch can make those peers match the selected outcome.

## Error Obligations

Classification has no transport or filesystem error surface. Listing,
pre-listing recovery, and snapshot lookup failures must already have been
handled or represented by the caller before this module is invoked.

If the caller supplies inconsistent in-memory input, such as duplicate records
for one peer, a peer role not represented by the active peer list, a file live
entry paired with directory-only metadata, or multiple canon peers, the module
must return an internal classification error to the parent sync orchestration
instead of silently inventing a state. It must not panic for ordinary malformed
caller input.

Classification must not convert missing live entries into peer deletions by
itself. It may expose tombstone and absent-unconfirmed facts; the decision step
is responsible for determining whether those facts select deletion, copy, or no
work.
