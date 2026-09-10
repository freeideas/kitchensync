# Multi-Tree Synchronization

## Overview

Synchronizes N file trees in a single recursive combined-tree walk. At each directory level: list all peers in parallel, union their entries, decide the authoritative state for each, act, and recurse. The traversal is pre-order: every entry in a directory is decided and acted on before recursing into any subdirectory. Entry traversal order within a directory is deterministic, case-insensitive lexicographic order with the original case-sensitive name as a tie-breaker. This means a directory marked for displacement is renamed (with its entire subtree) before its children are ever visited - there is no separate "file deletion" pass. Each directory's manifest (see manifest.md) is read per peer, alongside that peer's listing of the directory, and is consulted for reconciliation (detecting deletions and modifications). It does not contribute entries to the union - only live peer listings drive traversal.

Subordinate peers (`-` prefix) are listed and receive outcomes, but their entries do not influence decisions. See "Subordinate Peers" below.

## Algorithm

```
function sync_directory(peers, path):
    // Phase 0: Repair incomplete swaps and manifest replacements before
    // interpreting live state
    if not dry_run:
        parallel_for_each(peers):
            recover_swaps(peer, path)
            repair_manifest(peer, path)   // see manifest.md, "Writing"

    // Phase 1: List all peers in parallel, reading each peer's manifest for
    // this directory together with its listing
    listings = parallel_for_each(peers):
        list_directory(peer, path)  // returns entries or error
        read_manifest(peer, path)   // missing manifest = no history here

    // Phase 1b: Drop peers with listing errors
    failed = [p for p in peers if listings[p] is error]
    for p in failed:
        log(error, "listing failed for {p} at {path}, excluding from this subtree")
    if canon peer is in failed:
        return  // no authoritative state; no decisions or subordinate file displacement
    active_peers = peers - failed

    // If all contributing peers failed listing, skip this directory entirely
    contributing = [p for p in active_peers if not p.is_subordinate]
    if not contributing:
        return  // no decisions, no subordinate file displacement

    // Phase 2: Union entry names across contributing peers only
    all_names = union(contributing.listings.keys())
    // Also include names from subordinate peers (for cleanup), but they don't add to decisions
    all_names = all_names | union(subordinate.listings.keys() for subordinate in active_peers if subordinate.is_subordinate)

    // Phase 2a: Apply built-in and command-line excludes
    all_names = all_names - matched_by_built_in_excludes - matched_by_command_line_excludes

    // Phase 3: Decide and act on each entry
    ordered_names = sort_case_insensitive_then_case_sensitive(all_names)
    for name in ordered_names:
        states = gather_states(contributing, listings, name)  // subordinate peers excluded
        history = manifest_line_per_peer(name)  // from this directory's manifests
        decision = decide(states, history)

        // Apply decision to ALL active peers (including subordinate)
        if decision.type == directory:
            recursion_peers = []
            for peer in active_peers:
                if directory should exist on peer:
                    if peer has wrong type at path/name:
                        if displace(peer, path/name) succeeds:
                            record_absent(peer, name)
                        else:
                            continue
                    if peer lacks directory at path/name:
                        if create_dir(peer, path/name) succeeds:
                            record_present(peer, name)
                            recursion_peers.append(peer)
                    else:
                        record_present(peer, name)
                        recursion_peers.append(peer)
                else if peer has any entry at path/name:
                    if displace(peer, path/name) succeeds:
                        record_absent(peer, name)
            if recursion_peers:
                sync_directory(recursion_peers, path/name)

        if decision.type == file:
            record_present(peers whose listing showed the winning file, name)
            for peer that has directory at path/name:
                if displace(peer, path/name) succeeds:  // type conflict
                    record_absent(peer, name)
            for each dst_peer that needs the file:
                record_pending_copy(dst_peer, name, decision)  // last_seen "-"
                enqueue_copy(decision.src_peer, path/name, dst_peer, path/name)
            for each peer where file should be deleted:
                if displace(peer, path/name) succeeds:
                    record_absent(peer, name)

    // Phase 4: Write this directory's manifest on each active peer, once every
    // entry here has been decided and every copy into this directory on that
    // peer has finished or failed. Recursion into subdirectories does not have
    // to finish first; peers dropped for a listing error are not written.
    if not dry_run:
        for peer in active_peers:
            when outstanding_copies(peer, path) == 0:
                write_manifest(peer, path)
```

**All displacement is inline.** Every displacement (type conflicts, deletions) executes during the combined-tree walk, not in the operation queue. Displacement is a same-filesystem rename to BAK/. Running it inline eliminates ordering dependencies between displacement and file copies (e.g., a type-conflict directory must be gone before a file copy can rename into that path).

**Directory deletion:** Do not recurse into a directory that is being displaced on a peer. The displacement moves the entire subtree in a single rename, and the subtree's manifests travel with it, so nothing has to be cascaded. Only peers that are keeping the directory participate in recursion. Because the traversal is pre-order (decide every entry before recursing), a displaced directory is always renamed as a whole before any of its children are visited - never split files and directories into separate passes.

**Listing errors:** If `list_directory` fails for a specific path on a reachable peer, try that same listing up to `--retries-list` total times. Directory listing failures are not placed in the file-copy queue; they affect visibility for a whole subtree, not one file transfer.

If listing still fails after all allowed tries, that peer is excluded from decisions for that directory and its entire subtree (equivalent to an offline peer for that path). The error is logged at `error` level. The peer's manifests for that subtree are not rewritten - no `last_seen` is refreshed and no tombstone is added, so no false deletions are inferred. No files or directories are created, deleted, displaced, or copied on that peer under the failed subtree during this run.

If the failed listing is for the canon peer (`+`), skip decisions for that
directory and its entire subtree for all peers. No other peer can supply
authoritative state for that path while canon is unavailable, so no peer files
and no manifests are modified under that subtree during this run.

If all contributing peers fail listing for a directory (none of the contributing peers remain in `active_peers` for that level), skip decisions for that directory and its entire subtree - no entries are processed and no subordinate peer files are displaced. On the next run, the failed peer participates normally again if listing succeeds.

**Excludes:** Built-in excludes and paths supplied with `-x` are removed from
the entry union before decisions are made. A matching directory is not recursed
into. A matching file is not copied or deleted. Excluded paths do not consult
manifest lines during the run, and existing entries on any peer are left in
place; an existing manifest line for an excluded path is carried forward
unchanged when the manifest is rewritten.

## Subordinate Peers

A subordinate peer (`-` prefix on the command line) participates in listing and receives file operations, but does not contribute to decisions:

- Its entries are **not included** in the `gather_states` step - decisions are made as if the subordinate peer doesn't exist.
- After a decision is made, the subordinate peer is brought into conformance: files it has that shouldn't exist are displaced to BAK/, files it lacks are copied to it, directories are created or removed as needed.
- Its manifests are still read during traversal and rewritten like any other peer's. In `--dry-run`, no manifest is written. On future real runs without `-`, the peer participates normally.

This means a subordinate peer with pre-existing files that differ from the group's state will have those files displaced - it is made to match the group, not the other way around.

## Built-in Excludes

Always excluded from listings (never synced):

- `.kitchensync/` directories - sync metadata must not sync
- `.git/` directories - repository metadata must not sync
- Symbolic links (files and directories) - following symlinks could escape the sync root or create loops
- Special files (devices, FIFOs, sockets)

Excluded by pattern (see sync.md, "Excludes"): the built-in litter patterns,
each peer's `.kitchensync/ignore`, and `-x` patterns from the command line,
applied in that order with the last matching pattern winning.

## SWAP Recovery During Traversal

In `--dry-run`, peer-side SWAP recovery during traversal is skipped.

Before listing a directory for sync decisions in a normal run, check each peer for
`.kitchensync/SWAP/` at that directory level. Each direct child is one
`<encoded-basename>` swap directory for the corresponding user entry in the
same parent directory. Recover every swap directory before the directory's live
entries are listed for sync decisions.

For target `<basename>`:

- `old` exists and target exists: replacement completed; delete `new` if
  present, move `old` to BAK, and remove the empty SWAP directory.
- `old` exists, `new` exists, and target is missing: rename `new` to target,
  move `old` to BAK, and remove the empty SWAP directory.
- `old` exists, `new` is missing, and target is missing: rename `old` back to
  target and remove the empty SWAP directory.
- `old` is missing, `new` exists, and target exists: delete `new` and remove
  the empty SWAP directory.
- `old` is missing, `new` exists, and target is missing: rename `new` to target
  and remove the empty SWAP directory.

If recovery for a swap directory fails, treat that peer's listing for the
current directory as failed. The peer is excluded from this directory subtree
using the normal listing-error rules, and its manifests for the subtree are not
rewritten.

## BAK Cleanup During Traversal

In normal runs, after processing the union of entry names at each directory
level, separately check each peer for a `.kitchensync/` directory at the current
path (using `list_dir` or `stat` directly - this is a metadata operation, not a
sync operation, so the built-in exclude does not apply). If present, list its
`BAK/` subdirectories and purge expired entries:

- `.kitchensync/BAK/<timestamp>/` - remove entries older than `--keep-bak-days` days

This piggybacks on the existing traversal - no separate tree walk is needed. The `<timestamp>` component of each subdirectory name determines its age.

Do not purge `.kitchensync/SWAP/` by age. SWAP directories are recovered before
listing and are deleted only by successful recovery.

Old deletion records need no cleanup pass of their own: tombstones older than
`--keep-del-days` are dropped when the directory's manifest is rewritten (see
manifest.md).

In `--dry-run`, BAK cleanup on peers is skipped.

## Entry Classification

For each **file** entry, compare each contributing peer's state to that peer's line in this directory's manifest. A live file is unchanged only when both its mod_time and byte_size match the manifest line. (Directories are decided by existence and tombstone evidence, never their own mod_time - see "Directory Decisions" below.)

| Peer State               | Manifest line for **this peer** | `deleted_time` | Classification                                 |
| ------------------------ | ------------------------------- | -------------- | ---------------------------------------------- |
| Live, same mod_time and byte_size | Exists                 | `-`            | Unchanged                                      |
| Live, different mod_time or byte_size | Exists            | `-`            | Modified                                       |
| Live                     | Exists                          | set            | Modified (resurrection - clear `deleted_time`) |
| Live                     | No line                         | n/a            | New (peer has never had this entry)            |
| Absent                   | Exists                          | set            | Deleted (estimate = `deleted_time`)            |
| Absent                   | Exists                          | `-`            | Absent-unconfirmed (see rule 4b)               |
| Absent                   | No line                         | n/a            | - (never existed on this peer, no opinion)     |

A peer whose manifest for this directory is missing altogether has no line for any entry here, so every live entry on it is New and every absent entry is no opinion. See "Directories Without History" below.

## Decision Rules

### With a canon peer (`+`)

The canonical peer's state wins unconditionally:
- Canon has file -> push to all others (including subordinate peers)
- Canon lacks file -> delete everywhere else
- Canon is unreachable -> exit with error at startup

### Without a canon peer

Only contributing (non-subordinate) peers participate in decisions:

1. **All contributing peers unchanged and matching** -> the unchanged entry is
   the group outcome. No copy is needed between contributing peers that already
   match, but any active peer that lacks the entry or has the wrong type
   (including subordinate peers) is brought into conformance.
2. **Modified** -> newest mod_time wins; push to all that don't match
3. **New** -> newest mod_time wins; push to all peers that lack it (including peers with no manifest line)
4. **Deleted + existing** -> compare the deletion estimate against the existing file's mod_time. The deletion estimate is the `last_seen` or `deleted_time` on the absent peer's manifest line (see 4b for which applies). If multiple peers have deleted the entry, use the most recent estimate among the deleting peers. If the deletion estimate > mod_time, deletion wins (displace the file on all peers that have it). If mod_time >= the deletion estimate, the existing file wins (push to peers that lack it)
4b. **Absent-unconfirmed** (absent, the peer has a manifest line for the entry with `deleted_time` `-`) -> compare that line's `last_seen` against the max mod_time of peers that have the entry. If `last_seen` > max mod_time, this is a deletion - the entry was confirmed present on this peer after the latest modification anywhere, and has since been removed. Apply rule 4 using `last_seen` as the deletion estimate. If `last_seen` <= max mod_time (or `last_seen` is `-`), this is a failed copy or the peer has never successfully received the file - re-enqueue the copy, no deletion vote
5. **Same mod_time, different size** -> larger file wins
6. **Ties** -> keep data (existence over deletion, larger over smaller)
7. **Exact tie** (mod_time within tolerance and equal byte_size) -> the entries
   are treated as identical, even if their bytes differ. No copy is enqueued
   between the tied peers; each keeps its current content, and only manifest
   lines are updated. Content is never read or hashed to break a tie -
   decisions use only mod_time and byte_size. When another peer needs the
   entry, any one tied peer may be chosen as the copy source; the recorded
   winning mod_time and byte_size are the tied values either way.

Peers with no manifest line for the entry ("never had it") do not vote - they are simply targets for propagation once a winner is decided.

If no contributing peer votes (all have "absent, no line"), the entry does not exist in the group's view. No copy is enqueued. Subordinate peers that have the entry are displaced to BAK/.

If the winning entry already exists on a peer with a matching mod_time (within tolerance) and matching byte_size, no copy is performed for that peer - only the manifest line is created or updated.

Timestamp tolerance: 5 seconds in either direction. The tolerance applies to Entry Classification: a peer's mod_time is considered "same" as the manifest line's mod_time if it differs by <= 5 seconds. When comparing peers' mod_times in Decision Rules, find the maximum mod_time among all peers that have the entry. Any peer whose mod_time is within 5 seconds of the maximum is treated as tied with the maximum (fall through to rules 5/6). Peers whose mod_time is more than 5 seconds behind the maximum lose to it. The same tolerance applies when comparing a deletion estimate (`deleted_time`) against a file's mod_time in rule 4. The same tolerance applies to rule 4b: `last_seen` must exceed `max mod_time` by more than 5 seconds to be considered a deletion.

## Directories Without History

History is per directory, so a peer can have history for one directory and none for another. A peer with no manifest for the directory being decided simply has no line for any entry there: it votes for the entries it has live, and it has no opinion about entries it lacks. It cannot vote for deletion, because nothing records that it ever held the missing entry.

When **no** contributing peer has a manifest for a directory, the whole directory is merged additively:

- every live entry is New;
- for files, the newest mod_time wins and the winner is copied to every peer that lacks it or differs;
- directories exist everywhere and are recursed into;
- nothing in that directory is deleted or displaced, except a type conflict (file versus directory at the same name) and bringing a subordinate peer into conformance.

This is the same rule the sync root follows on a first run, and it applies at any depth. A directory reached for the first time under an already-synced parent is merged additively even though the parent has history, and a subdirectory that already has a manifest keeps using it even when the run starts from a parent that has none. See sync.md, "Canon Peer", for the one-line notice printed when the sync root itself has no history.

A canon peer (`+`) overrides all of this as usual: canon's contents win whether or not any manifest exists.

## Directory Decisions

Directories do not use mod_time for decision-making. Directory mod_times are filesystem bookkeeping (they change when children are added or removed) and vary in precision across filesystem types - they do not represent meaningful user intent.

Directory decisions use existence and the manifest lines for the directory
itself (never the directory's own mod_time):

- Canon peer (`+`) overrides as usual: canon has it -> create everywhere;
  canon lacks it -> delete everywhere.
- If every contributing peer that votes has the directory live, it should
  exist on all peers. Create it on active peers that lack it, and recurse.
- If at least one contributing peer has the directory live and at least one
  contributing peer votes deletion - absent in the current listing with a
  manifest line for the directory - the directory survives for this run: it
  is created on active peers that lack it, and the walk recurses into it.
  The decision is then made entry by entry inside, where the real evidence
  is:
  - The deletion estimate is the absent peer's line's `deleted_time` if set,
    else its `last_seen` (rule 4b's estimate). If several peers vote
    deletion, use the most recent estimate among them.
  - Inside the directory, file rules 4 and 4b remove every entry that is
    older than that deletion estimate, while anything newer survives and
    propagates by the normal file rules. A directory that was deleted on one
    peer and left untouched on another therefore empties out, and a
    directory that was deleted on one peer while new content arrived on
    another keeps just the new content.
  - If, after recursing, no live entry remains in the directory on any peer,
    the directory itself is displaced to BAK/ (as an empty directory) on
    every active peer that still has it, and tombstoned in the parent's
    manifest on those peers.
  - Nothing is listed recursively to decide this. The choice to keep the
    directory for one more level costs at most one extra pass and never
    risks deleting content that is newer than the deletion.
- If no contributing peer has the directory live in its listing, at least one contributing peer has a manifest line for the directory, and every contributing peer with a manifest line for the directory is absent in the current listing, delete it on all remaining peers (displace to BAK/). A line with `deleted_time` set is already a recorded deletion. A line with `deleted_time` `-` becomes a confirmed absence for this run and is tombstoned using the normal manifest update rule.
- A contributing peer with no manifest line for the directory has no opinion: it neither votes deletion nor blocks one - consistent with the file decision rules where peers with no line do not vote. A peer with the directory live votes for existence regardless of lines.
- If no contributing peer has the directory - neither live in its listing nor as a manifest line (with or without tombstone) - the directory does not exist in the group's view. Subordinate peers that have it are displaced to BAK/.

Directories are displaced to BAK/ just like files. Manifests still track directories (with `byte_size` `-1`) for deletion detection via tombstones, but `mod_time` on a directory line is informational only - it is recorded but not used in decisions.

## Type Conflicts

When the same path is a file on one peer and a directory on another: if a canon peer is present, the canon peer's state wins unconditionally. Canon has a file -> displace directories and sync the file everywhere. Canon has a directory -> displace files and create/sync the directory everywhere. Canon lacks the path -> displace the path everywhere else.

Without a canon peer, type-conflict decisions are based on contributing peers only. If at least one contributing peer has a file and at least one contributing peer has a directory at the same path, the file wins. The directory is displaced to BAK/ on the contributing peer(s) that have it, then the winning file is selected by the normal decision rules (rules 1-6) applied to the contributing file entries only and synced to all active peers. A subordinate peer's file does not make the file win over a contributing peer's directory; after the contributing decision is made, any subordinate path with the wrong type is displaced to BAK/ and replaced as needed.

## Manifest Updates

Each peer's manifest for a directory is assembled while that directory is being decided and written once, after every entry there is decided and every copy into that directory on that peer has finished or failed (see manifest.md, "Writing"). What gets recorded for an entry depends on what has actually been confirmed:

- Listed state may be recorded immediately because the entry has already been observed on that peer.
- Queued file-copy destinations may be recorded as intended state before the copy runs, but their `last_seen` stays `-` until the copy succeeds.
- Inline filesystem operations such as directory creation and displacement to
  BAK/ change the affected peer's record only after the operation succeeds.
  If the operation fails, that peer's existing line is carried forward
  unchanged.

- **Entry confirmed present** on a peer: write a line with the current mod_time, byte_size, `last_seen` set to a freshly generated timestamp, and `deleted_time` `-`. Keep the existing line's `placed` value when the observed mod_time and byte_size match that line; clear `placed` to `-` when either differs, because the user changed the content (see manifest.md, "Format")
- **Entry confirmed absent** on a peer whose line has `deleted_time` `-`: keep the line and set `deleted_time` to the deletion estimate - the line's `last_seen` if it has one, otherwise a freshly generated timestamp (the deletion happened sometime after that point). Do not update `last_seen`.
- **Entry confirmed absent** on a peer whose line already has `deleted_time` set: no change (tombstone already recorded)
- **Decision: push to a peer**: write a line for the destination peer with the winning entry's mod_time, byte_size, `deleted_time` `-`, and `placed` `-` until the copy completes. Do **not** set `last_seen` - it is only set when the entry is confirmed present (in a listing or after a completed copy). Until then `last_seen` is `-`.
- **Copy completed**: when a file copy finishes successfully, `last_seen` and `placed` on the destination peer's line both become a freshly generated timestamp - KitchenSync put that content there. Because the directory's manifest is not written until its copies have finished or failed, this needs no second write.
- **Inline directory creation completed**: after `create_dir` succeeds on a destination peer, that peer's line gets `last_seen` and `placed` set to a freshly generated timestamp. Directory creation is both decided and confirmed in one step (unlike file copies, which are enqueued).
- **Displacement completed**: after the entry is successfully moved to BAK/, set `deleted_time` on that peer's line as described above. Nothing cascades to descendants: a displaced directory carries its own `.kitchensync/` folder and every manifest below it into BAK/, so the removed subtree's history leaves with the subtree.

If the app exits before copies finish, the destination directory's manifest was either not written at all or written with `last_seen` `-` for the unfinished copy. The next run sees the entry as absent-unconfirmed and applies rule 4b: `last_seen` is `-` or older than the source's mod_time, so it is not read as a deletion and the copy is re-enqueued.

## Offline Peers

Unreachable peers are excluded entirely - they do not participate in listings or decisions. Their manifests are not read and not rewritten, so no `last_seen` is refreshed and no tombstone is added. On the next run when they're reachable, discrepancies between their filesystem state and their manifests drive sync decisions, bringing them up to date.
