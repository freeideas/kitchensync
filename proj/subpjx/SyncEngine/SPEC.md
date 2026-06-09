# SyncEngine:

The decision driver of a KitchenSync run. It performs the single recursive walk
over all peer trees, decides what should happen to every entry, and carries out
the directory creates and BAK displacements inline while enqueuing every file
copy into the copy queue.

## Purpose

KitchenSync syncs a group of peer file trees toward one agreed state. SyncEngine
owns the part of that work that decides what the agreed state is at each path and
drives the tree walk that reaches every path. It does not move file bytes itself
and it does not talk to a network: it reads each peer's live listing and snapshot
rows through the transport and snapshot services, applies the spec's
classification and decision rules, and turns each decision into one of a small
set of concrete actions -- create a directory, displace an entry to BAK,
enqueue a file copy, or do nothing. The run controller calls SyncEngine once per
run to perform the whole traversal-and-decision phase.

## Responsibilities

### The combined-tree walk

- Operate only on the reachable peer set the run controller hands to SyncEngine.
  A peer the controller could not reach at connect time is never part of any
  listing union or sync decision, and SyncEngine triggers no snapshot update that
  would touch its rows, so its snapshot stays exactly as it was (006.12, 006.13).
  (This whole-run exclusion is distinct from a reachable peer whose listing of one
  directory fails its allowed tries, covered under the subtree invariants below.)
- Drive one recursive, pre-order walk over the N peer trees rooted at each peer's
  sync prefix. At each directory level, list every reachable peer's directory in
  parallel through the transport, then build the union of live entry names.
  Contributing (canon and non-subordinate) peers drive the union; subordinate
  peers' names are included only so non-conforming entries can be cleaned up; the
  snapshot never contributes a name that no peer still has live (008.3, 008.4,
  008.5).
- Process the entries of a directory in case-insensitive lexicographic order,
  breaking ties by the original case-sensitive name, and finish acting on every
  entry in a directory before entering any subdirectory of it (008.1, 008.2).
- Preserve every entry name exactly as the filesystem reports it; never change
  case or characters. Syncing between case-sensitive and case-insensitive peers
  may therefore collapse or duplicate case-only variants, which stay recoverable
  from BAK (008.16).
- Recurse into a kept directory only on the peers that keep it; a directory
  chosen for displacement on a peer is moved as a single subtree rename and is
  not recursed into on that peer (008.7, 008.8, 008.9).
- Run displacement inline during the walk, not through the copy queue, so a
  displacement needed before a copy into the same path (a type conflict) finishes
  before that copy is enqueued and the copy succeeds within the same run (008.6).
- Enqueue each file copy into the copy queue as the walk reaches and decides its
  entry, so copy work for an already-scanned directory proceeds while traversal
  continues into later directories. There is no separate phase that scans the
  whole tree before any copy starts; the single streaming walk both decides and
  enqueues (006.8).
- Enqueue every file copy the run needs during this one walk. SyncEngine returns
  only after the entry union at every reachable directory has been decided and
  every needed copy has been enqueued, so that the run can drain all enqueued
  copies before it exits (the copy queue performs the draining) (006.9).
- After processing the union of entry names at a directory level, in a normal run
  inspect each peer's `.kitchensync/` directory at that path so the copy queue can
  perform its opportunistic BAK/TMP maintenance there. This inspection is the only
  point in the walk that reaches inside `.kitchensync/`, which is otherwise
  excluded from listings; it is skipped entirely under dry-run (021.9).

### Excludes

- Remove built-in excludes from the entry union before deciding anything:
  `.kitchensync/` directories, `.git/` directories, symbolic links, and special
  files. (Symbolic links and special files are already omitted by the transport;
  SyncEngine treats their absence from listings as the observable exclusion and
  never decides on them.) (009.1, 009.2, 009.3, 009.4)
- Apply each command-line `-x <relative-path>` exclude in addition to the
  built-in set; `-x` can add exclusions but cannot include or override a built-in
  one (009.5, 009.6).
- Treat an excluded path as nonexistent for the run: do not scan, copy, delete,
  or displace it; do not consult or update its snapshot row; leave any existing
  excluded entry untouched on every peer. An excluded directory removes its whole
  subtree from the walk (009.7, 009.8, 009.9).

### Peer roles

- Honor the canon (`+`) and subordinate (`-`) roles supplied for the run. At most
  one canon peer exists; when it and another peer differ on a file, the canon
  version wins and is propagated to the group (007.1).
- Keep subordinate peers out of decisions: their live entries never enter the
  state set used to pick a winner, so the group outcome is identical to the
  subordinate peer being absent. After the contributing decision is made, conform
  each subordinate peer to it -- copy files and create directories it lacks,
  displace files and directories it has that the group's state does not include
  (007.2, 007.3, 007.4, 007.5, 007.6).
- Treat any peer with no `.kitchensync/snapshot.db` as subordinate unless it is
  the canon peer, so a brand-new peer receives the group's state without
  influencing it; an explicit `-` on such a peer changes nothing (007.7, 007.8,
  007.9).
- SyncEngine records snapshot rows for a subordinate peer exactly as for a
  contributing peer, so the snapshot the run controller later writes back reflects
  the run for every reachable peer. SyncEngine itself neither downloads nor uploads
  a snapshot database -- that download/upload lifecycle belongs to the run
  controller (006.10) and the snapshot service (016), not here -- which is why a
  peer that was subordinate on one run participates normally on a later run that
  omits `-` (007.12). The upload itself, and its suppression under `--dry-run`,
  are the run controller's obligations (007.10, 007.11).

### Per-peer entry classification

- For each contributing peer and each file entry, classify the peer's live state
  against its own snapshot row into exactly one of: unchanged (live, row present
  and not deleted, mod_time within 5 seconds and byte_size equal), modified (live
  but mod_time or byte_size differs, including resurrection over a tombstoned
  row), new (live, no row), deleted (absent, row tombstoned), absent-unconfirmed
  (absent, row present with NULL `deleted_time`), or no-opinion (absent, no row)
  (010.1-010.8).
- Apply the 5-second tolerance when comparing a peer's live mod_time to its row's
  mod_time, and treat a file as unchanged only when both mod_time and byte_size
  match (010.1, 010.2, 010.3).

### File decision rules

- With a canon peer present, the canon state wins unconditionally: a file canon
  has is copied to every other peer including subordinates; a file canon lacks is
  deleted from every other peer (011.1, 011.2).
- Without a canon peer, resolve the contributing peers' classifications into one
  outcome per path: all-unchanged-and-matching performs no copy among matching
  peers but conforms peers that lack the file; modified and new resolve by newest
  mod_time; a deletion wins over an existing file only when the deletion estimate
  (`deleted_time`, or `last_seen` under the absent-unconfirmed rule) is later than
  the existing file's mod_time by more than 5 seconds, otherwise the file is kept
  and copied to peers that lack it; equal mod_time with differing byte_size lets
  the larger file win; ties keep data (011.3-011.12).
- A peer with no snapshot row casts no vote on which version wins but still
  receives the decided winner; apply the 5-second tolerance when comparing
  mod_times and deletion estimates to the maximum, and enqueue no copy to a peer
  that already matches the winner (mod_time within tolerance and equal byte_size)
  (011.13-011.17).

### Directory and type-conflict decisions

- Decide directories by existence, never by mod_time: if any contributing peer
  has the directory live, create it on every active peer that lacks it; if none
  has it live but at least one has a row and every such peer is now absent,
  displace it everywhere and let the snapshot service tombstone the row; a
  contributing peer with no row neither votes nor blocks deletion; if no
  contributing peer has it live or as a row, displace it from subordinate peers
  that still have it. Canon overrides as usual (012.1-012.7).
- Resolve a path that is a file on one peer and a directory on another: with
  canon present, the canon type wins and the conflicting type is displaced on the
  others; without canon, the file wins -- displace each contributing peer's
  conflicting directory first, then pick the winning file by the normal file
  rules and sync it. A subordinate peer's type never influences the contributing
  decision and is conformed afterward (012.8-012.17).

### Inline displacement to BAK

- Perform the displacement operation the decisions call for: before renaming an
  entry, create `<parent>/.kitchensync/BAK/<timestamp>/` and any missing parents
  through the transport, then rename `<parent>/<basename>` to
  `<parent>/.kitchensync/BAK/<timestamp>/<basename>`. A directory is displaced as
  a single subtree rename. The BAK directory sits under `.kitchensync/` at the
  displaced entry's own parent level, never aggregated at the sync root
  (021.1-021.4).
- On a displacement rename failure, log an error-level diagnostic through the
  output service and skip the displacement, leaving the entry in place at its
  original path; the walk continues (021.5, 021.6).

### Operations exposed across the boundary

- A single `run`-style entry point that, given the connected peers, their roles,
  the per-peer sync prefixes, the resolved excludes, the relevant option values
  (for example `--retries-list` and `--dry-run`), and handles to the transport,
  snapshot, copy queue, and output services, performs the whole traversal and
  decision phase and returns when every entry has been decided and every file
  copy has been enqueued.

### Construction and the hidden helpers

- SyncEngine is split internally into private helpers it owns and builds itself:
  the per-path decision-rules helper and the displacement helper. These helpers
  are an implementation detail of SyncEngine, not part of its public surface.
- The function that creates a SyncEngine instance takes exactly four parameters,
  one per shared sibling service it depends on, and no others: the copy queue, the
  output service, the snapshot service, and the transport service. Its parameter
  list is precisely those four service handles. It does not, and must not, accept
  a decision-rules helper or a displacement helper as a parameter.
- Inside that constructor, SyncEngine builds its own helpers by calling each
  helper subproject's own factory function -- the decision-rules helper's
  constructor and the displacement helper's constructor -- and stores the results
  on the engine. The displacement helper itself needs the transport and output
  services, so SyncEngine passes those through to it when it builds it. The caller
  hands SyncEngine only the four services and never names, imports, or constructs
  either helper.
- No parameter or return type of any public SyncEngine operation, and no
  parameter of its constructor, is a type that belongs to the decision-rules or
  displacement helper. Those helper types stay entirely behind the SyncEngine
  boundary; a consumer such as the run controller compiles and links against
  SyncEngine without ever depending on, importing, or referring to the
  decision-rules or displacement helper.

## Boundaries

- SyncEngine never moves file bytes between peers. It decides copies and enqueues
  them into the copy queue (`:copyqueue`); the copy queue executes them under the
  global slot limit, performs SWAP-staged replacement, and archives replaced
  files into BAK. SyncEngine performs only directory creates and inline BAK
  displacements itself, through the transport.
- SyncEngine reads peer listings and metadata and performs renames, directory
  creates, and BAK setup only through the transport service (`:transport`); it
  never branches on URL scheme and never opens connections. Connecting,
  reachability and canon gating, snapshot download/upload, copy-phase
  orchestration, and disconnect belong to the run controller, which calls
  SyncEngine once per run.
- SyncEngine reads snapshot rows through the snapshot service (`:snapshot`) for
  classification and decisions and triggers the snapshot updates that record a
  run, but it does not own the snapshot schema, path hashing, timestamps,
  tombstone-and-cascade mechanics, or the snapshot database files; those belong
  to the snapshot service.
- All progress, error, and info output goes through the output service
  (`:output`) so that stdout carries every line and stderr stays empty; SyncEngine
  does not format or write output directly. The `C`/`X` progress line content and
  verbosity rules are owned by the output and logging concern.
- Dry-run is honored by threading the flag into the operations that mutate a peer
  (directory create, displacement) and into the copies it enqueues, suppressing
  the mutation while still reading and deciding normally. Under `--dry-run`
  SyncEngine still lists peer directories so it can decide and report what would
  happen (024.4), but it creates no directory on any peer (024.12), displaces no
  entry to BAK so no destination file is moved aside (024.15), and the deletions
  it decides remove no destination file because they are carried out only as
  suppressed displacements (024.16). The full set of observable dry-run guarantees
  across the run is owned by the run lifecycle and the components that touch peers.
- SyncEngine owns no command-line parsing; it receives already-validated option
  values and resolved excludes from its caller. It defines no persistent global
  state and is created per dependent.
- Invariants SyncEngine maintains: a directory is decided and fully acted on
  before any of its subdirectories is entered; a displaced directory is never
  recursed into on the peer where it was displaced; an excluded path is never
  scanned, mutated, or recorded; a peer whose listing failed all allowed tries
  has nothing created, deleted, displaced, or copied under that subtree and none
  of its snapshot rows for that subtree modified; and when the canon peer's
  listing fails for a subtree, no peer is modified under it (008.10-008.15).
