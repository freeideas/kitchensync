# Displacement:

The inline mover for a KitchenSync run. When a sync decision says an entry must be
set aside rather than overwritten, Displacement performs the one rename that moves
that entry into a recoverable BAK location beside it, leaving the rest of the walk
to continue.

## Purpose

The SyncEngine walk reaches a path and decides what each peer must do there. Some
of those decisions -- a file losing to a deletion, a directory that no peer keeps,
a type conflict that must be cleared before a copy -- require an existing entry to
be moved aside instead of being deleted, so it stays recoverable. Displacement is
the single operation that carries out that move.

Given the parent directory of an entry, the entry's basename, the timestamp string
for the run, the dry-run flag, and a handle to the transport and output services,
Displacement renames `<parent>/<basename>` into a per-parent BAK directory under
that parent's `.kitchensync/`. It moves a directory as one subtree rename, so the
entry's whole subtree travels with it. It does not decide that a displacement is
needed and it does not move file bytes between peers; the SyncEngine facade decides
and calls Displacement, and the copy queue does the byte copying.

## Responsibilities

### Create the BAK directory (021.1, 021.4)

- Before renaming, create the directory `<parent>/.kitchensync/BAK/<timestamp>/`,
  including any missing parent directories (`.kitchensync/` and `BAK/`), through
  the transport (021.1).
- Create that BAK directory under `.kitchensync/` at the displaced entry's own
  parent directory. Never aggregate displaced entries into a single BAK directory
  at the sync root; each displacement uses the BAK directory co-located with the
  entry it moves (021.4).

### Rename the entry into BAK (021.2, 021.3)

- Rename the entry at `<parent>/<basename>` to
  `<parent>/.kitchensync/BAK/<timestamp>/<basename>`, preserving its basename
  (021.2).
- When the entry is a directory, move it as a single rename so its entire subtree
  is preserved and travels with it; do not copy and delete entry by entry (021.3).

### Handle a rename failure (021.5, 021.6)

- When the rename into BAK fails, log an error-level diagnostic through the output
  service (021.5).
- When the rename into BAK fails, leave the entry in place at its original path --
  do not delete it, do not partially move it -- and report the failure so the walk
  can continue without treating the entry as displaced (021.6).

### Honor dry-run (024.15, 024.16)

- Under dry-run, perform no rename and create no BAK directory, so no destination
  file on any peer is moved aside (024.15).
- Under dry-run, displace nothing for a decision that would remove a file, so no
  destination file on any peer is deleted -- a deletion is only ever carried out as
  a displacement, and the suppressed displacement leaves the file untouched
  (024.16). Displacement still returns as if the move were the decided action, so
  the facade can report what would have happened.

### Operations exposed across the boundary

- A single displace operation that takes the parent directory path, the entry's
  basename, the run timestamp string, the dry-run flag, and the transport and
  output handles, performs the BAK create and the rename (or suppresses both under
  dry-run), and reports whether the entry was moved or left in place.

## Boundaries

- Displacement performs exactly one displacement per call and decides nothing. It
  does not classify entries, resolve winners, or choose which entries to displace;
  those decisions belong to DecisionRules and the SyncEngine facade, which call
  Displacement with an entry already chosen to be moved aside.
- Displacement reaches the filesystem only through the transport service: it
  creates the BAK directory and performs the rename through the transport and never
  branches on URL scheme, opens connections, or moves bytes directly. It does not
  copy files; the copy queue owns SWAP-staged replacement and copy execution.
- Displacement emits no output except the single error-level diagnostic on rename
  failure, written through the output service. It does not own the `X` progress
  line for a successful displacement or the diagnostic's wording and verbosity
  rules; the output and logging concern owns those, and the facade emits the
  progress line.
- Displacement does not generate the timestamp string. It receives the already-
  formatted `<timestamp>` for the run and only places it in the BAK path; the
  timestamp format is owned by the timestamps concern.
- Displacement does not read or update the snapshot database. When a displacement
  stands in for a deletion, the snapshot service tombstones the relevant rows; that
  is the facade's and snapshot service's job, not this child's.
- Displacement does not purge or maintain BAK or TMP directories by age and does
  not perform TMP staging; opportunistic `.kitchensync/` cleanup during traversal
  belongs to the facade and copy queue.
- Displacement defines no persistent global state and is created per dependent.
- Invariants Displacement maintains: a successful displacement leaves the entry
  under `<parent>/.kitchensync/BAK/<timestamp>/<basename>` and nowhere else; a
  displaced directory keeps its whole subtree intact through one rename; a failed
  displacement leaves the entry exactly where it was; and under dry-run no peer is
  mutated at all.
