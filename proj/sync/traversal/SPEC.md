# traversal:

## Purpose

Own the recursive pre-order combined-tree walk for one prepared sync run. This module determines which directories and entry names are visited, which peers remain active for each subtree, when directory listing and traversal-scoped maintenance are requested, and when subtree failures prevent downstream decisions.

Traversal is a private child of `sync`. It supplies ordered live candidate paths and scoped peer visibility to the rest of `sync`; it does not choose reconciliation outcomes, mutate snapshot rows by rule, execute concrete file operations itself, or render user-facing output.

## Responsibilities

- Start traversal at the sync root using the reachable `SyncPeer` set supplied by `sync::run`; unreachable peers are already removed before this module is entered.
- Walk directories recursively in pre-order: process every eligible entry in a directory, including inline work requested by downstream sync logic, before recursing into any child directory.
- Report a scanned-directory progress event for each directory that traversal begins to process. The root directory is reported as `.`; other directories are reported as slash-separated relative paths.
- In normal runs, request user-entry SWAP recovery for every peer active at the current directory before that peer's live entries are listed for decisions. In dry-run mode, skip peer-side SWAP recovery during traversal.
- Start directory listing for every peer active at the current directory before awaiting any listing result for that directory level.
- Retry a failed directory listing for the same peer and directory up to `RunConfig.retries_list` total listing attempts, including the first attempt.
- Treat a failed pre-listing user-entry SWAP recovery as a listing failure for that peer at the current directory, using the same subtree exclusion rules as exhausted listing retries.
- Keep listing failures scoped to the current directory subtree only. A non-canon peer that fails listing or pre-listing SWAP recovery is removed from the active peer set for that subtree and may participate again in unrelated subtrees or later runs.
- If the canon peer fails listing or pre-listing SWAP recovery for a directory, skip the entire directory subtree for all peers. No candidate entries, decisions, operations, copy submissions, cleanup-driven snapshot changes, recursion, or snapshot row updates may occur under that subtree.
- If every active contributing peer is removed at a directory, skip the entire directory subtree. Subordinate-only entries in that subtree must not be processed or displaced.
- Build each directory's candidate entry set only from live listing results. Snapshot rows must never add names to traversal.
- Include listed names from active contributing peers in the candidate set. Include listed names from active subordinate peers only while at least one active contributing peer remains at that directory.
- Apply traversal excludes before snapshot lookup, classification, decision-making, operation dispatch, copy submission, recursion, and snapshot updates. Excluded paths are treated as nonexistent for the run and existing peer contents at those paths are left untouched.
- Enforce built-in excludes for `.kitchensync/` and `.git/` directories. Symlinks and special files are expected to be omitted by the transport listing contract and must not be reintroduced by traversal.
- Enforce command-line excludes as exact relative file matches and directory-subtree prefixes. An excluded directory prevents recursion into that directory and all descendants.
- Process candidate names in deterministic case-insensitive lexicographic order, using the original case-sensitive name as the tie-breaker. Preserve transport-reported filename spelling when constructing child `RelPath` values and downstream requests.
- For each non-excluded candidate path, pass the active peer set, per-peer listed metadata or absence, and relative path to the downstream sync decision/dispatch flow. Traversal may use the returned directory-recursion peer set, but it must not derive the group outcome itself.
- Recurse into a child directory only for peers that downstream sync logic reports as keeping or successfully creating that directory. Peers whose directory was displaced, failed creation, failed listing in the parent subtree, or lacks the selected directory outcome are not active for that child traversal.
- In normal runs, after all eligible entries in a directory have been processed and before returning from that directory, request BAK/TMP retention cleanup for each peer still active at that directory. In dry-run mode, skip peer-side BAK/TMP cleanup.
- Treat `.kitchensync/` metadata checks for BAK/TMP cleanup as traversal maintenance, not as sync candidate traversal. Cleanup must not cause `.kitchensync/` to be visited as user data.
- Maintain traversal accounting for scanned directories and skipped subtrees so `sync::run` can populate `TraversalReport` and `SyncReport`.

## Boundaries

- This module receives already parsed configuration, validated relative excludes, connected peer sessions, and prepared local snapshot stores from `sync::run`; it does not parse CLI arguments, connect peers, download snapshots, assign peer roles, or decide startup failures.
- This module depends on the transport handle only through the parent-level listing and metadata contracts. It does not implement `file://` or `sftp://` behavior, normalize transport errors beyond the shared categories, or inspect transport-specific failures.
- This module owns directory-list retry and subtree visibility consequences. It does not own file-copy retry behavior, copy-slot limits, queue ordering, worker execution, or transfer phases.
- This module requests SWAP recovery and BAK/TMP cleanup at the required traversal points, but `operations` owns the concrete recovery, displacement, cleanup, staging, BAK, TMP, and dry-run mutation-suppression mechanics.
- This module does not classify entries, compare modification times, apply the five-second tolerance, choose file or directory winners, decide deletion versus existence, or resolve type conflicts. Those decisions belong to the decision-oriented private sync flow.
- This module does not mutate snapshot rows directly as a consequence of listing, absence, intended copy, successful copy, directory creation, displacement, or stale-row cleanup. It only prevents downstream snapshot work for excluded paths, failed-peer subtrees, canon-failed subtrees, and no-contributing-peer subtrees.
- This module does not enqueue copy work directly except through the downstream dispatch contract selected by `sync`. It must not place directory creation, deletion, or displacement into the file-copy queue.
- This module does not format diagnostics or progress output. It emits structured progress, listing failure, recovery failure, and skipped-subtree facts through the sinks or report-building contract supplied by `sync`.
- This module must not expose traversal cursors, stack representation, task-spawning strategy, listing aggregation maps, candidate-set internals, or child-module helper types outside the private `sync` implementation.

## Error Obligations

- Listing and pre-listing recovery failures are ordinary sync failures. They are reported through structured diagnostics and `SyncFailure`/`SkippedSubtree` data; they must not panic the process.
- A non-canon peer failure at a directory must leave that peer's files and snapshot rows unmodified for the failed directory subtree during the current run.
- A canon peer failure at a directory must leave every peer's files and snapshot rows unmodified for that directory subtree during the current run.
- A no-contributing-peer directory must produce a skipped-subtree record and must not process subordinate-only entries under that subtree.
- Excluded paths must produce no traversal decisions, operations, copy submissions, recursion, or snapshot row updates, even if one or more peers list those paths.
- Cleanup requested after processing a directory must not compensate for or bypass a skipped subtree. If traversal skipped a directory because the canon failed or no contributing peer remained, traversal must not run candidate processing or cleanup under that skipped subtree.
