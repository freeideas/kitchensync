# 008_decision-making: File, directory, and conflict decisions

## Behavior
This concern derives from `specs/multi-tree-sync.md` sections "Entry Classification", "Decision Rules", "Directory Decisions", "Type Conflicts", and "Algorithm". It covers how contributing peer live state and snapshot history classify entries, normal bidirectional file conflict resolution, deletion-vs-existence rules, timestamp tolerance, same-time tie handling, directory decision rules, and file-vs-directory conflict resolution.

## $REQ_IDs
- `008.1` -- For a file entry on a contributing peer with a non-tombstone snapshot row, KitchenSync treats the live file as unchanged only when its byte size matches the snapshot row and its modification time is within 5 seconds of the snapshot row.
- `008.2` -- For a file entry on a contributing peer with a non-tombstone snapshot row, KitchenSync treats the live file as modified when its byte size differs from the snapshot row.
- `008.3` -- For a file entry on a contributing peer with a non-tombstone snapshot row, KitchenSync treats the live file as modified when its modification time differs from the snapshot row by more than 5 seconds.
- `008.4` -- For a file entry on a contributing peer with a tombstone snapshot row, KitchenSync treats the live file as modified.
- `008.5` -- For a file entry on a contributing peer with no snapshot row, KitchenSync treats the live file as new.
- `008.6` -- For an absent file entry on a contributing peer with a tombstone snapshot row, KitchenSync treats the peer as a deletion vote with the tombstone `deleted_time` as its deletion estimate.
- `008.7` -- For an absent file entry on a contributing peer with a non-tombstone snapshot row, KitchenSync treats the peer as absent-unconfirmed.
- `008.8` -- For an absent file entry on a contributing peer with no snapshot row, KitchenSync treats the peer as having no vote for that path.
- `008.9` -- When every contributing peer has an unchanged file entry for a path, KitchenSync leaves the file state at that path unchanged.
- `008.10` -- When one or more contributing peers have modified file entries and no deletion outcome wins, KitchenSync selects the winning file by applying the file modification-time and tie rules to the live file entries.
- `008.11` -- When one or more contributing peers have new file entries and no deletion outcome wins, KitchenSync selects the winning file by applying the file modification-time and tie rules to the live file entries.
- `008.12` -- When comparing live file modification times, KitchenSync treats every file within 5 seconds of the maximum modification time as tied for newest.
- `008.13` -- When comparing live file modification times, KitchenSync treats a file more than 5 seconds older than the maximum modification time as older than the maximum.
- `008.14` -- When multiple contributing peers supply deletion estimates for a file path, KitchenSync compares existing files against the most recent deletion estimate.
- `008.15` -- When at least one contributing peer has a deletion vote for a file path and no contributing peer has a live file candidate for that path, KitchenSync selects file absence as the outcome for that path.
- `008.16` -- When a deletion estimate is more than 5 seconds newer than the newest existing file modification time, KitchenSync selects deletion as the file outcome for that path.
- `008.17` -- When the newest existing file modification time is not more than 5 seconds older than a deletion estimate, KitchenSync selects an existing file outcome instead of deletion.
- `008.18` -- For an absent-unconfirmed file entry, KitchenSync treats the peer as a deletion vote with `last_seen` as the deletion estimate only when that peer's `last_seen` is more than 5 seconds newer than the newest existing file modification time.
- `008.19` -- For an absent-unconfirmed file entry, KitchenSync does not treat the peer as a deletion vote when that peer's `last_seen` is NULL.
- `008.20` -- For an absent-unconfirmed file entry, KitchenSync does not treat the peer as a deletion vote when that peer's `last_seen` is not more than 5 seconds newer than the newest existing file modification time.
- `008.21` -- When an absent-unconfirmed file entry does not become a deletion vote and an existing file outcome exists, KitchenSync propagates the existing file outcome to that peer.
- `008.22` -- When live file candidates are tied on modification time and have different byte sizes, KitchenSync selects the larger file as the winning file.
- `008.23` -- When a file existence outcome and a deletion outcome tie, KitchenSync selects the file existence outcome.
- `008.24` -- When a winning file is selected, KitchenSync propagates that file to active peers whose live file at that path does not match the winner by byte size and modification-time tolerance.
- `008.25` -- When a winning file is selected, KitchenSync propagates that file to active peers that lack the entry.
- `008.26` -- When no contributing peer votes for a file path because every contributing peer is absent with no snapshot row, KitchenSync treats the file as absent from the group's view.
- `008.27` -- When a peer already has a live file whose byte size matches the winning file and whose modification time is within 5 seconds of the winning file, KitchenSync does not replace that peer's file.
- `008.28` -- With a canon peer, a canon live file at a path is the winning outcome for that path.
- `008.29` -- With a canon peer, canon absence at a path is the winning outcome for that path.
- `008.30` -- With a canon peer, a canon live directory at a path is the winning outcome for that path.
- `008.31` -- KitchenSync does not use directory modification times to decide the directory outcome for a path.
- `008.32` -- When any contributing peer has a live directory at a path and no canon peer overrides it, KitchenSync selects directory existence as the outcome for that path.
- `008.33` -- When no contributing peer has a live directory at a path and every contributing peer with a snapshot row for that directory has an absent tombstone row, KitchenSync selects directory absence as the outcome for that path.
- `008.34` -- A contributing peer with no snapshot row for an absent directory does not block a directory deletion outcome.
- `008.35` -- When no contributing peer has a directory live in its listing or in any snapshot row, KitchenSync treats the directory as absent from the group's view.
- `008.36` -- With a canon peer, a file-vs-directory conflict is resolved to the canon peer's file, directory, or absence state at that path.
- `008.37` -- Without a canon peer, a file-vs-directory conflict is resolved as a file outcome.
- `008.38` -- Without a canon peer, after a file-vs-directory conflict resolves as a file outcome, KitchenSync selects the winning file by applying the normal file decision rules to the live file entries only.
- `008.39` -- A path present only on subordinate peers and absent from every contributing peer's live listing and snapshot rows is treated as absent from the group's view.

## Notes
This category owns choosing the intended group outcome for a visited path once the eligible contributing peers are known. Canon and subordinate role semantics, automatic subordination, first-sync gating, and no-contributing-peer startup outcomes belong to `017_peer-roles-and-startup-state`; CLI prefix parsing belongs to `003_peer-addressing`; peer connection reachability belongs to `004_peer-connectivity`; snapshot file download/upload belongs to `006_snapshot-lifecycle`; the mechanics of copying, displacing, or updating snapshot rows after the outcome is chosen belong to their own categories.
