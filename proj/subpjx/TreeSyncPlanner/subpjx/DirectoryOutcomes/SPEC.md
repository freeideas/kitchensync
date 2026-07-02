# DirectoryOutcomes:

## Purpose

DirectoryOutcomes selects the directory result for one non-excluded path in a
TreeSyncPlanner run. It receives active peer-role facts, per-peer live directory
presence, per-peer snapshot facts for that directory, the optional canon peer,
and any survival-evidence result gathered for a live directory conflict. It
returns structured planner facts and intents that say whether the directory
exists, is absent, is displaced, is created, or is safe to recurse into.

This child is a decision unit only. It does not list directories, mutate files,
move entries to `BAK/`, write snapshot rows, or decide child file outcomes. The
parent facade wires this child with traversal, file decisions, type-conflict
decisions, snapshot lookup, and mutation execution.

## Responsibilities

DirectoryOutcomes exposes a directory decision operation for a single directory
path. The operation accepts:

- the active peers for the path, including which peers are contributing,
  subordinate, and active targets;
- an optional canon peer identity;
- whether each active peer has the path as a live directory;
- whether each contributing peer has a snapshot row for the directory, including
  `deleted_time` and `last_seen` when present;
- the survival-evidence state for a live directory conflict: newest live file
  modification time under the directory, no live file evidence, or evidence
  collection failed after all allowed listing tries.

Directory modification times must not be read or used for the existence
decision. A live directory on a contributing peer votes for existence even when
that peer's snapshot row differs. A contributing peer with no live directory and
no snapshot row does not vote on that directory's existence. Subordinate peers
never vote, but they remain possible targets for creation or displacement after
the contributing result is selected.

With a canon peer, the canon peer's directory state wins unconditionally. If the
canon peer has a live directory, the returned result says the directory exists
on every active peer. Active peers that lack it receive a directory creation
intent, and the directory is eligible for recursion with active peers that keep
or create it. If the canon peer is missing the path, the returned result says
the path is absent on every active peer. Active peers that have the directory
receive a whole-directory displacement intent, peers that lack it receive no
creation intent, and the directory is not eligible for recursion.

Without a canon peer, the operation first identifies voting contributing peers.
A contributing peer votes when it has the live directory or has a snapshot row
for the directory. If every voting contributing peer has the live directory, the
directory exists on every active peer. Active peers that lack it receive a
directory creation intent, and recursion is eligible for active peers that keep
or create the directory.

When at least one contributing peer has the directory live and at least one
voting contributing peer is absent, the operation treats the path as a
live-directory deletion conflict. Each absent voting peer contributes a deletion
estimate from its `deleted_time` when present, otherwise from its `last_seen`.
If more than one deletion estimate is present, the newest estimate is used for
the conflict.

Survival evidence for a live-directory deletion conflict is the newest
modification time of live files anywhere under the live directory among peers
that have it live. Directories under that live directory do not provide survival
evidence. A live directory subtree containing no files provides no survival
evidence.

If survival-evidence collection fails after all allowed listing tries, the
returned result is a subtree block for this directory. The block must tell the
parent facade that no active peer may receive file mutation intents, directory
mutation intents, copy intents, displacement intents, creation intents,
recursion work, or snapshot update intents anywhere under that directory subtree
during the current run.

If the live-directory conflict has no survival evidence, directory deletion
wins. If survival evidence exists and the newest deletion estimate exceeds that
evidence by more than the five-second tolerance, directory deletion wins. When
directory deletion wins, every active peer that has the directory receives one
whole-directory displacement intent. Active peers that lack it receive no
creation intent, and the directory is not eligible for recursion.

If survival evidence exists and the newest deletion estimate does not exceed
that evidence by more than the five-second tolerance, the directory survives.
The returned result says the directory exists on every active peer, active peers
that lack it receive a directory creation intent, and the directory is eligible
for recursion with active peers that keep or create it. This survival result
must not suppress child file decisions: newer child files remain eligible to
propagate by the file rules, and older child files remain eligible for removal
by the file deletion rules during recursion.

If no contributing peer has the directory live, at least one contributing peer
has a snapshot row for it, and every contributing peer with a row is absent, the
operation selects whole-directory displacement for every active peer that has
the directory. Active peers that lack it receive no creation intent, and the
directory is not eligible for recursion.

If no contributing peer has the directory live or in a snapshot row, the group
result is absence without a contributing deletion history. Subordinate peers
that have the directory receive a whole-directory displacement intent. The
directory is not created on any peer and is not eligible for recursion from this
result.

Every directory displacement intent returned by this child is whole-directory
and pre-order. The intent must tell the parent facade to move the directory as
one directory before any of its children can be independently visited. A
directory selected for displacement is not recursed into on the displaced peer.

## Boundaries

DirectoryOutcomes does not parse command-line text, classify peer roles,
normalize paths or URLs, apply excludes, order sibling entries, start or retry
directory listings, collect live subtree listings, inspect file contents,
classify files, resolve file-versus-directory conflicts, create directories,
copy files, delete files, move entries to `BAK/`, format progress output, or
write SQLite snapshot rows.

The child receives structured facts from the parent facade and returns
structured directory outcome facts only. TreeTraversal owns path visibility,
listing retries, listing-failure exclusions, entry ordering, and final recursion
scheduling. FileOutcomes owns child file propagation and file deletion choices.
TypeConflictOutcomes owns mixed file and directory paths. Storage and mutation
children own snapshot updates and filesystem changes.

DirectoryOutcomes must preserve these invariants for every returned result:

- directory modification times do not affect directory existence;
- canon directory state wins over all other directory votes;
- only contributing peers vote on non-canon directory existence;
- contributing peers with neither a live directory nor a snapshot row do not
  vote for that directory;
- subordinate peers do not vote but can receive selected creation or
  displacement intents;
- live directories provide existence votes even when snapshot facts differ;
- deletion estimates use `deleted_time` before `last_seen`;
- live-directory conflicts use the newest deletion estimate;
- survival evidence comes only from live files under the live directory;
- empty live directory subtrees provide no survival evidence;
- failed survival-evidence collection blocks all peer mutation and snapshot
  update intents under the directory subtree for the current run;
- directory deletion winners are not recreated and are not recursed into;
- surviving directories are eligible for recursion;
- child file outcomes remain delegated to file rules when a directory survives;
- directory displacement is whole-directory and pre-order.
