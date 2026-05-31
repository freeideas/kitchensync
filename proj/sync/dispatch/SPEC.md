# dispatch:

## Purpose

Own execution of already-selected sync outcomes by converting them into the
inline operation requests, copy-task submissions, recursion eligibility, and
terminal copy-result consumption needed by the parent `sync` run.

Dispatch applies each outcome to the active peers for the current directory
level, including subordinate peers. It does not classify entries, choose
winners, define snapshot mutation timing, implement safe replacement, enforce
copy-slot limits, retry copies, or render output.

## Responsibilities

- Accept a path-level outcome from the parent sync orchestration with the
  active peer states for that directory, the winning source metadata when a
  file exists, and each peer's live state at the path.
- Apply outcomes only to peers that are active for the current directory
  subtree. Peers that are unreachable, excluded by listing failure for the
  subtree, or under a canon-skipped subtree must not receive operation requests,
  copy tasks, recursion membership, or snapshot-flow notifications from this
  module.
- Apply subordinate peers as targets only. A subordinate peer may receive
  creates, copies, or displacements required to match the selected outcome, but
  its live state must not change the outcome that dispatch receives.
- For a directory-existence outcome, request inline displacement for any
  wrong-type live file at the path before treating that peer as able to keep or
  receive the directory.
- For a directory-existence outcome, request inline directory creation on
  active peers that lack the directory after wrong-type displacement succeeds
  or is unnecessary.
- For a directory-existence outcome, return the child-recursion peer set to the
  parent traversal. Only peers whose directory already existed or whose
  directory creation succeeded may be included for that child path.
- For a directory-absence outcome, request inline displacement for every active
  peer that has a live file or directory at the path. Do not include displaced
  directories in any child-recursion peer set.
- For a file-existence outcome, request inline displacement for any active peer
  that has a live directory at the path before submitting a file copy to that
  peer.
- For a file-existence outcome, submit copy work for every active peer whose
  live file is absent or does not match the winning file by byte size and the
  required five-second modification-time tolerance.
- For a file-existence outcome, do not submit copy work to a peer whose live
  file already matches the winner by byte size and modification-time tolerance.
- For a file-existence outcome, preserve the transport-supplied relative path
  spelling and winning metadata when building `CopyTask` values.
- For an absence outcome, request inline displacement for any active peer with
  a live file or directory at the path, including subordinate-only paths that
  the selected group outcome treats as absent.
- Execute every deletion and type-conflict displacement inline through the
  supplied operation contract. Dispatch must never represent displacement as
  queued file-copy work.
- Submit eligible file-copy work as soon as the parent traversal reaches that
  outcome. Dispatch must not require a full-tree scan before copy work can
  enter the scheduler.
- Notify the snapshot-flow owner at the points required by the parent sync
  contract: intended file copy submitted, successful directory creation,
  successful displacement, and final successful copy result. Dispatch must not
  invent row-update rules of its own.
- Wait for the copy scheduler to close and finish all accepted copy work when
  the parent sync run reaches the end-of-run dispatch phase.
- Consume terminal copy results from the scheduler and return enough normalized
  success and failure information for the parent sync report and snapshot-flow
  updates.
- Pass dry-run context through operation and scheduler calls for creates,
  displacements, and copies. Dispatch still requests those effects in dry-run
  where the normal outcome requires them; peer-side mutation suppression belongs
  to operations and runtime copy execution.

## Boundaries

- Dispatch is a private child of `sync`. It is invoked by parent sync
  orchestration after traversal, excludes, classification, and decision logic
  have selected an outcome for a concrete relative path.
- Dispatch does not list directories, retry listings, recover per-directory
  SWAP state before listing, choose active peer sets for a directory, apply
  excludes, or decide traversal order.
- Dispatch does not inspect snapshot rows to classify live state, compare file
  timestamps for winner selection, evaluate deletion estimates, decide canon
  authority, or resolve file-vs-directory conflicts. It trusts the outcome it
  receives.
- Dispatch does not own snapshot mutation semantics. It may call or notify
  snapshot-flow after dispatch-visible events, but snapshot-flow owns how
  `SnapshotStore` rows are read or mutated.
- Dispatch does not implement filesystem mutation mechanics. Safe file copy
  replacement, SWAP `new` and `old` sequencing, BAK placement, directory
  creation mechanics, retention cleanup, dry-run mutation suppression, and
  transport error normalization belong to operations and transport.
- Dispatch does not own copy scheduling internals. Queue representation, worker
  execution, copy-slot accounting, retry ordering, transfer progress, and trace
  output belong to runtime and operations.
- Dispatch does not render diagnostics or progress. It returns or emits
  structured results through parent-level contracts so runtime can format
  stdout-only output.
- Dispatch must not depend on SQLite schema, path hashes, local snapshot file
  paths, transport-specific error values, concrete queue types, locks, channels,
  async runtimes, or sibling module private data structures.

## Error Obligations

- If displacement fails for a peer, dispatch must treat the live entry as still
  present for that peer. It must not enqueue a replacement copy or include that
  peer in child recursion in a way that assumes the displacement succeeded.
- If directory creation fails for a peer, dispatch must leave that peer out of
  recursion for that child directory and must report the failure without asking
  snapshot-flow to mark the directory confirmed present.
- If a wrong-type directory cannot be displaced before a file-existence
  outcome, dispatch must not enqueue the file copy to that peer for that path.
- If a wrong-type file cannot be displaced before a directory-existence
  outcome, dispatch must not request directory creation over that file for that
  peer.
- Failed inline operations are not retried by dispatch unless the supplied
  operation API reports that it performed its own retry behavior. Dispatch
  records the failure and continues with unaffected peers and paths.
- Copy-attempt failures and retry limits are owned by the scheduler and
  operation executor. Dispatch must consume only terminal copy results, update
  successful-copy state through snapshot-flow, and leave failed copies
  discoverable for a later run through the parent snapshot contract.
- In dry-run mode, dispatch must not skip copy submission, displacement
  requests, or directory-creation requests solely because the run is dry. It
  must rely on the supplied operation and scheduler contracts to perform
  read-only planning behavior.
