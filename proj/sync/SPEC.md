# sync:

## Purpose

Own the combined-tree traversal and reconciliation decision engine for one
KitchenSync run. The module lists active peers at each directory level, applies
built-in and command-line excludes, classifies live entries against per-peer
snapshot rows, chooses file, directory, deletion, type-conflict, canon, and
subordinate outcomes, updates snapshot state at the required decision points,
and requests inline operations or queued copies for the effects needed to make
active peers match the selected group outcome.

The module must be concrete enough that a run can be tested as a recursive
combined-tree walk without depending on transport internals, SQLite internals,
safe-replacement internals, or runtime rendering internals.

## Responsibilities

- Expose a traversal API that accepts root-owned run configuration, connected
  peer sessions, snapshot stores, an operation executor, a copy scheduler,
  diagnostic output, and progress output, then returns whether traversal and all
  copy work completed without unrecovered failures.
- Treat the peer set passed in by startup as the only reachable peers for the
  run. Unreachable peers are not listed, do not vote, do not receive operations,
  and their snapshot stores are not modified by this module.
- Traverse from the sync root using a single recursive combined-tree walk.
  Traversal is pre-order: for each directory, decide and act on every entry in
  that directory before recursing into any child directory.
- Before listing a directory in a normal run, ask operations to recover
  user-entry SWAP state for each peer still participating at that directory
  level. In dry-run mode, skip peer-side SWAP recovery.
- List the current directory on every peer participating at that directory
  level concurrently. Start each listing before awaiting any listing result.
- Retry each failed directory listing up to `RunConfig.retries_list` total
  attempts. Listing retries apply to the same peer and same relative directory.
- Treat a failed SWAP recovery for a peer at a directory as that peer's listing
  failure for the same directory and apply the normal listing-failure rules.
- If a non-canon peer exhausts listing retries, log an error-level diagnostic,
  exclude that peer from decisions and operations for that directory and its
  whole subtree, and leave that peer's snapshot rows under the failed subtree
  unchanged.
- If the canon peer exhausts listing retries, log an error-level diagnostic and
  skip decisions, operations, copies, recursion, cleanup-driven snapshot
  changes, and snapshot row changes under that directory subtree for every peer.
- If all active contributing peers fail listing for a directory, skip decisions
  for that directory and its whole subtree. Do not process subordinate-only
  entries under that subtree.
- Build each directory's candidate entry set from live listing names only.
  Snapshot-only rows must not add names to the traversal set.
- Include listed names from active contributing peers in the candidate set.
  When at least one active contributing peer remains, also include listed names
  from active subordinate peers so subordinate-only paths can be displaced when
  the group outcome is absence.
- Remove excluded entries from the candidate set before snapshot lookup or
  decision-making. Built-in excludes are `.kitchensync/`, `.git/`, symbolic
  links, special files, and other non-regular entries omitted by transport
  listing/stat behavior. Command-line excludes are `RunConfig.excludes`.
- Apply command-line excludes by exact relative path for files and by subtree
  prefix for directories. Excluded entries are treated as nonexistent for this
  run, are not copied, created, displaced, deleted, recursed into, or used for
  snapshot updates, and existing peer contents at excluded paths are left
  untouched.
- Process candidate entries in deterministic case-insensitive lexicographic
  order, using the original case-sensitive name as the tie-breaker. Preserve the
  filenames supplied by transports when requesting operations and copies.
- For each candidate path, gather live state and snapshot rows from active
  contributing peers for voting. Subordinate peers must not contribute live
  entries or snapshot history to decisions.
- Apply canon behavior before normal bidirectional conflict rules. When a canon
  peer is active for a directory, the canon peer's live file, live directory, or
  absence at a candidate path is the authoritative group outcome for that path.
- For file decisions without a canon peer, classify each contributing peer as
  unchanged, modified, new, deletion vote, absent-unconfirmed, or no-vote using
  that peer's live state and snapshot row.
- Treat a live file with a non-tombstone snapshot row as unchanged only when
  byte size matches and modification time is within 5 seconds of the snapshot
  row. A size mismatch, a modification-time difference greater than 5 seconds,
  a tombstone row, or no row classifies the live file as changed input according
  to the source rules.
- Treat an absent file with a tombstone row as a deletion vote using
  `deleted_time`. Treat an absent file with a non-tombstone row as
  absent-unconfirmed. Treat an absent file with no row as no-vote.
- For absent-unconfirmed files, use `last_seen` as a deletion estimate only
  when it is non-NULL and more than 5 seconds newer than the newest live file
  modification time. Otherwise, do not count it as a deletion vote; if an
  existing file outcome wins, the absent-unconfirmed peer becomes a copy target.
- Select a file winner from live contributing candidates by newest
  modification time with 5-second tolerance. Candidates within 5 seconds of the
  maximum modification time tie; among tied candidates with different sizes,
  choose the larger file. Prefer existing data over deletion on ties.
- When deletion estimates compete with live file candidates, compare the most
  recent deletion estimate against the newest live candidate modification time
  using the 5-second tolerance. Deletion wins only when the deletion estimate is
  more than 5 seconds newer.
- If at least one contributing peer has a deletion vote for a file path and no
  contributing peer has a live file candidate for that path, select absence as
  the group outcome.
- If no contributing peer votes for a file path because all contributing peers
  are absent with no snapshot row, select absence as the group outcome.
- For directory decisions, ignore directory modification time. If any active
  contributing peer has a live directory, select directory existence. If no
  contributing peer has a live directory and contributing snapshot rows prove
  absence or tombstones, select directory absence. Contributing peers with no
  directory snapshot row do not block deletion. If no contributing peer has a
  live directory or any snapshot row for the path, select absence as the group
  outcome.
- Resolve file-vs-directory conflicts through the canon peer when present.
  Without a canon peer, a conflict between contributing file and directory
  states resolves to a file outcome; select the winning file by applying the
  normal file rules to contributing live file entries only.
- Apply every selected group outcome to all active peers at that directory
  level, including subordinate peers. A subordinate peer receives creates,
  copies, and displacements required to match the group outcome but never
  changes the group outcome.
- For a directory-existence outcome, displace wrong-type entries inline before
  creating or keeping the directory. Ask operations to create missing
  directories inline. Only peers where the directory exists or was successfully
  created participate in recursion into that child directory.
- For a directory-absence outcome, ask operations to displace any live entry at
  that path inline. Do not recurse into a directory on a peer after deciding to
  displace it on that peer.
- For a file-existence outcome, record listed source states in snapshots, ask
  operations to displace wrong-type directories inline, enqueue copies for
  peers that lack the file or whose live file does not match the winner by byte
  size and 5-second modification-time tolerance, and avoid enqueuing copies to
  peers whose live file already matches.
- For an absence outcome, ask operations to displace any live file or directory
  at that path inline. This includes subordinate-only paths when no
  contributing peer votes for existence.
- Execute all deletion and type-conflict displacement inline through
  operations. Never place displacement work in the file-copy queue.
- Enqueue file-copy work as soon as traversal finds eligible work. Do not wait
  for the full tree scan to finish before allowing the copy scheduler to run.
- Wait for the copy scheduler to finish all queued copy work before returning a
  successful run result to the root.
- During normal traversal, after processing the candidate names for a directory,
  ask operations to perform BAK/TMP retention cleanup for each active peer at
  that directory. In dry-run mode, skip peer-side BAK/TMP cleanup.
- Start or request opportunistic snapshot stale-row cleanup without delaying
  the first directory scan or the first eligible copy. Correct traversal and
  decisions must not depend on cleanup finishing in the current run.
- Report the currently scanned directory to progress output. The root directory
  is reported as `.`; other directories use slash-separated relative paths.
- Emit diagnostics for listing failures and decision-level skipped work through
  the diagnostic sink using stdout-renderable events. Formatting belongs to
  runtime.

## Snapshot Obligations

- Read snapshot rows through `SnapshotStore` interfaces. The sync module must
  not know SQLite table mechanics, path hashing implementation details, or
  local temporary database paths.
- Update snapshot rows only for peers and paths whose live state has been
  observed or whose operation/copy outcome has reached the required point.
- When an entry is confirmed present by listing, upsert that peer's row with
  current `mod_time`, `byte_size`, fresh `last_seen`, and `deleted_time = NULL`.
- When a decision schedules a file copy to a destination peer, upsert the
  destination row with the winning `mod_time`, winning `byte_size`, and
  `deleted_time = NULL`, but do not update `last_seen` before the copy
  succeeds.
- When the copy scheduler reports a successful copy, set the destination row's
  `last_seen` to a fresh current timestamp. This is the only post-traversal row
  update owned by sync.
- If a queued copy does not finish successfully, leave the destination row's
  `last_seen` unchanged and keep `deleted_time = NULL` so the next run can
  treat the path as absent-unconfirmed.
- After inline directory creation succeeds, mark the directory confirmed
  present for that peer with fresh `last_seen`.
- If inline directory creation fails, leave that peer's existing row unchanged.
- When an entry is confirmed absent and the peer has an existing non-tombstone
  row, set `deleted_time` to that row's previous `last_seen` and do not update
  `last_seen`.
- When an entry is confirmed absent and the peer already has a tombstone row,
  leave the row unchanged.
- After successful displacement, mark the displaced entry deleted for that
  peer. When the displaced entry is a directory, request the snapshot store's
  same-peer subtree cascade so non-tombstone descendants reachable through
  `parent_id` receive the same deletion estimate.
- If displacement fails, leave that peer's snapshot row and descendants
  unchanged.
- Never update snapshot rows for excluded paths, unreachable peers, peers
  excluded from a failed listing subtree, or any peer under a directory skipped
  because canon listing failed.

## Boundaries

- The sync module owns traversal order, peer visibility at each subtree,
  exclude application, entry classification, conflict decisions, and the timing
  of calls into snapshot, operations, runtime, diagnostics, and progress.
- The sync module does not parse CLI arguments, validate peer operands, choose
  fallback URLs, connect peers, auto-subordinate snapshotless peers, enforce
  first-sync startup failures, download snapshots, upload snapshots, disconnect
  peers, or map process exit codes.
- The sync module does not implement local or SFTP filesystem operations. It
  observes only root transport contracts and normalized transport error
  categories exposed through peer sessions and operation results.
- The sync module does not implement SQLite schema, path hashing, timestamp
  generation, snapshot SWAP recovery/upload, or physical snapshot file
  lifecycle. It uses `SnapshotStore` behavior supplied by the snapshot module.
- The sync module does not implement safe file replacement, SWAP path
  sequencing, BAK/TMP path construction, BAK/TMP purge mechanics, or dry-run
  suppression of peer-side mutations inside individual operations. It asks
  operations to do those effects and responds to success or failure.
- The sync module does not own copy-slot accounting, retry scheduling for
  queued file copies, transfer progress rows, trace copy-slot output, or the
  concrete queue representation. It creates copy tasks and waits on the
  scheduler's completion/failure results.
- The sync module does not render terminal output. It emits structured
  progress and diagnostic events; runtime decides verbosity filtering and
  interactive or line-oriented rendering.
- The sync module must not match on transport-specific error values, filenames
  normalized for display, or sibling-module internal data structures.

## Error Obligations

- Directory listing failures are subtree-scoped. After retry exhaustion, the
  affected peer is removed only from that directory subtree for the current run;
  it may participate elsewhere in the same run where it has not failed, and it
  participates normally on later runs if listing succeeds.
- A canon listing failure is authoritative unavailability for that subtree. No
  other peer may supply decisions there, and no peer files or snapshot rows may
  be changed under that subtree by sync.
- A failed user-entry SWAP recovery before listing is handled exactly like a
  listing failure for that peer and directory.
- Operation failures are not retried by sync unless the called operation API
  explicitly defines retry behavior. Sync logs or propagates the failure
  through diagnostics, skips the dependent action for that peer/path, and
  preserves snapshot rows unless the operation reported success.
- Copy failures and copy retry limits are owned by runtime and operations. Sync
  must consume final copy results so successful copies update destination
  `last_seen` and failed copies remain discoverable on a future run.
- A failed displacement must leave the live entry in place for this run and
  must prevent sync from recursing into or replacing that path on that peer in a
  way that assumes the displacement happened.
- A failed directory creation must keep that peer out of recursion for the new
  directory and must leave its directory snapshot row unchanged.
- In dry-run mode, sync still performs traversal, decision-making, local
  snapshot updates, copy enqueueing, copy-slot exercise, and source reads via
  the scheduler, but it must not request peer-side SWAP recovery or BAK/TMP
  cleanup. Peer-side mutation suppression for copies, creates, and
  displacements is enforced by operations, and sync must call those APIs with
  the dry-run run configuration.
