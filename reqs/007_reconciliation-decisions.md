# 007_reconciliation-decisions: Reconciliation decisions

## Behavior
This concern derives from `specs/multi-tree-sync.md` sections "Entry
Classification", "Decision Rules", "Directory Decisions", "Type Conflicts",
"Subordinate Peers", and "Offline Peers", `specs/sync.md` sections "Canon Peer
(`+`)" and "Subordinate Peer (`-`)", and `specs/SCENARIOS.md` scenarios S-02
through S-07, S-09, and S-10. It covers the observable choice of group outcome
for files, directories, deletions, absent entries, type conflicts, canon peers,
normal bidirectional peers, subordinate peers, snapshotless peers, timestamp
ties, size ties, and paths with no contributing vote.

## $REQ_IDs

- `007.1` -- When no reachable peer had `.kitchensync/snapshot.db` at startup and no peer is marked canon, KitchenSync exits 1 before changing user files.
- `007.2` -- An existing `.kitchensync/snapshot.db` counts as snapshot history for bidirectional sync even when its `snapshot` table has no rows.
- `007.3` -- When a canon peer has a file at a path, that file is the group outcome for the path on all reachable peers.
- `007.4` -- When a canon peer has a directory at a path, that directory is the group outcome for the path on all reachable peers.
- `007.5` -- When a canon peer lacks a path, absence is the group outcome for the path on all reachable peers.
- `007.6` -- When the canon peer is unreachable, KitchenSync exits with an error before choosing reconciliation outcomes.
- `007.7` -- A non-canon peer that had no `.kitchensync/snapshot.db` at startup does not contribute to reconciliation decisions in that run.
- `007.8` -- A peer marked subordinate with `-` does not contribute to reconciliation decisions in that run.
- `007.9` -- A subordinate peer receives the group outcome chosen from the non-subordinate reachable peers.
- `007.10` -- A peer that was subordinate in an earlier run contributes to reconciliation decisions in a later run when it is reachable, has snapshot history, and is not marked subordinate.
- `007.11` -- An unreachable non-canon peer does not contribute to reconciliation decisions for the run.
- `007.12` -- When all contributing peers that vote for a file have matching unchanged file state, that unchanged file state is the group outcome.
- `007.13` -- When a contributing peer has a modified file more than 5 seconds newer than every other contributing live version of that file, that modified file is the group outcome.
- `007.14` -- When a contributing peer has a new file more than 5 seconds newer than every other contributing live version of that file, that new file is the group outcome.
- `007.15` -- A live file on a contributing peer with an existing tombstone snapshot row participates as a modified file.
- `007.16` -- When deletion evidence for a file is more than 5 seconds newer than every contributing live file version, absence is the group outcome.
- `007.17` -- When a contributing live file version is not more than 5 seconds older than the deletion evidence for that path, the live file is the group outcome.
- `007.18` -- When multiple contributing peers provide deletion evidence for a file, the newest deletion estimate is used for the deletion decision.
- `007.19` -- When a contributing peer is absent for a file with a snapshot row whose `deleted_time` is null and its `last_seen` is more than 5 seconds newer than every contributing live file version, that absence participates as deletion evidence.
- `007.20` -- When a contributing peer is absent for a file with a snapshot row whose `deleted_time` is null and its `last_seen` is null or not more than 5 seconds newer than every contributing live file version, that absence does not participate as deletion evidence.
- `007.21` -- A contributing peer that lacks both a live entry and a snapshot row for a file does not vote on that file.
- `007.22` -- When a file outcome exists and a reachable peer has no vote for that file, that peer receives the file outcome.
- `007.23` -- When no contributing peer votes for a file, absence is the group outcome.
- `007.24` -- When file modification times are within 5 seconds of the newest contributing file modification time, byte size breaks the tie.
- `007.25` -- When tied contributing file versions have different byte sizes, the larger file is the group outcome.
- `007.26` -- When a file existence decision ties with deletion evidence, the file is the group outcome.
- `007.27` -- When contributing file versions have modification times within 5 seconds of the newest modification time and equal byte size, each tied peer keeps its current file bytes.
- `007.28` -- When a peer that lacks a file receives a file outcome from exactly tied contributing files, the received file bytes match one of the tied contributing files.
- `007.29` -- When a peer that lacks a file receives a file outcome from exactly tied contributing files, the received file has the tied modification time and byte size.
- `007.30` -- Directory modification times do not decide directory outcomes.
- `007.31` -- When every contributing peer that votes for a directory has that directory live, the directory is the group outcome.
- `007.32` -- When a live directory conflicts with directory deletion evidence and the deletion estimate is more than 5 seconds newer than every live file in the directory subtree, absence is the group outcome for the directory.
- `007.33` -- When a live directory conflicts with directory deletion evidence and the live directory subtree contains no files, absence is the group outcome for the directory.
- `007.34` -- When a live directory conflicts with directory deletion evidence and at least one live file in the directory subtree is not more than 5 seconds older than the deletion estimate, the directory is the group outcome.
- `007.35` -- When a directory survives because of live subtree file evidence, child paths inside that directory are still reconciled by their own file and directory rules.
- `007.36` -- When no contributing peer has a live directory and every contributing peer with a snapshot row for that directory is absent, absence is the group outcome for the directory.
- `007.37` -- A contributing peer with no snapshot row for a directory does not vote on that directory unless it has the directory live.
- `007.38` -- A contributing peer with a live directory votes for the directory to exist regardless of its snapshot row for that directory.
- `007.39` -- When no contributing peer has a live directory or a snapshot row for a directory, absence is the group outcome for the directory.
- `007.40` -- When live subtree evidence cannot be fully listed for a directory after the configured listing tries, KitchenSync leaves that directory subtree unreconciled for all peers in that run.
- `007.41` -- When a contributing peer has a file and another contributing peer has a directory at the same path without a canon peer, the file type is the group outcome for that path.
- `007.42` -- When a file type wins a non-canon type conflict, the winning file content is chosen by the normal file decision rules using only contributing file entries.
- `007.43` -- A subordinate peer's file does not make the file type win over a contributing peer's directory.
- `007.44` -- A subordinate peer with the wrong type at a path receives the type chosen from contributing peers.

## Notes
This category owns what should happen to a path. The mechanics that copy,
displace, or stage that outcome belong to `008_copy-queue-and-concurrency` and
`009_recoverable-staging`.
