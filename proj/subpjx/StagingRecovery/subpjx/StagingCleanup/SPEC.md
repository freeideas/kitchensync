# StagingCleanup:

## Purpose

StagingCleanup removes expired BAK and TMP timestamp directories for one peer
and one parent directory after traversal work at that directory level has
finished. It exists so StagingRecovery can delegate age-based cleanup while
remaining a facade over the separate peer-side staging operation families.

The caller supplies the peer filesystem, the parent directory, the current time
used for age comparison, and the configured `--keep-bak-days` and
`--keep-tmp-days` values. StagingCleanup checks metadata directories below that
parent even though `.kitchensync/` is excluded from sync decisions.

## Responsibilities

StagingCleanup exposes a cleanup operation for one peer and one parent
directory. The operation is called after the caller has processed the union of
entry names at that directory level. Its inputs identify:

- the peer filesystem to inspect and mutate,
- the parent directory whose metadata area is being cleaned,
- the current time used to decide age,
- the keep duration from `--keep-bak-days`,
- the keep duration from `--keep-tmp-days`.

For each call, StagingCleanup checks only these cleanup roots below the supplied
parent directory:

- `<parent>/.kitchensync/BAK/`
- `<parent>/.kitchensync/TMP/`

For each direct timestamp directory below `BAK/`, StagingCleanup determines age
from that directory name's `<timestamp>` component. It removes
`<parent>/.kitchensync/BAK/<timestamp>/` when that timestamp is older than
`--keep-bak-days`, and leaves it in place when that timestamp is not older than
`--keep-bak-days`.

For each direct timestamp directory below `TMP/`, StagingCleanup determines age
from that directory name's `<timestamp>` component. It removes
`<parent>/.kitchensync/TMP/<timestamp>/` when that timestamp is older than
`--keep-tmp-days`, and leaves it in place when that timestamp is not older than
`--keep-tmp-days`.

Cleanup is based on the timestamp path component, not on filesystem creation
time, modification time, access time, or snapshot rows. A successful result means
all expired BAK and TMP timestamp directories found for the supplied parent were
removed, all unexpired BAK and TMP timestamp directories were left in place, and
no SWAP directory was purged by age.

StagingCleanup reports failure when it cannot inspect a BAK or TMP cleanup root
that exists, cannot determine the timestamp age for a staging directory it must
evaluate, or cannot remove a staging directory selected for removal. A failure
result includes enough peer and path context for the caller to report which
cleanup check failed. It does not retry, choose different retention values, or
delete unexpired directories as a fallback.

## Boundaries

StagingCleanup does not decide when a directory level has finished traversal.
The traversal owner calls it after processing the union of entry names at that
level.

StagingCleanup does not sync metadata directories. It directly reads and writes
below `.kitchensync/` only for BAK and TMP cleanup.

StagingCleanup does not recover or purge `.kitchensync/SWAP/` by age. SWAP
state remains for SWAP recovery to handle.

StagingCleanup does not create BAK directories for displaced entries or TMP
directories for transfer work. Those operations belong to the staging children
that prepare those paths.

StagingCleanup does not inspect live user entries, compare peers, update
snapshot rows, apply dry-run policy, format output, or pick the transport
implementation. It returns cleanup results with enough context for its caller to
handle reporting and higher-level traversal decisions.
