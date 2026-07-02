# 014_directory-and-type-decisions: Directory reconciliation and type conflicts

## Behavior
This concern derives from `specs/multi-tree-sync.md` sections "Directory
Decisions", "Type Conflicts", and "Algorithm", and `specs/sync.md` section
"Case Sensitivity". It covers directory existence decisions, directory deletion
evidence, live subtree file evidence, empty-directory deletion conflicts,
surviving-directory recursion, whole-directory displacement, file-versus-
directory conflict resolution, and filename case preservation.

## $REQ_IDs

- `014.1` -- Directory modification times do not affect directory existence decisions.
- `014.2` -- A canon peer's live directory makes that directory exist on every active peer.
- `014.3` -- A canon peer's missing path makes that path absent on every active peer.
- `014.4` -- Without a canon peer, a directory that is live on every voting contributing peer exists on every active peer.
- `014.5` -- A contributing peer with a live directory votes for directory existence even when its snapshot row differs.
- `014.6` -- A contributing peer with no live directory and no snapshot row for that directory does not vote on that directory's existence.
- `014.7` -- A directory deletion vote uses the absent peer's `deleted_time` when that value is present.
- `014.8` -- A directory deletion vote uses the absent peer's `last_seen` when `deleted_time` is absent.
- `014.9` -- A live-directory conflict uses the newest modification time of live files anywhere under the live directory as survival evidence.
- `014.10` -- Directories under a live directory do not provide survival evidence for that directory.
- `014.11` -- A live directory subtree containing no files provides no survival evidence for that directory.
- `014.12` -- A live-directory conflict uses the most recent deletion estimate when more than one contributing peer votes deletion.
- `014.13` -- When the newest deletion estimate exceeds the survival evidence by more than the five-second tolerance, the directory is displaced on every active peer that has it.
- `014.14` -- When a live-directory conflict has no survival evidence, the directory is displaced on every active peer that has it.
- `014.15` -- When directory deletion wins, the directory is not recreated on active peers that lack it.
- `014.16` -- When directory deletion wins, sync does not recurse into that directory.
- `014.17` -- When the newest deletion estimate does not exceed the survival evidence by more than the five-second tolerance, the directory exists on every active peer.
- `014.18` -- When a directory survives a deletion conflict, sync recurses into that directory.
- `014.19` -- When a directory survives because of newer file evidence, newer child files remain eligible to propagate by the file decision rules.
- `014.20` -- When a directory survives because of newer file evidence, older child files remain eligible for removal by the file deletion rules.
- `014.21` -- If collecting survival evidence fails after all allowed listing tries, no peer is modified under that directory subtree during that run.
- `014.22` -- If no contributing peer has the directory live, at least one contributing peer has a snapshot row for it, and every contributing peer with a snapshot row is absent, the directory is displaced on every active peer that has it.
- `014.23` -- If no contributing peer has the directory live or in a snapshot row, subordinate peers that have the directory are displaced.
- `014.24` -- A directory selected for displacement is moved as one directory before any of its children are independently visited.
- `014.25` -- With a canon peer, a canon file at a file-versus-directory path displaces directories at that path and syncs the file to active peers.
- `014.26` -- With a canon peer, a canon directory at a file-versus-directory path displaces files at that path and syncs the directory to active peers.
- `014.27` -- With a canon peer, a missing canon path in a file-versus-directory conflict displaces that path on active peers that have it.
- `014.28` -- Without a canon peer, a contributing file wins over a contributing directory at the same path.
- `014.29` -- Without a canon peer, the winning file in a file-versus-directory conflict is selected from contributing file entries only.
- `014.30` -- A subordinate file at a path does not make that path's file beat a contributing peer's directory.
- `014.31` -- After a file-versus-directory decision, a subordinate path with the losing type is displaced and replaced as needed.
- `014.32` -- Synced filenames preserve the exact case reported by the source filesystem.

## Notes
This file covers decisions for directories and mixed file/directory paths.
Staging paths, BAK moves, and snapshot cascades belong to the staging and
snapshot-update categories.
