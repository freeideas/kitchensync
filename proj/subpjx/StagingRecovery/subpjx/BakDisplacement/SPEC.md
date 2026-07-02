# BakDisplacement:

## Purpose

BakDisplacement moves one displaced user entry into the BAK storage directory
next to that entry's original parent directory. It exists so StagingRecovery can
delegate the BAK part of peer-side staging while remaining a facade over the
separate staging operation families.

The operation is for an entry already chosen for displacement by a caller. The
caller supplies the peer, the original `<parent>/<basename>` path, and the BAK
timestamp string. BakDisplacement creates the nearby BAK timestamp directory
when needed, then moves the original entry to
`<parent>/.kitchensync/BAK/<timestamp>/<basename>`.

## Responsibilities

BakDisplacement exposes a displacement operation for one existing user entry on
one peer. Its inputs identify:

- the peer filesystem to mutate,
- the parent directory that currently contains the entry,
- the entry basename,
- the timestamp directory name to use below `BAK/`.

For each call, BakDisplacement first creates
`<parent>/.kitchensync/BAK/<timestamp>/` and any missing parent directories
below `<parent>`. The BAK directory is always below the displaced entry's own
parent directory. The operation must not place displaced entries in a BAK
directory under the sync root unless the displaced entry's parent is the sync
root.

After the BAK timestamp directory exists, BakDisplacement moves
`<parent>/<basename>` to
`<parent>/.kitchensync/BAK/<timestamp>/<basename>`. A successful result means
the original path is absent and the displaced entry is present at the BAK
destination. If the displaced entry is a directory, the directory is moved as a
single entry and its complete subtree remains below the BAK destination.

BakDisplacement reports failure when it cannot create the BAK timestamp
directory or cannot move the displaced entry to the BAK destination. A failure
result includes enough path context for the caller to report which displacement
failed. It does not choose a different timestamp, choose a different BAK
location, delete the original entry, or partially copy directory contents as a
fallback.

## Boundaries

BakDisplacement does not decide which entries should be displaced. It does not
compare peers, inspect snapshot rows, apply dry-run policy, update transfer
queues, or format progress output.

BakDisplacement does not own timestamp generation. Callers provide the
timestamp string used as the BAK timestamp directory name.

BakDisplacement does not clean up old BAK directories. Age-based BAK and TMP
cleanup belongs to the staging cleanup child.

BakDisplacement does not recover SWAP state. SWAP recovery may use BAK storage
through its caller, but the recovery rules and encoded SWAP basename handling
belong outside this child.
