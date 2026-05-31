# decision:

## Purpose

Own side-effect-free reconciliation for a classified path. This module selects the group outcome as canon result, file result, directory result, absence, type-conflict result, or skipped work using the sync architecture's canon, bidirectional, subordinate, timestamp tolerance, size tie-breaking, deletion estimate, and absence rules.

Decision logic does not list peers, read or write snapshot stores, call transports, request inline operations, enqueue copies, recurse into directories, or emit rendered output. Its stable surface is a pure mapping from classified sync input to an outcome that parent orchestration can dispatch.

## Responsibilities

- Accept one classified candidate path from the sync classifier. The input must
  describe the active contributing peer states for that path, any active canon
  peer state, subordinate target states needed for later dispatch, and the
  snapshot-derived classifications needed for voting.
- Return an owned decision outcome without performing side effects. Outcomes are
  limited to file existence, directory existence, absence, canon-selected file,
  canon-selected directory, canon-selected absence, non-canon type-conflict
  file outcome, or a skipped/invalid input result for caller-contract failures.
- Treat subordinate peers as effect targets only. Subordinate live entries and
  subordinate snapshot rows must never change the selected group outcome.
- Apply canon behavior before all bidirectional rules. When an active canon
  peer is present for the classified path, the canon peer's live file, live
  directory, or absence is authoritative regardless of other peer live state,
  snapshot history, modification time, byte size, or deletion estimates.
- For a canon live file, return a file outcome whose winning source peer is the
  canon peer and whose winning metadata is the canon live file metadata.
- For a canon live directory, return a directory outcome. Directory
  modification time must not influence the result.
- For canon absence, return absence for the group outcome.
- Without a canon peer, ignore directory modification times. Select directory
  existence when any active contributing peer has a live directory and no
  contributing live file creates a file-vs-directory conflict.
- Without a canon peer, select directory absence when no contributing peer has
  a live directory, no contributing live file candidate creates a file outcome,
  and at least one contributing peer has directory snapshot history showing
  absence or tombstone state for the path. Every contributing peer with a
  directory snapshot row must be absent in the current classified input;
  contributing peers with no directory snapshot row do not block this absence
  result.
- Without a canon peer, select absence when no contributing peer has a live
  directory, live file, deletion vote, absent-unconfirmed deletion estimate, or
  relevant snapshot row for the path. This covers paths present only on
  subordinate peers.
- Resolve non-canon file-vs-directory conflicts from contributing peers as a
  file outcome. The winning file must be selected by applying the normal file
  rules to contributing live file candidates only; contributing live directory
  metadata must not participate in the file winner comparison.
- For non-canon file decisions, consider only contributing live file candidates
  and contributing deletion estimates derived by classification. A peer with an
  absent path and no snapshot row has no vote.
- Treat live file candidates classified as unchanged, modified, resurrected, or
  new as existing data candidates. The classifier owns assigning those labels;
  this module owns comparing candidates once supplied.
- Select the winning live file by newest modification time using the required
  five-second tolerance. Any live file candidate whose modification time is
  within 5 seconds of the maximum live modification time is tied for newest;
  any candidate more than 5 seconds older than that maximum loses to it.
- When tied newest live file candidates have different byte sizes, choose the
  candidate with the larger byte size.
- When tied newest live file candidates have the same byte size, choose a
  deterministic winner from the tied candidates without changing the observable
  group state. The selected peer is only the source for later copy work.
- Combine deletion votes by using the most recent deletion estimate among the
  contributing peers that supply one.
- Treat an absent-unconfirmed file classification as a deletion vote only when
  it includes a non-NULL `last_seen` value that is more than 5 seconds newer
  than the newest contributing live file modification time. If `last_seen` is
  NULL, or is not more than 5 seconds newer, it is not a deletion vote.
- When there are no contributing live file candidates, any contributing
  tombstone deletion vote or absent-unconfirmed non-tombstone file history
  selects absence. A contributing peer that is absent with no file snapshot row
  still has no vote.
- Select file absence over live file existence only when the most recent
  deletion estimate is more than 5 seconds newer than the newest contributing
  live file modification time.
- Prefer existing file data over deletion when the deletion estimate and newest
  live file modification time are tied within the five-second tolerance.
- Select absence when deletion votes exist and there are no contributing live
  file candidates.
- Select absence when no contributing peer votes for file existence or
  directory existence and all contributing peers are absent with no snapshot
  row for the path.
- Preserve the winning file metadata exactly as supplied by classification:
  source peer identity, relative path identity, modification time, byte size,
  and file kind. This module must not normalize display names or re-stat files.
- Include enough decision reason data for downstream dispatch and tests to
  distinguish canon outcomes, normal file outcomes, normal directory outcomes,
  deletion/absence outcomes, no-vote absence, and non-canon type-conflict file
  outcomes.
- Be deterministic for identical classified input. The same peers, roles,
  classifications, timestamps, byte sizes, and ordering inputs must produce the
  same outcome.

## Boundaries

- This module owns only per-path reconciliation after traversal and
  classification have determined which peer states are eligible to vote.
- This module does not build directory candidate sets, apply built-in or
  command-line excludes, retry listings, recover SWAP state, scope listing
  failures, or choose whether a subtree is traversed.
- This module does not compare live entries to snapshot rows to produce
  unchanged, modified, new, deletion-vote, absent-unconfirmed, no-vote, or
  directory-history classifications. That is classifier behavior.
- This module does not read `SnapshotStore`, mutate snapshot rows, generate
  timestamps, inspect SQLite schema, compute path hashes, or perform stale-row
  cleanup.
- This module does not decide concrete effects for each peer. It does not
  choose copy targets, request displacements, create directories, update
  destination intended-copy rows, cascade displaced-directory tombstones, or
  decide recursion peer sets.
- This module does not call transport APIs, operation APIs, copy scheduler APIs,
  diagnostic sinks, or progress sinks.
- This module does not implement dry-run mutation suppression. Dry-run has no
  effect on pure outcome selection for the same classified input.
- This module does not own startup role assignment, automatic subordination of
  snapshotless peers, first-sync canon enforcement, no-contributing-peer
  startup failure, or canon reachability checks.
- This module must not expose sibling internals as public `sync` API. Its
  records may remain private to the sync implementation and are consumed by
  sibling sync children only through parent-approved internal contracts.

## Error Obligations

- Decision evaluation must not panic on malformed classified input. If required
  fields are missing or contradictory, return a skipped/invalid input outcome
  that lets the parent sync module report or fail the path without performing
  peer mutations.
- If the input marks more than one active canon peer, return an invalid input
  outcome. Startup should prevent this, but decision logic must fail closed.
- If a canon outcome is requested but no canon live-or-absent state is present
  for the path, return an invalid input outcome rather than falling back to
  bidirectional rules.
- If no active contributing peer is represented and there is no canon state,
  return a skipped/invalid input outcome. Traversal normally skips such
  subtrees before calling decision logic.
- If a file outcome would be required but no contributing live file candidate
  exists, return absence when deletion/no-vote rules allow it; otherwise return
  invalid input rather than inventing a source peer.
- If timestamp or byte-size values needed for comparison are absent from a live
  file candidate, return invalid input. This module must not fetch replacement
  metadata from transports or snapshots.
- Operation, transport, snapshot, and copy failures are outside this module.
  They must be represented before decision as classified absence/skipped input
  or after decision by dispatch/runtime failure handling, not handled directly
  here.
