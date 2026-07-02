# TypeConflictOutcomes:

## Purpose

TypeConflictOutcomes resolves one visible path where active peers report a
mixture of live files and live directories. It receives the already-filtered
peer-role facts, the optional canon peer, and each active peer's live type at
that path from the TreeSyncPlanner facade. It returns structured planner facts
and intents saying whether the path should become a file, become a directory,
or be displaced everywhere that still has it.

This child owns only file-versus-directory decisions. It does not list
directories, apply excludes, classify file versions, collect directory survival
evidence, recurse into children, execute copies, create directories, move
entries to `BAK/`, or write snapshot rows.

## Responsibilities

TypeConflictOutcomes exposes a type-conflict decision operation for one path.
The operation accepts:

- the active peers for the path, including which peers are contributing,
  subordinate, and active targets;
- an optional canon peer identity;
- each active peer's live type at the path: file, directory, or missing;
- the exact source filename case reported by each peer that can be selected as
  the source of a winning file or directory.

With a canon peer, the canon peer's live type wins unconditionally. If the
canon peer has a file at the path, the returned result is a file outcome.
Active peers that have a directory at that path receive directory displacement
intents, and active peers that lack the winning file receive file sync intents
from the canon file. Non-canon peers must not change this decision.

If the canon peer has a directory at the path, the returned result is a
directory outcome. Active peers that have a file at that path receive file
displacement intents, and active peers that lack the winning directory receive
directory creation or sync intents from the canon directory. Non-canon peers
must not change this decision.

If the canon peer is missing the path, the returned result is absence for that
path. Every active peer that has either a file or a directory at the path
receives a displacement intent for that live entry. Active peers that already
lack the path receive no mutation intent. The result is not eligible for child
recursion.

Without a canon peer, contributing files beat contributing directories at the
same path. When one or more contributing peers have a file, the returned result
is a file outcome even if one or more contributing peers also have a directory.
The winning file source must be selected only from contributing peers that have
a live file at that path. Subordinate files are never eligible to make the file
type win over a contributing directory, and they are never eligible as the
winning source file.

Without a canon peer and without any contributing file at the path, a
contributing directory remains a directory outcome. Subordinate peers do not
vote on this decision, but active subordinate peers are targets after the
contributing type is selected.

After the winning type is selected, every active peer that has the losing type
at the path receives a displacement intent for that entry. A peer that lacks the
winning type then receives the replacement intent needed for the selected
outcome: file sync when the winner is a file, or directory creation or sync when
the winner is a directory. A subordinate peer with the losing type is displaced
and replaced in the same way as any other active target.

Every returned sync source fact must preserve the exact filename case reported
by the selected source filesystem. The child must not normalize, lowercase, or
otherwise rewrite the source name when returning file or directory sync facts.

## Boundaries

TypeConflictOutcomes does not parse command-line text, classify peer roles,
normalize paths or URLs, apply excludes, order sibling entries, start or retry
directory listings, inspect file metadata beyond the supplied live type facts,
compare file modification times, collect directory survival evidence, execute
file copies, create directories, delete files, move entries to `BAK/`, format
stdout, or write SQLite snapshot rows.

The child returns structured planner facts and intents only. TreeTraversal owns
path visibility, listing failure handling, entry ordering, and recursion
scheduling. FileOutcomes owns same-type file selection. DirectoryOutcomes owns
same-type directory existence, deletion, creation, and survival decisions. The
parent TreeSyncPlanner facade is responsible for routing only mixed file and
directory paths to this child and for placing the returned facts into the
pre-order action plan.

TypeConflictOutcomes has no transport, storage, stdout, or process-exit error
obligations. Its error obligation is decision safety: it must decide only from
the supplied active peer facts, must not invent a source when no eligible source
exists, and must return a structured invalid-input fact instead of creating copy
or displacement intents when the supplied facts cannot describe one mixed-type
decision for one path.

TypeConflictOutcomes must preserve these invariants:

- canon type decisions are final and cannot be changed by non-canon peers;
- a missing canon path selects absence and displaces every active live entry at
  that path;
- without a canon peer, only contributing peers choose whether a file or
  directory type wins;
- without a canon peer, any contributing file beats a contributing directory at
  the same path;
- the winning file source in a non-canon conflict is selected only from
  contributing file entries;
- subordinate files never make a file beat a contributing directory;
- subordinate peers can receive displacement and replacement intents after the
  contributing or canon outcome is selected;
- losing live types are displaced before replacement is applied by the parent
  plan;
- all returned intents are about one path only and contain no execution side
  effects;
- selected source names keep the exact source filesystem case.
