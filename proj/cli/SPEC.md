# cli:

## Purpose

Own the public command-line surface for `kitchensync [options] <peer> <peer> [<peer>...]`: help selection, argument validation, global options, peer operand syntax, fallback URL syntax, per-URL settings, command-line excludes, and conversion into a typed run request for root orchestration.

The module stops at invocation interpretation. It does not connect to peers, inspect snapshots, apply startup-derived peer roles, schedule transfers, render progress, or make sync decisions.
# peer:

## Purpose

Own peer identity, URL normalization, fallback URL selection, startup connectivity, peer role application, and construction of connected peer handles for a run. The module decides which URL wins for each logical peer and which reachable peers are canon, contributing, or subordinate after snapshot existence is known.

The module exposes reachable peer sessions to root orchestration. It does not implement transport filesystem operations, mutate snapshot databases, make per-path sync decisions, perform safe replacement sequences, or render output.
# transport:

## Purpose

Own the filesystem operation boundary shared by local `file://` peers and SSH/SFTP peers. The module provides rooted transport behavior for listing, stat, streaming reads and writes, rename without overwrite, create/delete operations, modification-time updates, and normalized transport error categories.

The module hides scheme-specific I/O details behind the root transport contract. It does not choose fallback URLs, classify sync outcomes, manage SQLite snapshots, perform SWAP/BAK replacement sequences, schedule copies, or render diagnostics.
# snapshot:

## Purpose

Own the per-peer SQLite snapshot database format and lifecycle, including local temporary snapshot copies, peer-side snapshot SWAP recovery and upload, path identifiers, tombstones, row mutation state, and timestamp generation.

The module provides stored peer history and snapshot update primitives to root orchestration and traversal. It does not connect peers, choose sync winners, mutate user data paths, schedule copy retries, or render progress.
# sync:

## Purpose

Own the combined-tree traversal and reconciliation decision engine for connected peers. The module lists active peers by directory, applies built-in and command-line excludes, classifies live entries against snapshot rows, chooses file, directory, deletion, type-conflict, canon, and subordinate outcomes, and requests the required snapshot, operation, and copy-scheduler effects.

The module exposes one sync traversal surface to root orchestration. It does not parse CLI input, connect peers, implement transport I/O, define SQLite storage, perform safe replacement sequencing, schedule copy retries, or render terminal output.
# operations:

## Purpose

Own peer-side mutation sequences other than abstract sync decision-making: safe file copy replacement, user-entry SWAP recovery, displacement to nearby BAK, directory creation, BAK/TMP retention cleanup, and dry-run suppression of peer-side mutations.

The module composes transport primitives into the required safety sequences. It does not choose sync outcomes, manage peer startup, store snapshot rows, enqueue or retry copy work, or render progress and diagnostics.
# runtime:

## Purpose

Own run-time coordination surfaces for copy scheduling, copy retry accounting, the global active-copy limit, progress events, diagnostics, verbosity filtering, live terminal status, and line-oriented stdout output.

The module provides the copy scheduler and output sinks used while traversal and transfer work are in progress. It does not decide sync outcomes, implement safe file replacement, perform transport I/O, manage snapshots, connect peers, or choose process exit codes.
