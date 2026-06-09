# 007_peer-roles: Canon and subordinate peer roles

## Behavior
This concern derives from `specs/sync.md` sections "Canon Peer (`+`)" and
"Subordinate Peer (`-`)", and `specs/multi-tree-sync.md` section "Subordinate
Peers".

It covers what the peer roles mean and how they shape participation: a canon
peer is authoritative and its state wins all conflicts; at most one canon peer
exists per run. A subordinate peer is invisible during decisions (its entries do
not enter `gather_states`) but is brought into conformance afterward - files it
has that should not exist are displaced, files it lacks are copied, directories
are created or removed. Any peer with no snapshot database is automatically
treated as subordinate unless it is the canon peer, so new peers receive the
group's state without influencing it; the explicit `-` prefix is redundant but
harmless for such peers. A subordinate peer's snapshot is still downloaded,
updated during traversal, and (in normal runs) uploaded, so on a later run
without `-` it participates normally.

How a canon or contributing peer's state actually wins a comparison is
`011_decision-rules` and `012_directory-and-type-decisions`. The startup exit
when no canon exists on a first run is `006_run-lifecycle`. The mechanics of
displacing a non-conforming entry are `021_staging-and-displacement`.

Uploading a peer's snapshot database back is a single peer-mutating action with a
single owner: the run lifecycle's writeback (`006.10`), using the snapshot-storage
mechanics (`016_snapshot-storage`). So `007.10` (a subordinate peer's snapshot is
uploaded back) and `007.11` (it is not updated under `--dry-run`) are facets of
that one writeback and belong to the same owner as `006.10`, not to a second
subproject. The decision engine's only share in these is recording the rows
during traversal (`007.12` then follows, because the history is present on the
next run); it must not itself download or upload a snapshot.

## $REQ_IDs
- `007.1` -- When a canon peer (`+`) and another peer hold differing versions of the same file, the canon peer's version is propagated to the group.
- `007.2` -- A subordinate peer's (`-`) entries do not affect sync decisions; the group outcome is the same as if the subordinate peer were absent.
- `007.3` -- A file a subordinate peer has that the group's state does not include is displaced to that peer's BAK/.
- `007.4` -- A file the group has that a subordinate peer lacks is copied to the subordinate peer.
- `007.5` -- A directory the group has that a subordinate peer lacks is created on the subordinate peer.
- `007.6` -- A directory a subordinate peer has that the group's state does not include is displaced to that peer's BAK/.
- `007.7` -- A peer with no `.kitchensync/snapshot.db` is treated as subordinate.
- `007.8` -- A peer with no `.kitchensync/snapshot.db` that is marked canon (`+`) is not treated as subordinate.
- `007.9` -- Adding the `-` prefix to a peer that has no `.kitchensync/snapshot.db` does not change the run's outcome.
- `007.10` -- After a normal run, a subordinate peer's `.kitchensync/snapshot.db` is uploaded back, updated to reflect the run.
- `007.11` -- In `--dry-run`, a subordinate peer's `.kitchensync/snapshot.db` on the peer is not updated.
- `007.12` -- On a later normal run without the `-` prefix, a peer that was previously subordinate participates in decisions using its snapshot history.

## Notes
The startup validation that at most one `+` peer is allowed per run is parse-time
validation owned by `006_run-lifecycle`; this file covers only what the canon and
subordinate roles mean for participation, not how they are validated at startup.
