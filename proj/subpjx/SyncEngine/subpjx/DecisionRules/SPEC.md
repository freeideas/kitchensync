# DecisionRules:

The pure per-path resolver for a KitchenSync run. Given what every peer holds at
one path -- each peer's live state, its snapshot row, and its role -- DecisionRules
classifies each peer and returns the single agreed outcome for that path: which
type wins, which file version wins, and what each peer must do to conform.

## Purpose

The SyncEngine walk reaches one path at a time across all peer trees. At each
path it must answer one question: given every peer's live entry, snapshot row,
and role, what is the agreed state here and what does each peer need to have done
to it? DecisionRules answers exactly that question and nothing else.

DecisionRules is a pure decision function. It performs no input or output: it
reads no filesystem, opens no connection, touches no snapshot database, and writes
no log line. It receives already-gathered per-peer facts for one path and returns
a decision describing the outcome and the per-peer actions. The SyncEngine facade
gathers those facts during the walk and carries out the actions DecisionRules
returns (creating directories, enqueuing copies, calling Displacement); the timing,
threading of the dry-run flag, and execution of those actions are the facade's job,
not this child's.

## Responsibilities

### Per-peer classification (010)

For each peer at the path, classify its live state against its own snapshot row
into exactly one category:

- unchanged -- live file, row present and not tombstoned, mod_time within 5 seconds
  of the row's mod_time and byte_size equal to the row's byte_size (010.1).
- modified -- live file whose byte_size differs from the row (010.2), or whose
  mod_time differs from the row by more than 5 seconds (010.3), or that is present
  where the row carries a non-NULL `deleted_time` (resurrection) (010.4).
- new -- live file with no snapshot row on that peer (010.5).
- deleted -- absent entry whose row has a non-NULL `deleted_time`; the
  `deleted_time` is that peer's deletion estimate (010.6).
- absent-unconfirmed -- absent entry whose row has a NULL `deleted_time`; this is
  not a recorded deletion (010.7).
- no-opinion -- absent entry with no snapshot row; that peer alone never causes the
  entry to be removed from peers that have it (010.8).

The 5-second tolerance governs every mod_time comparison; a file counts as
unchanged only when both mod_time and byte_size match.

### Roles and canon override (007)

- A peer may be canon (`+`), contributing (ordinary), or subordinate (`-`). At
  most one peer is canon.
- A subordinate peer's entries never enter the set used to pick a winner, so the
  contributing outcome is identical to that peer being absent. After the
  contributing decision is made, the subordinate peer is conformed to it (007.2).
- When a canon peer is present its state wins unconditionally over differing peers
  (007.1); see the file and directory rules below.

### File decision (011)

- With a canon peer present, the canon state wins outright: a file canon has is
  copied to every other peer, including subordinate peers (011.1); a file canon
  lacks is deleted from every other peer (011.2).
- Without a canon peer, resolve the contributing peers' classifications into one
  outcome:
  - All contributing peers unchanged and already matching -> no copy among the
    matching peers (011.3), but copy the file to any active peer that lacks it,
    including subordinate peers (011.4).
  - Differing modified versions, or a file new on one or more peers -> the version
    with the newest mod_time wins and is propagated to every peer that does not
    already match it (011.5, 011.6).
  - One or more peers deleted the file -> use the most recent deletion estimate
    among the deleting peers (011.7). The deletion wins and the file is removed
    from every peer that has it only when that estimate exceeds the existing
    file's mod_time by more than 5 seconds (011.8); when the existing file's
    mod_time is within 5 seconds of, or later than, the estimate, the file is kept
    and copied to peers that lack it (011.9).
  - For an absent-unconfirmed peer, treat its row's `last_seen` as the deletion
    estimate only when `last_seen` exceeds the maximum mod_time among peers that
    have the file by more than 5 seconds (011.10); otherwise (or when `last_seen`
    is null) re-copy the file to that peer and cast no deletion vote (011.11).
  - When contributing peers share the same mod_time but differ in byte_size, the
    larger file wins (011.12).
- A peer with no snapshot row casts no vote on which version wins (011.13) but
  still receives the winning file once a winner is decided (011.14).
- When selecting the newest version, a peer within 5 seconds of the maximum
  mod_time is treated as tied with the maximum (011.16); a peer more than 5
  seconds behind the maximum loses to it (011.17).
- Enqueue no copy to a peer that already matches the winner -- mod_time within 5
  seconds and equal byte_size (011.15).

### Directory decision (012.1-012.7)

Directories are decided by existence, never by mod_time (012.2):

- If any contributing peer has the directory live, create it on every active peer
  that lacks it (012.1). With a canon peer that has the directory, it is created
  on every peer that lacks it (012.6).
- If no contributing peer has it live, at least one contributing peer has a
  snapshot row for it, and every contributing peer that has a row is now absent,
  displace it to BAK on every peer that still has it (012.3). A contributing peer
  with no row neither votes nor blocks this displacement (012.4). When a canon
  peer lacks the directory, it is displaced on every peer that has it (012.7).
- If no contributing peer has it live and none has a row, displace it from
  subordinate peers that still have it (012.5).

### File/directory type conflict (012.8-012.17)

When the path is a file on one peer and a directory on another:

- Canon present, canon has a file: displace the conflicting directories to BAK
  (012.8) and sync the canon file to every peer (012.9).
- Canon present, canon has a directory: displace the conflicting files to BAK
  (012.10) and create and sync the directory on every peer (012.11).
- Canon present, canon lacks the path: displace the path to BAK on every peer that
  has it (012.12).
- No canon: the file wins. Displace each contributing peer's conflicting directory
  to BAK (012.13), then select the winning file among the contributing file
  entries by the normal file rules above and sync it to all active peers (012.14).
  A subordinate peer's file never causes the file to win over a contributing
  peer's directory (012.15).
- After the contributing type decision, a subordinate peer whose path has the
  wrong type is displaced to BAK (012.16) and then conformed to the decided type
  (012.17).

## Boundaries

- DecisionRules is a pure function with no side effects. It does not read or write
  the filesystem, does not connect to peers, does not read or update the snapshot
  database, and does not emit output. All per-peer facts (live type, byte_size,
  mod_time; snapshot row byte_size, mod_time, `deleted_time`, `last_seen`; role)
  arrive as inputs gathered by the SyncEngine facade.
- DecisionRules decides; it never acts. It does not create directories, move
  bytes, or perform the BAK rename. It names which entries must be displaced, but
  the rename itself is the Displacement child's job, invoked by the facade.
  Enqueuing copies, threading the dry-run flag, ordering the walk, and applying
  excludes all belong to the facade.
- The decision covers exactly one path, considering only that path's per-peer
  entries and rows. It does not walk, recurse, or look at child paths; the facade
  drives recursion.
- The 5-second tolerance is the single comparison constant used for every mod_time
  and deletion-estimate comparison.
- Operation exposed across the boundary: a single `decide` entry point that takes
  the per-peer inputs for one path (each peer's live state, snapshot row, and role,
  plus which peer if any is canon) and returns the resolved outcome -- the agreed
  type at the path (file, directory, or absent), the winning file source when the
  outcome is a file, and the per-peer action each peer needs (copy the winner in,
  create the directory, displace the existing entry to BAK, or do nothing).
  Classification is an internal step of this decision.
- Invariants: the result is deterministic for a given set of inputs; subordinate
  peers never affect which contributing version or type wins; a peer with no
  snapshot row never votes but always receives the decided winner; and ties keep
  data rather than deleting it.
