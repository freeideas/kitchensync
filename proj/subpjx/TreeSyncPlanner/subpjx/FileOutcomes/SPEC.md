# FileOutcomes:

## Purpose

FileOutcomes classifies per-peer file state for one already-visible file path
and selects the file outcome intents for that path. It receives active peer
role facts, live file facts, and per-peer snapshot row facts from the
TreeSyncPlanner facade. It returns structured facts saying whether the group
outcome is a file, deletion, or no file, plus copy and displacement intents
that the parent can place in the full action plan.

This child owns file decisions only. It does not list directories, choose which
paths are visible, resolve file-versus-directory conflicts, execute copies,
move entries to `BAK/`, update file modification times, or write snapshot rows.

## Responsibilities

FileOutcomes exposes a file classification operation for one peer and one file
path. The operation accepts that peer's live file fact, absent fact, snapshot
row fact, and `last_seen` value. It returns one of these structured states:

- unchanged live file;
- modified live file;
- new live file;
- deleted file with a deletion estimate;
- absent-unconfirmed file;
- absent file with no row and no vote.

A live file with a snapshot row whose `deleted_time` is NULL, whose byte size
matches, and whose modification time is within 5 seconds of the snapshot row
modification time is unchanged. A live file with the same kind of row is
modified when the byte size differs or the modification time is more than 5
seconds different from the row modification time. A live file with a row whose
`deleted_time` is not NULL is modified. A live file with no row is new.

An absent file with a row whose `deleted_time` is not NULL is deleted, using
that `deleted_time` as the deletion estimate. An absent file with a row whose
`deleted_time` is NULL is absent-unconfirmed. An absent file with no row
contributes no vote for the path.

FileOutcomes exposes a file outcome operation for one path. The operation
accepts the classified state of every active peer for that path and the role
facts that identify canon, contributing, subordinate, and targetable peers. It
returns:

- the selected group outcome for the path;
- the selected source peer or set of identical source peers when the outcome is
  an existing file;
- copy intents for active peers that need the winning file;
- deletion or displacement intents for active peers that have a file when the
  selected outcome is deletion or no file;
- per-peer decision facts that explain which peers matched the winner and which
  peers were not selected for copy.

With a canon peer, the canon peer's file state wins unconditionally. If the
canon peer has the file, that file is the outcome for every other active peer.
If the canon peer lacks the file, deletion is the outcome for every other
active peer that has the file. Other peers must not change the canon file
decision.

Without a canon peer, only contributing peers vote. Subordinate peers do not
contribute votes, but active subordinate peers remain targets for the outcome
selected from contributing peers.

When all contributing peers that have a file are unchanged and matching, that
unchanged file is the group outcome. No copy is selected between contributing
peers that already match, and an active peer that lacks the file is selected to
receive it.

For live file votes selected from modified classifications, the newest
modification time selects the winning file. For live file votes selected from
new classifications, the newest modification time selects the winning file,
and that new-file winner is propagated to peers that lack the file, including
peers with no snapshot row for the file.

When comparing live file votes, any modification time within 5 seconds of the
maximum modification time is tied with the maximum; a modification time more
than 5 seconds behind the maximum loses to the maximum. Among tied live file
votes, the larger byte size selects the winning file. Files whose modification
times are tied and whose byte sizes are equal are identical for planning even
when their bytes differ. No copy is selected between peers whose files are
identical. A peer that needs a file available from multiple identical source
peers receives the file from one of those source peers.

When deleted votes and existing file votes both exist, FileOutcomes compares
the most recent deletion estimate with the winning existing file modification
time. If multiple peers deleted the file, the most recent deletion estimate is
used. A deletion estimate more than 5 seconds newer than the existing file
modification time selects deletion. An existing file whose modification time is
not more than 5 seconds older than the deletion estimate wins over deletion.
When an existing file and deletion are tied, the existing file wins.

An absent-unconfirmed peer contributes a deletion vote only when `last_seen` is
present and more than 5 seconds newer than the maximum modification time of
peers that have the file. In that case, `last_seen` is the deletion estimate.
An absent-unconfirmed peer whose `last_seen` is NULL contributes no deletion
vote. An absent-unconfirmed peer whose `last_seen` is not more than 5 seconds
newer than the maximum live file modification time also contributes no deletion
vote. An absent-unconfirmed peer that contributes no deletion vote is selected
to receive the file when an existing file wins.

If every contributing peer is absent with no snapshot row for a file, the file
does not exist in the group outcome. No copy outcome is selected for that file.
An active subordinate peer that has the file is selected for displacement to
`BAK/`.

A peer that already has the winning byte size and a live file modification time
within 5 seconds of the winning modification time is not selected for copy. The
copy source fact must preserve the exact source filename case supplied by the
parent planner for the selected source peer.

## Boundaries

FileOutcomes does not decide whether a path is visible, apply excludes, fetch
snapshot rows, inspect local or remote file metadata, retry listings, recurse
into directories, resolve directory outcomes, resolve type conflicts, execute
file copies, delete files, move files to `BAK/`, set timestamps, update SQLite,
or format stdout.

The child returns structured planner facts and intents only. The parent
TreeSyncPlanner facade is responsible for calling this child only for file
paths that are visible after traversal, excludes, listing failure handling, and
type-conflict routing have already been decided.

FileOutcomes has no transport, storage, stdout, or process-exit error
obligations. Its error obligation is decision safety: it must not invent
missing metadata, fetch additional state, or treat malformed facts as votes
when the supplied peer-role facts and per-peer file facts cannot describe one
decision for one visible file path.

FileOutcomes must preserve these invariants:

- classification uses only the live file fact, snapshot row fact, absent fact,
  and `last_seen` for the peer being classified;
- the 5-second tolerance is applied consistently to snapshot comparison, live
  file vote comparison, deleted-versus-existing comparison, and
  absent-unconfirmed deletion votes;
- canon file decisions are final and cannot be changed by non-canon peers;
- subordinate peers never vote in non-canon file decisions;
- active subordinate peers can receive copy or displacement intents after a
  contributing outcome is selected;
- absent files with no row contribute no vote;
- identical live files do not copy between each other;
- peers already matching the winning size and time tolerance do not receive a
  copy intent;
- deletion outcomes produce only deletion or displacement intents, never copy
  intents;
- no-file outcomes for all-no-row contributing inputs produce no copy intents
  and displace subordinate live files;
- all returned intents are about one file path only and contain no execution
  side effects.
