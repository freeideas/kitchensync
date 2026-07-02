# TreeSyncPlanner:

## Purpose

TreeSyncPlanner owns the combined-tree sync plan. It receives the reachable
peers for one run, their run roles, accepted excludes, live directory entries,
and per-peer snapshot facts, then selects the file, directory, and type-conflict
outcomes that the rest of KitchenSync must apply.

This child is a planner and traversal controller. It decides which paths are
visible for the run, which peers may vote, which active peers are targets, which
files should be copied, which paths should be displaced, which directories
should be created, and which child directories may be visited. It does not own
the transport, copy execution, BAK rename mechanics, snapshot database storage,
or stdout formatting.

## Responsibilities

TreeSyncPlanner exposes a startup-role operation that accepts the reachable peer
set, each peer's command-line role, whether each peer had snapshot data on disk
at startup, and whether a canon peer was designated. It returns the run role for
each reachable peer:

- a reachable canon peer is contributing even when its snapshot database did
  not exist at startup;
- a reachable non-canon peer with no startup snapshot database is subordinate
  for that run;
- a reachable peer marked with `-` is subordinate for that run even with
  snapshot history;
- a reachable peer with snapshot history and no `-` marker is contributing.

The same operation returns the specified fatal startup outcomes as structured
facts for the caller to print or exit on. If fewer than two peers are reachable,
the outcome is a startup failure with exit code `1`. If a designated canon peer
is not reachable, the outcome is an unreachable-canon failure with exit code
`1`. If no reachable peer has snapshot data and no canon peer is designated,
the outcome is the first-sync failure with exit code `1` and the required
stdout line `First sync? Mark the authoritative peer with a leading +`. If
automatic subordination leaves no reachable contributing peer, the outcome is
the no-contributing-peer failure with an error exit and the required stdout
line `No contributing peer reachable - cannot make sync decisions`. A run with
at least one reachable contributing peer with snapshot history does not require
a canon peer. Peers unreachable at startup are not present in the planner's
peer set and receive no listings, decisions, or snapshot update intents for
that run. Their existing snapshot rows remain untouched; on a later run when
the peer is reachable again, differences between its live state and those rows
are ordinary inputs to the planner's decisions.

TreeSyncPlanner exposes a recursive planning operation for the sync root. For
each traversed directory path, it starts a directory-listing request for every
peer still active in that subtree before awaiting any listing result. A listing
request is retried for that same peer and path up to `--retries-list` total
tries. Listing failures are never returned as file-copy work.

When a peer still cannot list a directory after all allowed tries, the planner
returns an error-level diagnostic fact identifying that peer and path. The peer
is excluded from decisions for that directory and every descendant path in the
current run, and the plan contains no file operation, directory operation, copy
intent, or snapshot update intent under that failed subtree for that peer. This
exclusion is run-local; the same peer participates normally in a later run when
the listing succeeds. If the failed peer is not canon and at least one
contributing peer remains active, the planner continues processing the current
directory with the remaining active peers.

If the failed listing belongs to the canon peer, the planner skips all decisions
for that directory and its descendants for every peer. No peer receives file,
directory, or snapshot update intents under that subtree. If every contributing
peer fails listing a directory, the planner also skips all decisions for that
directory and descendants, including subordinate cleanup.

After successful listing at a directory level, the planner forms the entry set
from live listings only. Snapshot-only paths do not add names to the traversal.
The entry set includes live entry names from every active contributing peer and
from every active subordinate peer so subordinate-only extras can be displaced
when the group outcome says the path does not exist. Within one directory,
entries are processed in case-insensitive lexicographic order, using the
original case-sensitive name as the tie-breaker.

The planner applies excludes before snapshot lookup and before any decision for
the path. Every accepted command-line exclude hides its matching path for the
run. A command-line exclude that matches a file hides only that file. A
command-line exclude that matches a directory hides the directory and all
descendants. Built-in excludes always hide `.kitchensync/` directories, `.git/`
directories, symbolic link files, symbolic link directories, and special files,
regardless of command-line excludes. Excluded paths are not scanned, recursed
into, copied, deleted, displaced, used for sync decisions, used for snapshot
lookups, or used for snapshot update intents.

For each non-excluded file path, the planner classifies each contributing peer
from live state and that peer's snapshot row. A live file with a snapshot row
whose `deleted_time` is NULL, matching byte size, and modification time within
5 seconds is unchanged. A live file with a different byte size, a modification
time more than 5 seconds away from the snapshot row, or a non-NULL
`deleted_time` is modified. A live file with no row is new. An absent file with
a non-NULL `deleted_time` row is deleted using that value as its deletion
estimate. An absent file with a NULL `deleted_time` row is
absent-unconfirmed. An absent file with no row contributes no vote.

With a canon peer, the canon file state wins unconditionally. If the canon peer
has the file, that file is the outcome for every other active peer. If the
canon peer lacks the file, deletion is the outcome for every other active peer
that has the file. Other peers cannot change the canon file decision.

Without a canon peer, subordinate peers do not vote on file decisions, but they
are targets after the contributing outcome is selected. When all contributing
peers that have a file are unchanged and matching, that unchanged file is the
group outcome; no copy is selected between contributing peers that already
match, and active peers that lack the file receive it. Among modified votes, the
newest modification time wins. Among new votes, the newest modification time
wins and propagates to peers that lack the file, including peers with no row.

When deleted and existing file votes both exist, the planner compares the most
recent deletion estimate with the existing file modification time. A deletion
estimate more than 5 seconds newer selects deletion. An existing file whose
modification time is not more than 5 seconds older wins over deletion. An
absent-unconfirmed peer contributes a deletion vote using `last_seen` only when
`last_seen` is present and more than 5 seconds newer than the maximum live file
modification time. Otherwise it contributes no deletion vote and receives the
file when an existing file wins.

When comparing live file votes, any modification time within 5 seconds of the
maximum is tied with the maximum; a time more than 5 seconds behind loses. Among
tied live file votes, the larger byte size wins. If an existing file and a
deletion tie, the file wins. Files tied on modification time and byte size are
treated as identical even if their bytes differ; no copy is selected between
identical peers, and a peer needing that file may copy from any identical
source. A peer that already has the winning byte size and a modification time
within 5 seconds of the winning modification time is not selected for copy. If
every contributing peer is absent with no row, the file does not exist in the
group outcome, no copy is selected, and an active subordinate peer that has the
file is selected for displacement.

For directories, the planner ignores directory modification times for existence
decisions. With a canon peer, a live canon directory makes the directory exist
on every active peer, and a missing canon path makes the path absent on every
active peer. Without a canon peer, a directory that is live on every voting
contributing peer exists on every active peer. A contributing peer with a live
directory votes for existence even if its snapshot row differs. A contributing
peer with no live directory and no snapshot row does not vote.

For a live-directory deletion conflict, the planner uses the absent peer's
`deleted_time` when present, otherwise that peer's `last_seen`, as the deletion
estimate. When multiple peers vote deletion, the most recent estimate is used.
Survival evidence is the newest modification time of live files anywhere under
the live directory across peers that have it live. Directories under the live
directory do not provide survival evidence, and a subtree containing no files
provides no survival evidence. If collecting survival evidence fails after all
allowed listing tries, the planner skips decisions for that directory subtree
for every peer and returns no peer mutation or snapshot update intents under
that subtree.

When the newest directory deletion estimate exceeds survival evidence by more
than the 5-second tolerance, or when no survival evidence exists, directory
deletion wins. The planner selects displacement for every active peer that has
the directory, does not recreate it on peers that lack it, and does not recurse
into it. When the newest deletion estimate does not exceed survival evidence by
more than the tolerance, the directory survives on every active peer and the
planner recurses into it. Child files in a surviving directory are still decided
by the file rules, so newer child files can propagate and older child files can
be removed during recursion.

If no contributing peer has the directory live, at least one contributing peer
has a snapshot row for it, and every contributing peer with a row is absent, the
directory is selected for displacement on every active peer that has it. If no
contributing peer has the directory live or in a snapshot row, the directory
does not exist in the group outcome and subordinate peers that have it are
selected for displacement.

For a file-versus-directory conflict at one path, a canon peer's state wins. A
canon file displaces directories and syncs the file to active peers. A canon
directory displaces files and syncs the directory to active peers. A missing
canon path displaces that path on active peers that have it. Without a canon
peer, a contributing file wins over a contributing directory, and the winning
file is selected from contributing file entries only. A subordinate file does
not make a file beat a contributing directory. After the decision, subordinate
paths with the losing type are displaced and replaced as needed.

The planner returns a pre-order action plan. It finishes every entry in a
directory before returning any child-directory recursion work. A directory
selected for displacement is planned as one directory move before any of its
children can be independently visited. The planner never recurses into a
directory on a peer after that directory is displaced on that peer, and it
recurses into a child directory only with peers that keep or create that child
directory. Synced filenames preserve the exact case reported by the source
filesystem.

## Boundaries

TreeSyncPlanner does not parse command-line text, validate exclude syntax,
normalize peer URLs, connect to peers, choose fallback URLs, authenticate SFTP,
download or upload snapshots, perform SWAP recovery, format stdout, enforce copy
slot concurrency, execute file copies, create directories, rename entries to
BAK, set file modification times, or write SQLite rows. It receives structured
facts and returns structured decisions, diagnostics, and action intents.

Transport and filesystem children own the actual listing calls and mutations.
Snapshot children own snapshot lookup, row upsert, tombstone, and cascade
mechanics. Copy children own queued transfer execution and copy retry behavior.
Output children own the exact text printed to stdout.

The planner's error obligations are limited to decision safety. A failed
listing after retry removes visibility for the failed subtree as specified. A
canon listing failure, all-contributing listing failure, or survival-evidence
listing failure makes the affected subtree produce no peer mutation or snapshot
update intents. Excluded paths and failed subtrees are invisible to decisions
and remain untouched by the plan.

The planner must preserve these invariants for every returned plan:

- decisions use live listing names, not snapshot-only names, to drive traversal;
- subordinate peers are listed and targeted but never contribute votes;
- unreachable startup peers are absent from all planner work for the run;
- excluded paths never cause snapshot lookup, snapshot update, copy,
  displacement, deletion, creation, or recursion;
- listing failures are never placed in the file-copy queue;
- path ordering is deterministic within each directory;
- directory displacement is whole-subtree and pre-order;
- directory recursion includes only peers that keep or create the directory;
- the 5-second tolerance is applied consistently to file classification, file
  vote comparison, deletion-versus-file comparison, and absent-unconfirmed
  deletion votes;
- selected source names keep the exact source filesystem case.
