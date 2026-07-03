# DryRunMode:

## Purpose

DryRunMode owns the read-only meaning of `--dry-run` for one sync run. It
defines which normal sync work still happens, which peer-side operations are
forbidden, and which local temporary work remains allowed so the run can plan
as realistically as possible without changing any peer.

This child is not an executable and does not parse command-line text. The root
coordinator and the other first-layer children use its run-mode policy when
startup, snapshot preparation, traversal, copy work, staging, cleanup, and
snapshot upload need to behave differently in dry-run mode.

## Responsibilities

DryRunMode exposes operations for these behaviors:

- Report that a dry-run sync must print exactly `dry run` as one stdout line
  before any progress line or `sync complete` line. The line is emitted for
  every dry-run sync, including dry runs at `--verbosity error`.
- Provide startup root policy for candidate peer URLs. In dry-run mode, a
  `file://` candidate whose local root path does not already exist fails for
  that run, and no local root or missing parent directory is created. An
  `sftp://` candidate may establish SSH/SFTP connection state, but if the
  remote root path does not already exist, that candidate fails for the run
  and no remote root or missing parent directory is created.
- Preserve normal reachable-peer behavior for roots that already exist.
  Dry-run startup still establishes connections to reachable peer roots and
  still uses the winning URL selected at startup for later operations.
- Provide snapshot startup policy. Before downloading snapshots in dry-run
  mode, peer-side snapshot SWAP recovery is skipped. When a reachable peer has
  a live `.kitchensync/snapshot.db`, the live file is downloaded as-is to the
  local temporary snapshot database. When the live snapshot is not found, a new
  empty snapshot is created only as local temporary working state. A snapshot
  download failure other than not found excludes that peer from the reachable
  set for the run and carries the same error-level diagnostic obligation as
  the normal snapshot startup failure.
- Allow local temporary snapshot databases to be created and updated during
  traversal. These writes are local working state only and are not peer state.
- Forbid all peer snapshot mutation in dry-run mode. No dry-run operation may
  create, modify, rename, delete, or upload `.kitchensync/snapshot.db` or its
  peer-side SWAP state through a peer URL.
- Preserve traversal reads. Dry-run mode lists reachable peer directories and
  uses those live listings for sync decisions.
- Preserve dry-run copy realism. Queued copy work still acquires global copy
  slots subject to `--max-copies`, reads source files, and applies the
  configured `--retries-copy` total-try limit. A source read failure is a real
  dry-run copy try failure and is retried or exhausted by the same rules as a
  normal run.
- Preserve copy and delete progress visibility. Dry-run mode emits `C` and
  `X` progress lines under the same verbosity settings as a normal run. It
  reports planned copy and delete actions without performing the peer writes
  that would make them real.
- Forbid user-file peer writes in dry-run mode. Planned sync content must not
  create destination directories, create `.kitchensync/TMP/`,
  `.kitchensync/SWAP/`, or `.kitchensync/BAK/` directories, write destination
  files, rename peer entries, delete peer entries, displace peer entries to
  BAK, or set file modification times on peers.
- Skip peer-side user-file SWAP recovery during traversal in dry-run mode.
  Existing peer SWAP state is left untouched and is not interpreted by a
  dry-run recovery action before directory listings.
- Skip BAK/TMP cleanup on peers in dry-run mode. Existing peer BAK and TMP
  entries are left untouched regardless of their age.
- Skip final peer snapshot upload in dry-run mode. Updated local temporary
  snapshots are not uploaded back to peer `.kitchensync/snapshot.db` paths.

The dry-run operation result distinguishes three kinds of work:

- `peer read`: connection to an existing root, directory listing, stat,
  snapshot download, and source file reads. These remain allowed.
- `local working write`: creation and update of local temporary snapshot
  databases. These remain allowed.
- `peer write`: directory creation, file write, rename, delete, displacement,
  modification-time setting, SWAP recovery, BAK/TMP cleanup, and snapshot
  upload through any peer URL. These are suppressed in dry-run mode.

When DryRunMode suppresses a peer write, the caller receives a planned-success
result for dry-run decision flow only. Suppressed peer writes must not call the
underlying peer transport and therefore must not produce transport errors from
the skipped write phase. Real read failures, connection failures, and snapshot
download failures still surface through the normal error paths.

## Boundaries

DryRunMode does not own command-line parsing or validation. CommandLine owns
accepting `--dry-run` and its default value. DryRunMode owns the behavior once
that accepted option is enabled.

DryRunMode does not implement local filesystem access, SSH/SFTP access, root
existence checks, directory listing, stat, file reads, writes, renames,
deletes, or modification-time updates. Those operations remain behind
PeerTransportSurface and the transport children. DryRunMode defines which of
those operations may be invoked in dry-run mode.

DryRunMode does not decide peer grouping, fallback URL order, winning URL
selection, canon status, subordinate status, first-sync validity, or reachable
set failure outcomes. PeerConnections owns those decisions using DryRunMode's
startup and snapshot policies.

DryRunMode does not own the SQLite schema, row meaning, local temporary
snapshot file implementation, snapshot row updates, normal snapshot SWAP
replacement, or normal snapshot SWAP recovery cases. SnapshotDatabase owns
those behaviors and uses DryRunMode to skip peer-side recovery and upload
while still allowing local temporary snapshot work.

DryRunMode does not own reconciliation decisions, listing retry rules,
exclude rules, or traversal ordering. SyncTraversal owns the walk and uses
DryRunMode to skip peer-side SWAP recovery during traversal while preserving
directory listings and local temporary snapshot updates.

DryRunMode does not own copy queue scheduling, bounded buffering, copy-slot
accounting, copy retry state, progress line formatting, user-file SWAP/BAK/TMP
paths, displacement implementation, or cleanup retention rules. CopyStaging
owns those behaviors and uses DryRunMode to read sources and exercise copy
tries while suppressing destination peer mutations and cleanup.

Its invariants are:

- A dry run never creates a missing peer root or missing peer root parent.
- A dry run never creates, modifies, renames, deletes, displaces, uploads, or
  sets modification times through a `file://` or `sftp://` peer URL.
- A dry run may create and update only local temporary snapshot databases as
  working state.
- Dry-run snapshot startup reads the live peer snapshot as-is and never repairs
  peer-side snapshot SWAP state.
- Dry-run traversal reads live peer directories and source files, but skips
  peer-side SWAP recovery and BAK/TMP cleanup.
- Dry-run copy work consumes copy slots and try counts exactly like normal copy
  work, except destination peer mutations are not invoked.
- Dry-run output begins with the single line `dry run` before progress or
  completion output.
