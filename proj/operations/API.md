# operations API

Rust module path: `kitchensync::operations`.

The `operations` module exports the peer-mutation contract used by `sync` and
`runtime`. It owns operation sequencing for user-entry SWAP recovery, safe copy
replacement, BAK displacement, directory creation, BAK/TMP retention cleanup,
and dry-run suppression. It does not export path-building helpers, transport
implementations, retry policy, traversal decisions, snapshot storage, or output
rendering.

## Imported Root Contracts

The public API names these root-owned contracts without redefining them:

- `RunConfig`
- `PeerSession`
- `PeerId`
- `RelPath`
- `EntryMeta`
- `Timestamp`
- `TransportError`
- `DiagnosticSink`
- `ProgressSink`
- `TransferPhase`
- `CopyResult`

`TransferPhase` values used by this module are the root values
`read_source`, `write_swap_new`, `move_existing_to_swap_old`, `rename_final`,
`set_mod_time`, `archive_old`, and `cleanup`.

## Public Trait

```rust
pub trait OperationExecutor {
    fn recover_directory_swaps(
        &self,
        peer: &PeerSession,
        directory: &RelPath,
    ) -> OperationResult<RecoveryReport>;

    fn displace_to_bak(
        &self,
        peer: &PeerSession,
        path: &RelPath,
        timestamp: Timestamp,
    ) -> OperationResult<DisplacementReport>;

    fn create_directory(
        &self,
        peer: &PeerSession,
        path: &RelPath,
    ) -> OperationResult<DirectoryCreationReport>;

    fn cleanup_retention(
        &self,
        peer: &PeerSession,
        directory: &RelPath,
        now: Timestamp,
        keep_bak_days: u32,
        keep_tmp_days: u32,
    ) -> OperationResult<CleanupReport>;

    fn execute_copy_attempt(
        &self,
        source_peer: &PeerSession,
        source_path: &RelPath,
        destination_peer: &PeerSession,
        destination_path: &RelPath,
        winning_meta: &EntryMeta,
    ) -> CopyResult;
}
```

Implementations must use the connected transport handle already contained by
each `PeerSession`. Callers must pass normalized root-relative `RelPath` values.
The trait must not expose local filesystem, SFTP, or other transport-specific
error types.

## Public Construction

```rust
pub fn executor<'a>(
    config: &'a RunConfig,
    diagnostics: &'a dyn DiagnosticSink,
    progress: &'a dyn ProgressSink,
) -> impl OperationExecutor + 'a;
```

The returned executor borrows the run configuration and sinks for the duration
of the run. It does not own peer sessions, snapshot stores, copy scheduler
state, or traversal state.

## Public Result Types

```rust
pub type OperationResult<T> = Result<T, OperationError>;
```

```rust
pub struct RecoveryReport {
    pub peer_id: PeerId,
    pub directory: RelPath,
    pub recovered_entries: u64,
    pub dry_run: bool,
}
```

`RecoveryReport` reports successful traversal-time recovery work for one
directory. In dry-run mode it reports the planned no-op result and no peer state
is changed.

```rust
pub struct DisplacementReport {
    pub peer_id: PeerId,
    pub original_path: RelPath,
    pub bak_path: RelPath,
    pub dry_run: bool,
}
```

`DisplacementReport` identifies the active entry path and nearby BAK path used
for a successful or planned displacement.

```rust
pub struct DirectoryCreationReport {
    pub peer_id: PeerId,
    pub path: RelPath,
    pub dry_run: bool,
}
```

`DirectoryCreationReport` confirms that the requested directory and any missing
parents were created, or would be created during dry-run.

```rust
pub struct CleanupReport {
    pub peer_id: PeerId,
    pub directory: RelPath,
    pub removed_targets: Vec<CleanupTarget>,
    pub retained_targets: Vec<CleanupTarget>,
    pub nonfatal_failures: Vec<CleanupFailure>,
    pub dry_run: bool,
}
```

`CleanupReport` reports BAK and TMP retention work under one traversal
directory. Cleanup failures are nonfatal and must be included in
`nonfatal_failures` instead of changing user-entry sync decisions already made
by the caller.

```rust
pub struct CleanupTarget {
    pub kind: CleanupTargetKind,
    pub path: RelPath,
    pub timestamp: Timestamp,
}
```

```rust
pub enum CleanupTargetKind {
    Bak,
    Tmp,
}
```

```rust
pub struct CleanupFailure {
    pub target: Option<CleanupTarget>,
    pub error: TransportError,
}
```

## Public Errors

```rust
pub struct OperationError {
    pub peer_id: PeerId,
    pub context: OperationErrorContext,
    pub error: TransportError,
}
```

```rust
pub enum OperationErrorContext {
    RecoverDirectorySwaps { directory: RelPath },
    DisplaceToBak { path: RelPath },
    CreateDirectory { path: RelPath },
    CleanupRetention {
        directory: RelPath,
        target: Option<CleanupTarget>,
    },
}
```

Operation errors expose only normalized `TransportError` categories and the
operation context needed by callers to decide whether to skip snapshot updates,
treat a directory as unlistable, or emit diagnostics. Copy-attempt failures are
reported through root `CopyResult` with the exact failing `TransferPhase`.

## Ownership And Mutation Rules

- `OperationExecutor` borrows `RunConfig`, `DiagnosticSink`, and `ProgressSink`.
- `PeerSession`, `RelPath`, and `EntryMeta` arguments are borrowed unless a
  result must retain identifying data after the call returns.
- Report and error values own their `RelPath`, `Timestamp`, target, and
  diagnostic fields.
- The module mutates peers only through the `TransportHandle` reachable from
  the supplied `PeerSession`.
- Dry-run mode must not create, modify, rename, delete, archive, clean up, or
  set modification times on any peer. Dry-run copy attempts still read the
  source stream and report source read failures as `read_source`.
- The module may optimize local-to-local content transfer internally, but public
  results and error categories must remain identical across `file://`,
  `sftp://`, and mixed transfers.

## Private Implementation Details

The following remain private to `operations` and must not be relied on by other
modules:

- SWAP, BAK, and TMP path construction helpers;
- basename percent-encoding helpers;
- recovery state classifiers;
- bounded stream-copy implementation details;
- best-effort cleanup routines;
- fresh timestamp acquisition for copy archiving;
- local-to-local copy optimization choices;
- dry-run wrapper internals.
