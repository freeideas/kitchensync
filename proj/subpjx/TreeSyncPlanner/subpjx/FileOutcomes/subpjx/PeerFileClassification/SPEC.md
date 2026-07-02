# PeerFileClassification:

## Purpose

PeerFileClassification classifies one peer's supplied facts for one visible
file path. It receives only the live file fact, absent-file fact, snapshot row
fact, and `last_seen` value that the parent FileOutcomes facade already has for
that peer. It returns a structured classification that later file outcome
selection can use as that peer's vote or non-vote.

This child owns per-peer file classification only. It does not compare peers,
select a group outcome, choose copy sources, produce copy or displacement
intents, inspect files, read snapshots, update snapshots, or access transports.

## Responsibilities

PeerFileClassification exposes one classification operation for one peer and
one file path. The operation accepts supplied facts describing whether the peer
has a live file, whether the peer is absent for the path, the peer's optional
snapshot row for the path, and the peer's optional `last_seen` value. The
operation returns exactly one of these states:

- unchanged live file;
- modified live file;
- new live file;
- deleted file with a deletion estimate;
- absent-unconfirmed file;
- absent file with no snapshot row and no vote.

A live file with no snapshot row is new.

A live file with a snapshot row whose `deleted_time` is not NULL is modified.
The prior deletion marker does not make the live file deleted; the live file is
present and differs from the recorded deleted state.

A live file with a snapshot row whose `deleted_time` is NULL is unchanged only
when its byte size matches the snapshot row byte size and its modification time
is within 5 seconds of the snapshot row modification time. The comparison is
absolute: a live modification time up to and including 5 seconds earlier or
later than the snapshot row modification time matches.

A live file with a snapshot row whose `deleted_time` is NULL is modified when
its byte size differs from the snapshot row byte size. It is also modified when
its modification time is more than 5 seconds earlier or later than the snapshot
row modification time.

An absent file with no snapshot row contributes no vote for the path.

An absent file with a snapshot row whose `deleted_time` is not NULL is deleted.
The returned deleted classification must carry that `deleted_time` value as the
deletion estimate.

An absent file with a snapshot row whose `deleted_time` is NULL is
absent-unconfirmed. This child does not decide whether the peer's `last_seen`
later becomes a deletion vote; it preserves the absent-unconfirmed state for
the group outcome child to evaluate with other peers' live file times.

The returned live classifications must preserve the live file metadata needed
by group outcome selection: byte size and modification time. The returned
deleted classification must preserve the deletion estimate. The returned
absent-unconfirmed classification must preserve the peer's optional `last_seen`
value if the boundary type includes it for later decision making.

## Boundaries

PeerFileClassification is pure decision logic. It does not list directories,
normalize paths, choose visible paths, fetch peer metadata, read database rows,
write database rows, read file contents, compare file bytes, execute copies,
delete files, move files to `BAK/`, set modification times, format stdout, or
decide process exit status.

The parent facade is responsible for supplying coherent facts for exactly one
peer and one file path. A valid input describes either a live file or an absent
file, and may include zero or one snapshot row for that same peer and path.

This child has no transport, storage, snapshot, stdout, or process-exit error
obligations. Its error obligation is decision safety: if the supplied facts do
not describe exactly one live-or-absent state for one peer and one file path,
or if required metadata for the selected state is missing, the operation must
return an explicit invalid-input error rather than inventing metadata or
silently producing a vote.

PeerFileClassification must preserve these invariants:

- classification uses only the supplied facts for the peer being classified;
- a live fact always produces a live classification: unchanged, modified, or
  new;
- an absent fact always produces a non-live classification: deleted,
  absent-unconfirmed, or no-vote;
- absent files with no snapshot row contribute no vote;
- non-NULL `deleted_time` on an absent file is the deletion estimate exactly as
  supplied;
- non-NULL `deleted_time` on a live file makes the live file modified;
- NULL `deleted_time` snapshot comparisons require both matching byte size and
  modification time within 5 seconds to classify a live file as unchanged;
- modification time comparison for snapshot matching uses an inclusive
  5-second tolerance;
- modification time differences greater than 5 seconds classify a live file as
  modified;
- no classification operation reads, writes, or mutates external state.
