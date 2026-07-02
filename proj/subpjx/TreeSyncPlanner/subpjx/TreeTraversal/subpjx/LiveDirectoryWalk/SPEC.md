# LiveDirectoryWalk:

## Purpose

LiveDirectoryWalk owns the live recursive combined-tree walk for one
KitchenSync run. It lists reachable peers at each directory path, turns
successful live listings into deterministic entry facts, scopes directory
listing failures to the affected subtrees, and forms child-recursion intents
from the parent facade's entry decisions.

This child does not decide copy, delete, create, displacement, type-conflict,
or snapshot outcomes. It exposes live traversal facts that the TreeTraversal
facade can filter for excluded paths and pass to sibling outcome planners.

## Responsibilities

LiveDirectoryWalk exposes an operation to list one directory path for the peers
that are active in that subtree. The input includes the directory path, each
peer's run role, the peers still eligible in the subtree, the configured
`--retries-list` count, and a caller-supplied listing operation for each peer.
For the first try at a directory, it starts the listing operation for every
reachable active peer before awaiting any listing result. A failed listing is
retried for the same peer and path until the peer either succeeds or reaches the
configured total try count.

For each peer that still cannot list the directory after all allowed tries,
LiveDirectoryWalk returns an error-level diagnostic fact with the peer and path.
It also returns a run-local failed-subtree fact that makes that peer ineligible
for decisions, file or directory mutation, and snapshot row updates at that
directory and every descendant path. Listing failures are traversal errors only;
this child never reports them as file-copy queue items.

When a failed peer is not the canon peer and at least one contributing peer
remains active at the directory, LiveDirectoryWalk continues the directory with
the remaining active peers. A failed peer is removed from the live entry set for
that directory and from every descendant walk during the same run. The failure
does not persist across runs; a later run starts from its supplied active peers
and may include the peer normally when listing succeeds.

When the canon peer still cannot list the directory after all allowed tries,
LiveDirectoryWalk returns a subtree-skip fact for that directory and all
descendant paths for every peer. The skip fact must make every covered peer
ineligible for file mutation, directory mutation, and snapshot row updates under
that subtree. The child returns no entry facts or child-recursion intents below
that skipped path.

When every contributing peer still cannot list a directory after all allowed
tries, LiveDirectoryWalk returns a subtree-skip fact for that directory and all
descendant paths. This skip also blocks subordinate cleanup under the subtree,
including displacement of subordinate peer files. The child returns no entry
facts or child-recursion intents below that skipped path.

After successful listings are available for the peers that remain active,
LiveDirectoryWalk forms the directory's entry names only from live listing
results. Snapshot-only paths do not add entry names. The entry set includes
names from every active contributing peer and every active subordinate peer.
For each entry name, the child returns a live entry fact containing the
directory path, the entry name, the peers whose live listing reported that
entry, and the peer eligibility produced by listing success or failure at this
point in the walk.

LiveDirectoryWalk orders entry facts within one directory by case-insensitive
lexicographic order, using the original case-sensitive name as the tie-breaker.
The ordering is stable and independent of listing completion order.

LiveDirectoryWalk exposes a second operation that accepts the fully processed
entry facts for a directory together with the parent facade's decision for each
child-directory entry. That operation returns child-recursion intents only after
all entries in the directory have been processed by the parent. A recursion
intent includes only peers that keep or create that child directory according
to the parent decision. If the parent decision displaces the child directory on
a peer, that peer is omitted from the recursion intent. If no peer remains for
the child directory, no recursion intent is returned.

## Boundaries

LiveDirectoryWalk does not parse command-line options, classify peer roles,
connect to peers, authenticate transports, read snapshot rows, write snapshot
rows, apply accepted or built-in excludes, choose sync outcomes, execute file
or directory changes, enqueue copies, or print logs. It receives structured
inputs and returns structured entry facts, listing-failure facts, subtree-skip
facts, diagnostics, and recursion intents.

Transport and filesystem code own the actual listing calls. The TreeTraversal
facade applies exclusion policy to this child's live facts before returning
entries, skips, and recursion intents outside the TreeTraversal subtree.
Sibling outcome planners decide whether each listed entry is copied, created,
deleted, displaced, or left unchanged. Snapshot code owns lookup and update.

LiveDirectoryWalk must preserve these invariants for every run:

- traversal is one recursive combined tree over the active peers supplied for
  each subtree;
- the first listing attempt for every reachable active peer at one directory is
  started before any listing result for that directory is awaited;
- listing retry counts are tracked per peer and per directory path;
- entry names come from live peer listings, not snapshot-only paths;
- active contributing peers and active subordinate peers both contribute live
  entry names;
- entry ordering within one directory is case-insensitive lexicographic order
  with the original case-sensitive name as the tie-breaker;
- listing failures are never placed in the file-copy queue;
- failed listing facts block decisions, mutations, and snapshot updates for the
  failed peer under the failed subtree during only the current run;
- canon listing failure skips the subtree for every peer;
- all-contributing listing failure skips the subtree for every peer and blocks
  subordinate displacement under that subtree;
- every entry in a directory is processed by the parent before any child
  recursion intent is emitted;
- recursion into a child directory includes only peers that keep or create that
  child directory and excludes peers where that directory was displaced.
