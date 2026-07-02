# TreeTraversal:

## Purpose

TreeTraversal controls the recursive combined-tree walk for one KitchenSync run.
It receives the active peer-role facts, accepted excludes, the configured
directory-list retry count, and listing access supplied by the parent planner.
It returns structured traversal facts, diagnostics, and child-recursion intents
that the TreeSyncPlanner facade can pass to the file, directory, and
type-conflict outcome planners.

This child decides which directory paths are visible, which peers remain active
inside each subtree, which live entry names are considered at each directory,
and which child directories are eligible for later recursion. It does not choose
file winners, directory winners, type-conflict winners, copy sources, BAK moves,
or snapshot row contents.

## Responsibilities

TreeTraversal exposes a traversal operation for the sync root and for child
directories selected by the parent facade. For each traversed directory path,
the operation starts a directory-listing request for every peer still active in
that subtree before awaiting any listing result. Listing is attempted for the
same peer and path up to `--retries-list` total tries. A listing failure after
all tries is returned only as a structured failure fact and an error-level
diagnostic identifying the peer and path; it is never returned as file-copy
work.

The traversal operation keeps listing-failure exclusions run-local. If a peer
cannot list a directory after all allowed tries, that peer is removed from
decision eligibility for that directory and every descendant path for the
current run. The returned facts must make that peer ineligible for file
mutation intents, directory mutation intents, copy intents, and snapshot update
intents under the failed subtree. The same peer must be eligible in a later run
when that later run can list the path successfully.

When the failed peer is not canon and at least one contributing peer remains
active for the directory, TreeTraversal continues the current directory using
the remaining active peers. When the canon peer fails listing a directory after
all tries, TreeTraversal returns a subtree-skip intent for that directory and
all descendants for every peer. When every contributing peer fails listing a
directory after all tries, TreeTraversal also returns a subtree-skip intent for
that directory and all descendants, including subordinate cleanup. A skipped
subtree must not produce peer mutation or snapshot update eligibility for any
peer covered by the skip.

After listing succeeds for the peers that remain active at a directory,
TreeTraversal forms that directory's entry set only from live listing results.
Snapshot-only paths do not add names to traversal. The entry set includes live
entry names from every active contributing peer and every active subordinate
peer, so subordinate-only entries can be considered by sibling outcome planners.
Within one directory, TreeTraversal orders entry names by case-insensitive
lexicographic comparison and uses the original case-sensitive name as the
tie-breaker.

TreeTraversal applies excludes before any sync decision, child scan, child
recursion, or snapshot lookup eligibility is returned for a path. Every accepted
command-line exclude hides its matching path for the run. If the accepted
exclude matches a file, only that file path is hidden. If it matches a
directory, that directory and all descendants are hidden. Built-in excludes
always hide `.kitchensync/` directories, `.git/` directories, symbolic link
files, symbolic link directories, and special files. Built-in excludes cannot
be overridden by command-line excludes.

For an excluded path, TreeTraversal returns no decision item, snapshot lookup
eligibility, snapshot update eligibility, copy eligibility, deletion
eligibility, displacement eligibility, scan request, or recursion intent. An
excluded directory is not scanned and is not recursed into. Existing excluded
entries are left outside the plan even when another peer's live listing would
otherwise create, delete, copy, or displace that path.

For each non-excluded entry in the sorted directory entry set, TreeTraversal
returns an entry-processing fact that contains the path, the exact live names
reported by active peers, the peers whose live listing contributed that entry,
and the peer eligibility facts needed by sibling outcome planners. The child
does not consult snapshot rows itself; it marks only which non-excluded paths
are eligible for snapshot lookup by another planner or by the parent facade.

TreeTraversal returns recursion work only after every entry in the current
directory has been processed by the parent facade. A returned child-recursion
intent must contain only peers that keep or create that child directory
according to the parent facade's directory or type-conflict decision. If a
directory is displaced on a peer, that peer is omitted from recursion for that
directory. If no peer remains eligible for a child directory, no recursion
intent is returned for it.

## Boundaries

TreeTraversal does not parse command-line text, validate exclude syntax,
classify startup peer roles, connect to peers, authenticate transports, inspect
snapshot databases, decide file or directory outcomes, gather directory
survival evidence, execute copies, create directories, delete files, move paths
to `BAK/`, write SQLite rows, or print progress lines. It receives structured
inputs and returns structured traversal facts, diagnostics, exclusions, skips,
and recursion intents.

The parent TreeSyncPlanner facade wires this child to peer-role facts and
sibling outcome planners. Transport or filesystem code performs the actual
listing attempts. Snapshot code owns lookup and update. Outcome planners decide
whether listed entries become copy, create, delete, displacement, or no-op
intents.

TreeTraversal must preserve these invariants for every run:

- traversal is one recursive combined tree over active peers;
- directory listing requests for all active peers in a subtree are started
  before any listing result for that directory is awaited;
- retry counts are per peer and per directory path;
- listing failures are never placed in the file-copy queue;
- a failed listing removes only run-local visibility for the failed subtree;
- canon listing failure skips the subtree for every peer;
- all-contributing listing failure skips the subtree for every peer and does
  not displace subordinate files under that subtree;
- live listings, not snapshot-only paths, determine entry names to process;
- active contributing and subordinate peers both contribute live entry names;
- entries within one directory have deterministic case-insensitive ordering
  with original case as the tie-breaker;
- excluded paths never cause scanning, recursion, snapshot lookup, snapshot
  update, copying, deletion, displacement, or sync decisions;
- built-in excludes remain active regardless of command-line excludes;
- all entries in a directory are handled before any child recursion begins;
- recursion into a child directory includes only peers that keep or create that
  child directory.
