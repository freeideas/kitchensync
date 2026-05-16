# Sync Decision Engine

A Java 21 library for making one-entry synchronization decisions for a set of
file-tree peers. It consumes the live state reported for one relative path,
each peer's previous snapshot row for that path, and each peer's role. It
returns the authoritative state for that path plus declarative filesystem and
snapshot effects for each peer.

The library is pure decision logic. It does not list directories, read files,
parse command lines or URLs, apply ignore patterns, open network connections,
perform copies or renames, store SQLite databases, generate timestamps, schedule
work, traverse subdirectories, clean BAK/TMP directories, or log diagnostics.
Callers provide already-listed peer state and execute the returned effects.

## Public API

The API may use normal Java classes, records, interfaces, or equivalent types,
but it must expose this behavior.

### Data Shapes

`PeerId`

An opaque stable identifier chosen by the caller. Equality compares the
identifier value only.

`PeerRole`

| Value | Meaning |
| --- | --- |
| `canon` | This peer's live state wins unconditionally. At most one peer may have this role. |
| `normal` | This peer contributes to decisions and receives outcomes. |
| `subordinate` | This peer receives outcomes but does not contribute to decisions. |

`EntryKind`

| Value | Meaning |
| --- | --- |
| `file` | A regular file. |
| `directory` | A directory. |

`LiveEntry`

| Field | Meaning |
| --- | --- |
| `kind` | `file` or `directory`. |
| `mod_time` | File or directory modification time as a UTC instant. Directory times are carried through but not used for decisions. |
| `byte_size` | File size in bytes, or `-1` for directories. |

`SnapshotRow`

| Field | Meaning |
| --- | --- |
| `kind` | `file` or `directory`, matching the stored row's `byte_size` convention. |
| `mod_time` | Last observed modification time. |
| `byte_size` | File size in bytes, or `-1` for directories. |
| `last_seen` | UTC instant when the entry was last confirmed present, or absent. |
| `deleted_time` | UTC deletion estimate for a tombstone, or absent while the entry exists. |

`DecisionInput`

| Field | Meaning |
| --- | --- |
| `relative_path` | Caller-supplied path label used only in returned effects. The engine treats it as opaque text. |
| `peers` | Ordered map of `PeerId` to `PeerRole` for peers active at this directory level. The order is used only to break otherwise identical source-peer ties deterministically. |
| `live_entries` | Map of `PeerId` to `LiveEntry` for peers where this path exists in the live listing. Missing means absent. |
| `snapshot_rows` | Map of `PeerId` to `SnapshotRow` for peers with a previous row for this path. Missing means no row. |

The timestamp tolerance is fixed at five seconds. A time is considered tied
with another time when the absolute difference is less than or equal to five
seconds. A time wins only when it is more than five seconds later.

`AuthoritativeState`

| Field | Meaning |
| --- | --- |
| `kind` | `absent`, `file`, or `directory`. |
| `source_peer` | The peer to copy from when `kind` is `file`; absent for `absent` and optional for `directory`. |
| `mod_time` | Winning file modification time when `kind` is `file`; absent otherwise. |
| `byte_size` | Winning file size when `kind` is `file`; absent otherwise. |

`FilesystemEffect`

| Value | Meaning |
| --- | --- |
| `keep` | No filesystem operation is required for this peer at this path. |
| `copy_file` | Copy the winning file from `source_peer` to this peer at `relative_path`. |
| `create_directory` | Create the directory at `relative_path`. |
| `displace` | Move the existing entry at `relative_path` aside through the caller's displacement mechanism. |

Effects are returned in execution order per peer. For example, a peer that has a
file where the authoritative state is a directory receives `displace` followed
by `create_directory`.

`SnapshotEffect`

| Value | Meaning |
| --- | --- |
| `confirm_present` | The peer's live listing already confirms the authoritative entry is present. Upsert the row from that peer's listed metadata, clear `deleted_time`, and set `last_seen` to a caller-generated current timestamp. |
| `copy_pending` | A file copy to this peer is required. Upsert winning file metadata, clear `deleted_time`, and leave `last_seen` unchanged or absent until the copy completes. |
| `create_directory_confirmed` | Directory creation is required and is considered confirmed after the caller performs it. Upsert directory metadata, clear `deleted_time`, and set `last_seen` after creation succeeds. |
| `mark_absent` | The peer is confirmed absent for a state that should not exist. If its row is not already tombstoned, set `deleted_time` from the row's current `last_seen`; do not update `last_seen`. |
| `mark_displaced` | The caller must displace this peer's entry. Set `deleted_time` from the row's current `last_seen`; for directories, cascade that deletion estimate to descendants in the same peer's snapshot store. |
| `no_snapshot_change` | No snapshot update is required for this peer at this path. |

`EntryDecision`

| Field | Meaning |
| --- | --- |
| `authoritative_state` | The chosen group state for this path, or `absent`. |
| `filesystem_effects` | Ordered effects per peer. |
| `snapshot_effects` | Ordered snapshot effects per peer. |
| `recurse_peers` | Peers that should participate when the authoritative state is a directory. Empty for files and absent states. |
| `skipped` | True only when there are no active contributing peers, so callers must skip this path and its subtree. |

### Operations

`SyncDecisionEngine.decide_entry(input) -> EntryDecision`

Returns a deterministic decision for one path. The operation has no I/O side
effects and must not write to stdout or stderr.

## Observable Behavior

### Contributing Peers

Only `canon` and `normal` peers contribute to decisions. `subordinate` peers are
ignored while choosing the authoritative state, then brought into conformance
with that state.

If the input contains no active contributing peer, the result is `skipped =
true`, with no filesystem or snapshot effects. This models a directory level
where every contributing peer failed listing and subordinate files must not be
displaced.

### Canon Peer

When a canon peer is present, its live state wins unconditionally:

- Canon has a file: the file is authoritative and is copied to peers that do not
  already have matching file metadata.
- Canon has a directory: the directory is authoritative; wrong-type entries are
  displaced, missing directories are created, and directory peers recurse.
- Canon lacks the path: the path is absent; all other peers with a live entry
  receive `displace`.

### File Classification

For each contributing peer with a file live entry:

| Live state | Snapshot row | `deleted_time` | Classification |
| --- | --- | --- | --- |
| Live file, same `mod_time` within tolerance | Exists | absent | `unchanged` |
| Live file, different `mod_time` beyond tolerance | Exists | absent | `modified` |
| Live file | Exists | present | `modified` |
| Live file | No row | - | `new` |

For each contributing peer without a live file entry:

| Live state | Snapshot row | `deleted_time` | Classification |
| --- | --- | --- | --- |
| Absent | Exists | present | `deleted` using `deleted_time` as estimate |
| Absent | Exists | absent | `absent_unconfirmed` using `last_seen` as the possible deletion estimate |
| Absent | No row | - | `no_opinion` |

Directories use the directory rules below instead of file classifications based
on modification time.

### File Decision Rules Without Canon

When no canon peer exists, only contributing peers participate in these rules:

1. If all file votes are unchanged and all live files have matching metadata,
   keep the file where it already exists and copy it only to peers that lack it.
2. Modified file votes beat unchanged file votes as a class. If one or more
   modified files exist, choose among the modified files by latest `mod_time`;
   an unchanged file does not win merely because its `mod_time` is later.
3. New files participate in the same class as modified files. If one or more
   modified or new files exist, choose among that combined class by latest
   `mod_time`.
4. Deleted votes compete with existing files by comparing the latest deletion
   estimate against the latest live file `mod_time`. A deletion estimate wins
   only when it is more than five seconds later than the file time. Otherwise
   the existing file wins.
5. For an `absent_unconfirmed` row, `last_seen` is a deletion estimate only
   when it is more than five seconds later than the latest live file `mod_time`.
   If `last_seen` is absent or not later beyond tolerance, the absence is a
   failed or incomplete copy and does not vote for deletion.
6. When file `mod_time` values are tied within tolerance, the larger
   `byte_size` wins.
7. Remaining ties keep data: existence beats deletion. If multiple live files
   are still tied with the same winning metadata, the first contributing peer in
   input order is selected as `source_peer`.

Peers with no snapshot row and no live entry do not vote. They are targets for
the winning state once one exists.

If no contributing peer votes for existence or deletion, the authoritative state
is absent. Subordinate peers that have a live entry receive `displace`.

If a peer already has a file whose `mod_time` is within tolerance of the winning
file and whose `byte_size` matches, it receives `keep`, not `copy_file`.

### Directory Decisions Without Canon

Directory decisions are existence-based:

- If any contributing peer has the directory live, the directory is
  authoritative and should exist on every active peer.
- If no contributing peer has the directory live, and every contributing peer
  with a snapshot row for the directory has a tombstone and is absent live, the
  directory is absent and live entries on other peers are displaced.
- If no contributing peer has the directory live and no contributing peer has a
  snapshot row for it, the directory is absent. Subordinate live entries are
  displaced.
- A contributing peer with no snapshot row has no opinion and does not block
  deletion.

Directory `mod_time` values do not affect the decision.

### Type Conflicts

When the same path is a file on one contributing peer and a directory on another
contributing peer:

- With a canon peer, the canon peer's state wins.
- Without a canon peer, the file wins. Directory entries are displaced, then the
  winning file is selected by the file decision rules applied to file entries
  only.

The same conformance rule applies to subordinate peers: a subordinate with the
wrong type receives `displace`, then any creation or copy effect required by the
authoritative state.

### Snapshot Effects

Snapshot effects describe required updates but do not generate timestamps or
modify storage.

- A peer whose live entry already matches the authoritative state receives
  `confirm_present`.
- A destination peer for a file copy receives `copy_pending`; after the caller
  completes the copy successfully, the caller must set `last_seen` separately.
- A peer that must create an authoritative directory receives
  `create_directory_confirmed`; the caller sets `last_seen` after creation
  succeeds.
- A peer that is confirmed absent for an absent authoritative state receives
  `mark_absent` when it has an existing untombstoned row.
- A peer whose live entry must be displaced receives `mark_displaced`.

## Error Behavior

Invalid inputs fail with `invalid_input` and no partial result:

- Duplicate peer identifiers.
- More than one canon peer.
- A `live_entries` or `snapshot_rows` key not present in `peers`.
- A file entry with a negative `byte_size`.
- A directory entry whose `byte_size` is not `-1`.
- A snapshot row with `deleted_time` present and no `last_seen` value.

Missing live entries and missing snapshot rows are valid states. The engine does
not validate path syntax, filesystem accessibility, timestamp formatting, or
whether a listed peer is reachable; callers perform those checks before calling
the engine.

The library must not throw transport-specific, database-specific, or filesystem
errors because it performs none of those operations.

## Examples

### Newer File Wins

Input:

```text
relative_path = "notes/todo.txt"
peers = { A: normal, B: normal }
live_entries = {
  A: file mod_time=2026-05-15T10:00:00Z byte_size=4,
  B: file mod_time=2026-05-15T10:05:30Z byte_size=8
}
snapshot_rows = {
  A: file mod_time=2026-05-15T10:00:00Z byte_size=4 last_seen=2026-05-15T10:01:00Z deleted_time=absent,
  B: file mod_time=2026-05-15T10:00:00Z byte_size=4 last_seen=2026-05-15T10:01:00Z deleted_time=absent
}
```

Output:

```text
authoritative_state = file source_peer=B mod_time=2026-05-15T10:05:30Z byte_size=8
filesystem_effects = {
  A: [copy_file from B],
  B: [keep]
}
snapshot_effects = {
  A: copy_pending,
  B: confirm_present
}
recurse_peers = []
skipped = false
```

### Deletion Estimate Wins

Input:

```text
relative_path = "old.txt"
peers = { A: normal, B: normal }
live_entries = {
  B: file mod_time=2026-05-15T10:00:00Z byte_size=12
}
snapshot_rows = {
  A: file mod_time=2026-05-15T09:00:00Z byte_size=12 last_seen=2026-05-15T11:00:00Z deleted_time=absent,
  B: file mod_time=2026-05-15T10:00:00Z byte_size=12 last_seen=2026-05-15T10:01:00Z deleted_time=absent
}
```

Output:

```text
authoritative_state = absent
filesystem_effects = {
  A: [keep],
  B: [displace]
}
snapshot_effects = {
  A: mark_absent,
  B: mark_displaced
}
recurse_peers = []
skipped = false
```

`A`'s absence is treated as a deletion because its `last_seen` is more than
five seconds later than `B`'s live file modification time.

### Directory Conformance With Subordinate Wrong Type

Input:

```text
relative_path = "album"
peers = { A: normal, B: normal, C: subordinate }
live_entries = {
  A: directory mod_time=2026-05-15T12:00:00Z byte_size=-1,
  C: file mod_time=2026-05-15T12:30:00Z byte_size=99
}
snapshot_rows = {}
```

Output:

```text
authoritative_state = directory
filesystem_effects = {
  A: [keep],
  B: [create_directory],
  C: [displace, create_directory]
}
snapshot_effects = {
  A: confirm_present,
  B: create_directory_confirmed,
  C: mark_displaced then create_directory_confirmed
}
recurse_peers = [A, B, C]
skipped = false
```

The subordinate peer's file does not influence the decision, but it is displaced
so the peer can conform to the authoritative directory.

## Testing Requirements

Tests are black-box tests of the public API. No external service account, SFTP
server, local filesystem fixture, SQLite database, or network access is
required.

Required scenarios:

- Canon file, canon directory, and canon absence each win unconditionally.
- Subordinate peers never influence the authoritative state but receive copy,
  create, and displace effects needed for conformance.
- No active contributing peers returns `skipped = true` with no effects.
- Unchanged, modified, new, deleted, absent-unconfirmed, and no-opinion file
  classifications drive the specified file decisions.
- Deletion estimates win only when later than the live file time by more than
  five seconds.
- Timestamp differences of exactly five seconds are ties.
- Same-time file ties use larger `byte_size`; remaining ties choose the first
  tied contributing peer in input order.
- A peer with matching winning file metadata receives `keep`, not `copy_file`.
- Directory decisions ignore directory modification time.
- Directory tombstones can delete a directory when no contributing peer has it
  live, and no-row peers do not block deletion.
- Type conflicts use canon when present and otherwise make files win over
  directories.
- Snapshot effects distinguish confirmed presence, pending file copies,
  confirmed directory creation, absence, and displacement.
- Invalid inputs report `invalid_input` without emitting stdout or stderr.

Scenarios to avoid:

- Do not test real file copies, renames, BAK or TMP path construction, cleanup,
  or atomic swap behavior.
- Do not test SFTP, SSH authentication, connection pooling, local filesystem
  behavior, or transport error mapping.
- Do not test SQLite schema, path hashing, tombstone purging, or recursive CTE
  execution.
- Do not test command-line parsing, URL normalization, fallback URL selection,
  or peer startup reachability.
- Do not test ignore-pattern parsing or `.syncignore` resolution order.

## Semantic Anchors

This specification is anchored in the semantic source sections for multi-tree
synchronization, subordinate peers, entry classification, decision rules,
directory decisions, type conflicts, snapshot updates, timestamp tolerance, and
the rule that listing failures can remove all contributing peers for a subtree.
