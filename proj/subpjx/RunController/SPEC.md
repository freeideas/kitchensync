# RunController:

## Purpose

RunController drives a single KitchenSync run from end to end once the command
line has already been parsed into a validated run configuration. It is the one
component that owns the control flow of a run; everything else is a focused
service it calls in order.

Given the validated configuration (the peers with their roles, fallback URLs,
and per-URL settings, plus the global option values including `--dry-run`),
RunController connects the peers, decides whether the run may proceed, drives the
traversal and copy phases, writes updated snapshots back, disconnects, reports
completion, and produces the process exit code. It never parses arguments, never
lists or classifies entries itself, never executes a file copy itself, and never
reads or writes a snapshot row itself. It sequences and gates the services that
do those things, and it threads the dry-run flag through to them.

## Responsibilities

RunController exposes one primary operation across its boundary: run the whole
synchronization for a validated configuration and return the outcome (the
process exit code, and on the error exits the diagnostic message that must be
shown). Concretely, in order, it does the following.

Connect and gather the reachable set:

- Start connection attempts to all peers concurrently rather than strictly one
  peer after another (006.1). The per-peer work of trying the primary URL then
  each fallback, bounding the SFTP handshake, and creating or checking the root
  directory is delegated to the Transport service (`005_connection-establishment`);
  RunController only launches those attempts together and collects which peers
  became reachable.
- Treat a peer whose every URL failed as unreachable, and carry only the
  reachable set forward into the gates, the snapshot work, and the walk. (The
  guarantee that unreachable peers are excluded from listings and decisions and
  that their snapshot rows stay unmodified is enforced by the SyncEngine and
  Snapshot services RunController hands the reachable set to.)

Gate the run on reachability and canon (each gate that fails ends the run before
any traversal):

- When fewer than two peers are reachable, exit 1 (006.2).
- When the designated canon (`+`) peer is unreachable, exit 1 (006.3).
- When no reachable peer has snapshot data and no canon peer is designated,
  surface the message `First sync? Mark the authoritative peer with a leading +`
  (006.4) and exit 1 (006.5).
- After auto-subordination of snapshotless peers (the rule itself is
  `007_peer-roles`), when no contributing (non-subordinate) peer is reachable,
  surface the message `No contributing peer reachable - cannot make sync
  decisions` (006.6) and exit 1 (006.7).

Recover and prepare snapshots:

- Before the walk, have the Snapshot service recover any interrupted SWAP and
  download each reachable peer's snapshot database. The mechanics of SWAP
  recovery, download, and writeback are `016_snapshot-storage`; RunController
  only orders this work ahead of traversal and behind a successful set of gates.

Run the traversal and copy phases:

- Drive the combined-tree walk through the SyncEngine. The interleaving of copy
  work with traversal and the wait for all enqueued copies to finish are owned
  by the SyncEngine and CopyQueue services; RunController orders the walk after
  the gates and snapshot preparation, and treats the walk as complete only once
  SyncEngine reports that traversal and its enqueued copies have finished.

Finish the run:

- In a normal run, write the updated snapshots back to their peers before the
  run exits (006.10); the writeback mechanics are `016_snapshot-storage`. The
  writeback covers every reachable peer, including subordinate ones, so a
  subordinate peer's snapshot is uploaded back updated to reflect the run (007.10).
  RunController is the sole driver of this upload: no other service (the
  SyncEngine included) opens or writes back a snapshot.
- Disconnect all peers, then report completion through the Output service
  (the message format and verbosity are `023_logging`).
- A run that completes all phases exits 0 (006.11).

Dry-run threading (the cross-cutting mode):

- In `--dry-run`, the run is read like a normal run but every peer-mutating step
  is suppressed: no snapshot writeback (the dry-run effect on snapshots is
  enforced inside the Snapshot service), so a subordinate peer's snapshot.db on
  the peer is not updated (007.11); no copies or displacements applied.
  RunController carries the flag into each service it calls rather than
  re-deciding mutation at every call site, so the same orchestration sequence
  serves both normal and dry-run executions.

## Boundaries

Error obligations:

- Each lifecycle gate that fails must end the run deterministically with exit 1
  and, for the two condition-specific messages (006.4 and 006.6), must cause that
  exact message to be shown. RunController is responsible for choosing the exit
  code and the gate message; the Output service is responsible for emitting it.
- The reachability and canon gates are checked before any traversal, snapshot
  download, or copy is started, so a run that cannot make valid decisions never
  mutates a peer.
- A normal run that passes the gates and completes connect, traversal, copy,
  writeback, and disconnect exits 0; any uncompleted required phase is not
  reported as a successful completion.

Invariants:

- RunController is the sole orchestrator: it sequences connect, gate, recover,
  download, walk, writeback, disconnect, and report, and it owns the process exit
  code. It does not implement any of those steps itself.
- Connection attempts overlap (006.1). The walk runs to completion through the
  SyncEngine before writeback begins.
- In a normal run, snapshots are written back before exit (006.10); in
  `--dry-run`, no peer-mutating step (writeback, copy, or displacement) is
  applied.
- RunController does not parse the command line (`001_command-line`), select a
  peer's winning URL or create roots (`005_connection-establishment`), define
  what the canon and subordinate roles mean for decisions (`007_peer-roles`,
  `011_decision-rules`, `012_directory-and-type-decisions`), perform the walk or
  the interleaved copy-drain (`008_traversal`, `006.8`, `006.9`), exclude
  unreachable peers from listings and decisions (`006.12`, `006.13`), read or
  write snapshot rows or move snapshot.db (`013`-`018`), execute copies,
  replacements, or displacements (`019`-`021`), speak any transport protocol
  (`022_transports`), or format log output (`023_logging`). It calls the
  components that own those concerns.

The operation RunController exposes across its boundary is: run the whole
synchronization for a validated configuration, returning the process exit code
and, on a gated error exit, the diagnostic message to show. That outcome is the
shape later jobs build its interface, implementation, and tests against.
