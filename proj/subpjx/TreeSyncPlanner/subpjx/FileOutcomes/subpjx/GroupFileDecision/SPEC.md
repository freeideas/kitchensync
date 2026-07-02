# GroupFileDecision:

## Purpose

GroupFileDecision selects the group outcome for one already-visible file path
from classified peer states. It receives the per-peer classifications produced
outside this child and the role facts that identify canon, contributing,
subordinate, and active peers. It returns structured planner facts describing
whether the path outcome is an existing file, deletion, or no file, plus the
copy and displacement intents needed to make active peers match that outcome.

This child owns group file decision logic only. It does not classify raw peer
facts, inspect files, read snapshots, write snapshots, choose visible paths,
execute copies, delete files, move files to `BAK/`, set modification times, or
format process output.

## Responsibilities

GroupFileDecision exposes one decision operation for one file path. The
operation accepts the classified state of each active peer for that path and
the supplied peer role facts. The operation returns:

- the selected group outcome: existing file, deletion, or no file;
- the source peer or identical source peers when the outcome is an existing
  file;
- copy intents for active peers that need the winning file;
- deletion or displacement intents for active peers that have a file when the
  selected outcome requires the file to be absent;
- per-peer decision facts that state whether each peer voted, matched the
  winner, needed a copy, needed deletion, or needed displacement.

With a canon peer, the canon peer's classified state selects the outcome
without consulting any other peer's state. If the canon peer has a live file,
that file is the outcome for every other active peer. If the canon peer lacks
the file, deletion is the outcome for every other active peer that has a live
file. A non-canon peer must not change a canon file decision.

Without a canon peer, only contributing peers provide votes. Subordinate peers
do not vote, but active subordinate peers are still targets for the outcome
selected from contributing peers.

When all contributing peers that have a file are unchanged and matching, that
unchanged file is the group outcome. Contributing peers whose files already
match that outcome do not receive copy intents. An active peer that lacks the
file receives a copy intent for the unchanged file.

Modified live-file votes are compared by modification time. The newest
modification time selects the winning modified file. New live-file votes are
also compared by modification time. The newest modification time selects the
winning new file, and that new-file winner is propagated to active peers that
lack the file, including peers with no snapshot row for the file.

When comparing live-file votes, any peer modification time within 5 seconds of
the maximum modification time is tied with the maximum. A peer modification
time more than 5 seconds behind the maximum loses to the maximum. Among
live-file votes tied on modification time, the larger byte size selects the
winning file. Files with tied modification times and equal byte sizes are
treated as identical even when their bytes differ.

No copy intent is selected between peers whose files are treated as identical.
When a target peer needs a file that is identical on multiple source peers, the
copy intent may name any one of those identical source peers. A peer that
already has the winning byte size and a live file modification time within 5
seconds of the winning modification time is not selected for a copy.

When deleted votes and existing file votes both exist, the most recent deletion
estimate is compared with the winning existing file modification time. A
deletion estimate more than 5 seconds newer than the existing file
modification time selects deletion. An existing file whose modification time is
not more than 5 seconds older than the deletion estimate wins over deletion.
When an existing file and deletion are tied, the existing file selects the
outcome.

An absent-unconfirmed contributing peer contributes a deletion vote only when
its `last_seen` is present and more than 5 seconds newer than the maximum
modification time of contributing peers that have the file. That deletion vote
uses `last_seen` as its deletion estimate. An absent-unconfirmed peer whose
`last_seen` is NULL contributes no deletion vote. An absent-unconfirmed peer
whose `last_seen` is not more than 5 seconds newer than the maximum live-file
modification time also contributes no deletion vote. An absent-unconfirmed
peer that contributes no deletion vote is selected to receive the file when an
existing file wins.

If every contributing peer is absent with no snapshot row for the file, the
group outcome is no file. No copy intent is selected for that file. An active
subordinate peer that has a live file is selected for displacement to `BAK/`.

## Boundaries

GroupFileDecision is pure decision logic over supplied classifications and role
facts. It does not create classifications from live facts, fetch metadata, read
or write databases, list directories, normalize paths, inspect file contents,
compare file bytes, execute file operations, set timestamps, format stdout, or
decide process exit status.

The caller is responsible for supplying coherent classifications for exactly
one file path and for supplying role facts that identify active peers and their
decision roles. This child must return an explicit invalid-input error when the
supplied facts cannot describe one group decision for one file path, when a
required canon or contributing role fact is contradictory, or when a live vote
is missing the byte size or modification time needed for comparison. It must
not invent metadata, fetch additional state, or silently treat malformed facts
as votes.

GroupFileDecision must preserve these invariants:

- canon file decisions are final and cannot be changed by non-canon peers;
- subordinate peers never vote in non-canon file decisions;
- active subordinate peers can receive copy or displacement intents after a
  contributing outcome is selected;
- absent files with no snapshot row contribute no vote;
- absent-unconfirmed deletion votes use only the supplied `last_seen` value and
  the maximum live-file modification time;
- the 5-second tolerance is applied consistently to live vote ties,
  deleted-versus-existing comparison, absent-unconfirmed deletion votes, and
  winner match checks;
- among tied live-file votes, byte size breaks the tie;
- tied live-file votes with equal byte size are identical for planning;
- identical live files do not receive copy intents between each other;
- peers already matching the winning size and time tolerance do not receive a
  copy intent;
- deletion outcomes produce deletion or displacement intents only, never copy
  intents;
- no-file outcomes for all-no-row contributing inputs produce no copy intents
  and displace active subordinate live files;
- all returned outcomes, source facts, per-peer decision facts, and intents are
  about the single input file path and have no execution side effects.
