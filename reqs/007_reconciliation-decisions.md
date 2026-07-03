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

## Notes
This category owns what should happen to a path. The mechanics that copy,
displace, or stage that outcome belong to `008_copy-queue-and-concurrency` and
`009_recoverable-staging`.

## $REQ_IDs
