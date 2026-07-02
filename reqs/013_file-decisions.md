# 013_file-decisions: File classification and reconciliation rules

## Behavior
This concern derives from `specs/multi-tree-sync.md` sections "Entry
Classification" and "Decision Rules", and `specs/sync.md` sections "Canon Peer
(+)" and "Errors". It covers how live file state and per-peer snapshot rows are
classified, how modified, new, deleted, absent-unconfirmed, same-time,
same-size, and tie cases are decided for files, how the 5-second tolerance is
applied, how no-row peers vote, and when file copy or deletion outcomes are
chosen.

## $REQ_IDs
- `013.1` -- A live file whose snapshot row has NULL `deleted_time`, matching byte size, and matching modification time is treated as unchanged.
- `013.2` -- A live file whose snapshot row has NULL `deleted_time` and a different byte size is treated as modified.
- `013.3` -- A live file whose snapshot row has NULL `deleted_time` and a modification time more than 5 seconds different from the snapshot row modification time is treated as modified.
- `013.4` -- A live file whose snapshot row has non-NULL `deleted_time` is treated as modified.
- `013.5` -- A live file with no snapshot row for that peer is treated as new.
- `013.6` -- An absent file whose snapshot row has non-NULL `deleted_time` is treated as deleted with `deleted_time` as its deletion estimate.
- `013.7` -- An absent file whose snapshot row has NULL `deleted_time` is treated as absent-unconfirmed.
- `013.8` -- An absent file with no snapshot row for that peer contributes no vote for that entry.
- `013.9` -- A canon peer that has the file selects that file as the outcome for all other active peers.
- `013.10` -- A canon peer that lacks the file selects deletion as the outcome for every other active peer that has the file.
- `013.11` -- A canon peer's file decision is not changed by any other peer's state for the same file.
- `013.12` -- A run with an unreachable canon peer exits with status 1.
- `013.13` -- A first run with no canon and no peer snapshot history prints `First sync? Mark the authoritative peer with a leading +` to stdout.
- `013.14` -- A first run with no canon and no peer snapshot history exits with status 1.
- `013.15` -- A run with no reachable contributing peer prints `No contributing peer reachable - cannot make sync decisions` to stdout.
- `013.16` -- A run with no reachable contributing peer exits with status 1.
- `013.17` -- Without a canon peer, subordinate peers do not contribute votes to file decisions.
- `013.18` -- Without a canon peer, active subordinate peers are targets for the file outcome selected from contributing peers.
- `013.19` -- When all contributing peers with a file are unchanged and matching, that unchanged file is the group outcome.
- `013.20` -- When all contributing peers with a file are unchanged and matching, no copy outcome is selected between contributing peers that already match.
- `013.21` -- When all contributing peers with a file are unchanged and matching, an active peer that lacks the file is selected to receive the file.
- `013.22` -- Among modified file votes, the newest modification time selects the winning file.
- `013.23` -- Among new file votes, the newest modification time selects the winning file.
- `013.24` -- A new-file winner is selected for propagation to peers that lack the file, including peers with no snapshot row for the file.
- `013.25` -- When deleted votes and existing file votes both exist, the deletion estimate is compared with the existing file modification time.
- `013.26` -- When multiple peers have deleted the file, the most recent deletion estimate is used for the deleted-versus-existing comparison.
- `013.27` -- A deletion estimate that is newer than the existing file modification time by more than 5 seconds selects deletion as the outcome.
- `013.28` -- An existing file whose modification time is not more than 5 seconds older than the deletion estimate wins over deletion.
- `013.29` -- An absent-unconfirmed peer whose `last_seen` is more than 5 seconds newer than the maximum modification time of peers that have the file contributes a deletion vote using `last_seen` as the deletion estimate.
- `013.30` -- An absent-unconfirmed peer whose `last_seen` is NULL contributes no deletion vote.
- `013.31` -- An absent-unconfirmed peer whose `last_seen` is not more than 5 seconds newer than the maximum modification time of peers that have the file contributes no deletion vote.
- `013.32` -- An absent-unconfirmed peer that contributes no deletion vote is selected to receive the file when an existing file wins.
- `013.33` -- Among file votes tied on modification time, the larger byte size selects the winning file.
- `013.34` -- When an existing file and a deletion are tied, the existing file selects the outcome.
- `013.35` -- Files whose modification times are tied and whose byte sizes are equal are treated as identical even when their bytes differ.
- `013.36` -- No copy outcome is selected between peers whose files are treated as identical.
- `013.37` -- A peer that needs a file identical on multiple source peers receives the file from one of those identical source peers.
- `013.38` -- If every contributing peer is absent with no snapshot row for a file, the file does not exist in the group outcome.
- `013.39` -- If every contributing peer is absent with no snapshot row for a file, no copy outcome is selected for that file.
- `013.40` -- If every contributing peer is absent with no snapshot row for a file, an active subordinate peer that has the file is selected for displacement to `BAK/`.
- `013.41` -- A peer that already has the winning file modification time within 5 seconds and the winning byte size is not selected for a copy.
- `013.42` -- A live file modification time within 5 seconds of the snapshot row modification time is treated as matching for file classification.
- `013.43` -- A live file modification time more than 5 seconds different from the snapshot row modification time is treated as different for file classification.
- `013.44` -- When comparing file votes, a peer modification time within 5 seconds of the maximum modification time is treated as tied with the maximum.
- `013.45` -- When comparing file votes, a peer modification time more than 5 seconds behind the maximum modification time loses to the maximum.

## Notes
This file covers decision selection for file entries. Copy execution,
displacement mechanics, and snapshot row writes belong to later categories.
