# 011_decision-rules: File decision rules

## Behavior
This concern derives from `specs/multi-tree-sync.md` section "Decision Rules"
(both the canon and non-canon paths) for regular file entries.

It covers how the classified states of contributing peers are resolved into a
single outcome for a file path. With a canon peer present, the canon peer's
state wins unconditionally: canon has the file means push to all others; canon
lacks the file means delete everywhere. Without a canon peer it covers rules
1-6: all-unchanged-and-matching keeps the entry and conforms peers that lack it;
modified and new resolve by newest mod_time; deleted-versus-existing compares the
deletion estimate (`deleted_time`, or `last_seen` under the absent-unconfirmed
rule 4b) against the existing file's mod_time, with deletion winning only when
the estimate is later; same-mod_time-different-size lets the larger file win;
ties keep data. It covers that peers with no snapshot row do not vote but are
propagation targets, that an entry already matching the winner (mod_time within
tolerance and equal byte_size) needs no copy, and that the 5-second tolerance is
applied when comparing peers' mod_times and deletion estimates to the maximum.

Mapping each peer's live and snapshot state into a classification is
`010_entry-classification`. Directory existence decisions and file-versus-
directory type conflicts are `012_directory-and-type-decisions`. Enqueuing and
running the resulting copies is `020_copy-execution`; recording the resulting
snapshot state is `017_snapshot-updates`.

## $REQ_IDs

- `011.1` -- When a canon peer has a file, sync copies that file to every other peer, including subordinate peers.
- `011.2` -- When a canon peer lacks a file that exists on another peer, sync deletes that file from every other peer.
- `011.3` -- When every contributing peer has the file unchanged and the peers' copies already match, sync performs no copy among those matching peers.
- `011.4` -- When every contributing peer has the file unchanged and matching, sync copies that file to any active peer that lacks it, including subordinate peers.
- `011.5` -- When contributing peers hold differing modified versions of a file, sync propagates the version with the newest mod_time to every peer that does not already match it.
- `011.6` -- When a file is new on one or more contributing peers, sync copies the version with the newest mod_time to every peer that lacks it.
- `011.7` -- When more than one peer has deleted a file, sync uses the most recent deletion estimate among the deleting peers.
- `011.8` -- When the deletion estimate exceeds the existing file's mod_time by more than 5 seconds, sync removes the file from every peer that has it.
- `011.9` -- When the existing file's mod_time is within 5 seconds of, or later than, the deletion estimate, sync keeps the file and copies it to every peer that lacks it.
- `011.10` -- For an absent peer whose snapshot row has no `deleted_time`, sync treats `last_seen` as the deletion estimate only when `last_seen` exceeds the maximum mod_time among peers that have the file by more than 5 seconds.
- `011.11` -- For an absent peer whose snapshot row has no `deleted_time` and whose `last_seen` does not exceed the maximum mod_time by more than 5 seconds (or is null), sync re-copies the file to that peer and casts no deletion vote.
- `011.12` -- When contributing peers share the same mod_time but differ in byte_size, sync propagates the larger file.
- `011.13` -- A peer with no snapshot row for a file does not affect which version wins.
- `011.14` -- A peer with no snapshot row for a file receives the winning file once a winner is decided.
- `011.15` -- When the winning file already exists on a peer with a mod_time within 5 seconds and an equal byte_size, sync performs no copy to that peer.
- `011.16` -- When selecting the newest version, a peer whose mod_time is within 5 seconds of the maximum mod_time is treated as tied with the maximum.
- `011.17` -- When selecting the newest version, a peer whose mod_time is more than 5 seconds behind the maximum mod_time loses to it.

## Notes

The Decision Rules section also specifies that when no contributing peer votes
(all absent with no snapshot row) no copy is enqueued and subordinate peers that
have the entry are displaced to BAK/. That displacement outcome is owned by
`020_copy-execution`, so it is not asserted here.
