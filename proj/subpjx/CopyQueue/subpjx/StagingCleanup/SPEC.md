# StagingCleanup:

## Purpose

StagingCleanup purges aged staging entries under one peer directory's
`.kitchensync/` area. For a given directory on a given peer it inspects the
`BAK/` and `TMP/` staging areas and removes the entries that have aged past
their retention limits, judging each entry's age only from the timestamp that
appears in its own directory name. Recent entries are left untouched, and
`SWAP/` entries are never removed on the basis of age.

StagingCleanup is the cleanup worker that the CopyQueue facade delegates to when
a normal run finishes processing a directory level. It does not decide when
cleanup runs, does not list live entries for sync decisions, and does not
displace or archive anything into BAK; it only deletes already-aged BAK and TMP
staging entries.

## Responsibilities

### Cleanup of one directory level on one peer

- Inspect a peer's `.kitchensync/` directory directly and purge aged `BAK/` and
  `TMP/` entries from it, even though the built-in exclude removes
  `.kitchensync/` from synced listings (021.10).
- Remove each `.kitchensync/BAK/<timestamp>/` entry whose timestamp is older
  than the BAK retention limit (021.11), and remove each
  `.kitchensync/TMP/<timestamp>/` entry whose timestamp is older than the TMP
  retention limit (021.12).
- Leave each `.kitchensync/BAK/<timestamp>/` entry whose timestamp is not older
  than the BAK retention limit in place (021.14), and leave each
  `.kitchensync/TMP/<timestamp>/` entry whose timestamp is not older than the
  TMP retention limit in place (021.15).
- Never remove `.kitchensync/SWAP/` entries based on age (021.16).

### Age judged from the directory name

- Determine each entry's age solely from the `<timestamp>` component of its
  directory name, not from any filesystem modification time (021.13).

### Retention limits and their defaults

- Treat the BAK retention limit as the `--keep-bak-days` value, using a 90-day
  limit when the flag is not supplied (021.17).
- Treat the TMP retention limit as the `--keep-tmp-days` value, using a 2-day
  limit when the flag is not supplied (021.18).

### Dry-run suppression

- In a dry-run, skip peer-side BAK/TMP cleanup entirely, mutating no peer state
  (021.19, 024.19).

## Boundaries

### Operations exposed across the boundary

- A single cleanup operation that purges aged BAK and TMP entries for one
  directory on one peer. Its inputs are the per-peer filesystem handle to use,
  the directory whose `.kitchensync/` is to be cleaned, the BAK and TMP
  retention limits in days, the reference time that ages are measured against,
  and whether the run is a dry-run. When the run is a dry-run the operation
  makes no peer-side change.

### How it reaches peer state

- StagingCleanup performs every listing and removal through the per-peer
  filesystem handle it is given by the CopyQueue facade. It never opens
  connections itself and never branches on the peer's scheme; the same code path
  serves local and SFTP peers.

### Error obligations

- No cleanup-specific error handling is required beyond the requirements above.
  A purge that the filesystem handle cannot complete is not specified to receive
  special treatment.

### Not in scope

- Deciding when cleanup runs, traversing the tree, and listing a directory's
  live entries for sync decisions belong to the sync engine and the CopyQueue
  facade.
- Creating BAK directories, displacing entries into BAK, and the SWAP staging
  and recovery sequence belong to the displacement path and the swap-transfer
  sibling, not to StagingCleanup.
- The exact timestamp string format embedded in BAK and TMP directory names is
  owned by the timestamp concern; StagingCleanup interprets that component to
  compute age but does not define its format.
- The per-peer filesystem primitives (list, remove, stat) belong to the
  transport component, which StagingCleanup is handed and calls.

### Invariants

- A `.kitchensync/SWAP/` entry is never removed by StagingCleanup on the basis
  of age.
- An entry's age is judged only from the `<timestamp>` in its directory name.
- In a dry-run, StagingCleanup mutates no peer state.
