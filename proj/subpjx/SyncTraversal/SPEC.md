# SyncTraversal:

## Purpose

SyncTraversal owns the recursive combined-tree walk for one sync run. It visits
the accepted reachable peer trees through the shared peer transport surface,
removes excluded paths before making decisions, chooses the group outcome for
each visited file or directory, and asks the snapshot and staging children to
record or apply the chosen outcome.

This child is not an executable. The root run coordinator calls it after peer
startup has produced the reachable peers, their canon or subordinate roles,
their snapshot-history status, their local temporary snapshot databases, the
accepted command-line excludes, and the run options that affect traversal.

## Responsibilities

SyncTraversal exposes an operation that starts traversal at the sync root and
continues recursively until every allowed directory subtree has either been
processed or skipped by the specified failure rules. The operation accepts only
reachable peers. Peers that startup marked unreachable are not listed, do not
vote in decisions, and have no snapshot rows changed by this child in that run.
A peer that was unreachable in an earlier run is treated like any other
reachable peer when it is passed into a later traversal.

At each directory level, SyncTraversal starts a listing operation for every peer
that is active for that subtree before waiting for any listing result from that
directory level. Each peer listing is retried at most the configured
`--retries-list` total times, including the first try. If a listing still fails,
SyncTraversal reports an error-level diagnostic obligation for that peer and
path. A non-canon failed peer is removed from decisions and recursion for that
directory and its subtree, and SyncTraversal must not create, delete, displace,
copy to, copy from, or update snapshot rows for that peer under the failed
subtree during the run. If the canon peer fails to list a directory after all
tries, SyncTraversal skips decisions for that directory and its subtree on all
peers and performs no peer file or snapshot changes under that subtree. If all
contributing peers fail for a directory, SyncTraversal processes no entries in
that directory or its subtree.

The traversal entry set for a directory is the union of child names returned by
live listings from all active contributing peers and all active subordinate
peers for that directory. Snapshot rows never add a name to the traversal entry
set. Built-in and command-line excludes are removed before any reconciliation
decision is made. Built-in excludes are `.kitchensync/` directories, `.git/`
directories, symbolic links, and special files. Command-line excludes are the
accepted `-x RELPATH` values supplied by the command-line child. A command-line
exclude adds paths to skip; it never makes a built-in excluded path syncable. A
file exclude skips only that file. A directory exclude skips the directory and
all descendants. Excluded paths are left unchanged on every peer and
SyncTraversal must not consult or update snapshot rows for those paths during
the run.

SyncTraversal processes the surviving names in one directory in deterministic
order: case-insensitive lexicographic order, using the original case-sensitive
name as the tie-breaker. It processes every entry in a directory before
recursing into any child directory from that directory. It never recurses into a
directory that is displaced, and only peers that keep or receive the directory
participate in recursion into that directory.

For each visited path, SyncTraversal gathers the live entry state from the
current directory listings and the per-peer snapshot rows needed for that path.
Only contributing peers vote in decisions. A canon peer, when present and
active for that subtree, chooses the outcome directly: a canon file means file
outcome, a canon directory means directory outcome, and canon absence means
absence outcome. A non-canon peer with no snapshot database at startup and a
peer explicitly marked subordinate with `-` do not contribute to decisions in
that run. A subordinate peer still receives the outcome chosen from the active
contributing peers. The same peer may contribute in a later run when it is
reachable, has snapshot history, and is not marked subordinate.

For file decisions without a canon peer, SyncTraversal classifies each
contributing peer's state from its live entry and snapshot row. A live file with
matching untombstoned snapshot state is unchanged. A live file with different
state, a live file over a tombstone, or a live file without a row is a live file
vote. An absent peer with a tombstone row votes deletion using its
`deleted_time`. An absent peer with an untombstoned row votes deletion only when
its `last_seen` is more than five seconds newer than every contributing live
file version. An absent peer with no row does not vote. If no contributing peer
votes for a file, absence is the outcome.

File outcomes use the specified five-second tolerance rules supplied by
FormatRules. Matching unchanged file votes produce the unchanged file outcome.
A modified or new file more than five seconds newer than every other live file
version wins. Multiple deletion votes use the newest deletion estimate. Deletion
wins only when its estimate is more than five seconds newer than every
contributing live file version; a live file that is not more than five seconds
older than the deletion estimate wins over deletion. File existence wins exact
ties with deletion evidence. Among live versions within five seconds of the
newest modification time, byte size breaks ties and the larger file wins. If
the tied live versions have the same modification time within tolerance and the
same byte size, each tied peer keeps its current bytes. A peer that lacks the
file and receives that exact-tie outcome receives bytes from one tied source,
with the tied modification time and byte size.

For directory decisions without a canon peer, directory modification times do
not decide the outcome. A live directory votes for directory existence
regardless of its snapshot row. A contributing peer with no live directory and
no snapshot row does not vote. If every contributing peer that votes has the
directory live, the directory is the outcome. If no contributing peer has a live
directory and every contributing peer with a row is absent, absence is the
outcome. If no contributing peer has either a live directory or a snapshot row,
absence is the outcome.

When live directory evidence conflicts with directory deletion evidence,
SyncTraversal uses the newest deletion estimate from the absent contributing
rows and gathers survival evidence from live files inside the directory
subtree. Directory `mod_time` values do not count as survival evidence. If the
live subtree contains no files, absence wins. If the deletion estimate is more
than five seconds newer than every live file in the directory subtree, absence
wins. If at least one live file in the directory subtree is not more than five
seconds older than the deletion estimate, the directory survives and child
paths inside it are still reconciled by their own rules. If live subtree
evidence cannot be fully listed after the configured listing tries,
SyncTraversal leaves that directory subtree unreconciled for all peers in that
run.

For type conflicts without a canon peer, contributing peers decide the type. If
at least one contributing peer has a file and at least one contributing peer has
a directory at the same path, the file type is the group outcome. The winning
file content is then chosen by the normal file decision rules using only
contributing file entries. A subordinate peer's file does not make the file type
win over a contributing peer's directory, but a subordinate peer with the wrong
type receives the type chosen from contributing peers.

After choosing an outcome, SyncTraversal applies it to all active peers for
that subtree, including subordinate peers. It delegates user-file copy,
replacement, displacement to BAK, and user-file SWAP recovery to CopyStaging.
It delegates snapshot row reads and writes to SnapshotDatabase, and it requests
row changes only after the corresponding listed state, intended file copy,
completed inline directory creation, confirmed absence, or successful
displacement is allowed by the traversal rules. It delegates relative path,
snapshot identity, timestamp parsing, and five-second tolerance comparisons to
FormatRules. It uses only PeerTransportSurface for live peer listings and
direct directory creation checks needed by traversal; it does not call local or
SFTP transport implementations directly.

## Boundaries

SyncTraversal does not parse command-line arguments, validate option text,
select fallback URLs, connect peers, decide startup reachability, decide
first-sync validity, download snapshots, upload snapshots, or print the final
completion line. Those responsibilities belong to the root coordinator and the
startup, command-line, and snapshot children.

SyncTraversal does not own the transport implementation. It must use only
PeerTransportSurface for peer listing and metadata operations. It must not
depend on local filesystem APIs, SFTP APIs, transport-specific handles,
transport-specific path syntax, or rename-over-existing behavior.

SyncTraversal does not own user-file staging mechanics, copy scheduling, copy
retry behavior, bounded streaming, BAK/TMP cleanup, SWAP layout, copy progress
lines, displacement implementation, or dry-run suppression of peer writes. It
requests the needed copy or displacement work through the staging boundary and
uses the result to decide which snapshot updates are allowed.

SyncTraversal does not own SQLite schema, local snapshot file lifecycle,
snapshot upload ordering, stale-row cleanup, recursive tombstone SQL, path hash
generation, timestamp generation, or timestamp formatting. It asks
SnapshotDatabase for row values and row updates and asks FormatRules for the
path and time rules needed to make decisions.

Its invariants are:

- Traversal is a single recursive combined-tree walk driven only by live
  listings of active peers.
- Listings for one directory level are started for all active peers before any
  result is awaited.
- Snapshot rows are decision evidence only; they never add entries to the walk.
- Excluded paths are not decided, copied, displaced, deleted, created, or
  consulted in snapshots by this child.
- Failed listing subtrees and unreachable peers are protected from peer-file
  and snapshot mutation according to the canon and non-canon failure rules.
- Contributing peers choose outcomes; subordinate peers receive outcomes.
- Directory recursion occurs only after all entries at the parent directory
  level have been processed.
