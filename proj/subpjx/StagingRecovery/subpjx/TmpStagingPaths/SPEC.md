# TmpStagingPaths:

## Purpose

TmpStagingPaths creates transfer staging locations below the supplied parent
directory without touching live user paths. It exists so StagingRecovery can
delegate TMP path creation while remaining a facade over the separate staging
operation families.

The operation is for transfer work already chosen by a caller. The caller
supplies the peer, the parent directory for the current directory level, the
TMP timestamp string, and the transfer UUID. TmpStagingPaths returns a staging
path under `<parent>/.kitchensync/TMP/<timestamp>/<transfer-uuid>/`.

## Responsibilities

TmpStagingPaths exposes a TMP staging-path operation for one transfer on one
peer. Its inputs identify:

- the peer filesystem to mutate,
- the parent directory that owns the transfer staging area,
- the timestamp directory name to use below `TMP/`,
- the transfer UUID segment for this transfer's staging path.

For each call, TmpStagingPaths creates
`<parent>/.kitchensync/TMP/<timestamp>/` and any missing metadata parent
directories below `<parent>`. It then creates or returns the transfer-specific
directory `<parent>/.kitchensync/TMP/<timestamp>/<transfer-uuid>/`.

The returned path is only a temporary work location for the caller's transfer
steps. A successful result means the transfer-specific TMP directory exists and
the operation did not rename, delete, overwrite, or replace any live user path
under `<parent>`.

TmpStagingPaths reports failure when it cannot create the TMP timestamp
directory, cannot create the transfer-specific TMP directory, or finds that the
requested TMP path cannot be used as a directory. A failure result includes
enough peer and path context for the caller to report which TMP staging path
could not be prepared. It does not choose a different timestamp, choose a
different UUID, remove a conflicting path, or fall back to a live user path.

## Boundaries

TmpStagingPaths does not decide which transfers run, which peer paths are
copied, or when staged content is moved into its final destination. It only
prepares and returns the TMP directory path for one transfer.

TmpStagingPaths does not own timestamp generation or UUID generation. Callers
provide the timestamp string used as the TMP timestamp directory name and the
transfer UUID used as the transfer-specific path segment.

TmpStagingPaths does not clean up old TMP directories. Age-based BAK and TMP
cleanup belongs to the staging cleanup child.

TmpStagingPaths does not recover SWAP state, move displaced entries to BAK, or
update snapshot rows. It does not format output, retry failed operations,
suppress writes for dry-run mode, or pick the transport implementation.
