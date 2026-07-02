# DryRunPolicy:

## Purpose

DryRunPolicy applies `--dry-run` behavior for one KitchenSync run. It lets the
run connect to peers, read peer state, make sync decisions, exercise queued
copy work, and update local temporary snapshot databases, while preventing
every peer-side mutation that dry-run forbids.

This child is a policy boundary. Other children still own command parsing,
peer connection mechanics, snapshot storage, traversal decisions, transport
calls, copy execution, staging recovery, cleanup, and stdout formatting.
DryRunPolicy tells those callers which dry-run operations must proceed, which
peer operations must be skipped, and which facts must be reported so the root
can keep normal startup and progress behavior.

## Responsibilities

DryRunPolicy exposes a run-mode policy for dry-run execution. The policy
answers these questions for callers before they invoke peer operations:

- whether startup should connect to peer URLs;
- whether missing peer root directories or missing peer root parents may be
  created;
- whether a missing peer root makes the current URL unreachable for this run;
- whether peer-side snapshot SWAP recovery may run before snapshot download;
- whether the live peer `.kitchensync/snapshot.db` should be downloaded as it
  currently exists;
- whether a missing peer snapshot should create a new empty local temporary
  snapshot database;
- whether traversal should list peer directories for decisions;
- whether peer-side user-data SWAP recovery may run during traversal;
- whether local temporary snapshot databases may be updated during traversal;
- whether queued copy work should be exercised;
- whether a queued copy should acquire an active-copy slot;
- whether a queued copy should read the source file;
- whether queued copy retry limits still apply;
- whether `C` progress events should be emitted for copy work in the same
  cases as a normal run;
- whether `X` progress events should be emitted for failed copy work in the
  same cases as a normal run;
- whether dry-run marker text must be emitted to stdout;
- whether peer-side directory creation, file creation, file-content writes,
  renames, deletes, BAK displacement, modification-time updates, snapshot
  upload, BAK cleanup, and TMP cleanup may execute.

During dry-run startup, DryRunPolicy requires connection establishment to run
for every peer URL. It forbids creating missing peer roots and missing root
parents through both `file://` and `sftp://` URLs. When the selected URL's root
path does not already exist, the policy makes that URL fail as unreachable for
this run rather than allowing creation.

During dry-run snapshot startup, DryRunPolicy skips peer-side
`.kitchensync/SWAP/snapshot.db/` recovery. If the live peer
`.kitchensync/snapshot.db` exists, the policy requires downloading that live
file exactly as it is currently present on the peer. If the peer snapshot is
not found on an otherwise reachable peer, the policy requires creating a new
empty local temporary snapshot database for that peer.

During dry-run traversal, DryRunPolicy allows directory listing for sync
decisions and allows local temporary snapshot database updates. It skips
peer-side `.kitchensync/SWAP/` recovery for user data at each directory level.
It also skips peer-side BAK cleanup and TMP cleanup during traversal.

For planned file copies in dry-run, DryRunPolicy requires the copy queue to be
used as in a normal run. Queued work acquires active-copy slots, reads source
files, tracks per-copy try counts, applies the `--retries-copy` total try
limit, and produces the same `C` progress events and failed-copy `X` progress
events that a normal run would produce. Destination-side writing is not
performed.

DryRunPolicy exposes a peer-mutation guard for every operation that would
change a `file://` or `sftp://` peer. In dry-run, the guard always returns a
skipped planned action for:

- creating peer directories, including TMP, SWAP, BAK, and destination
  directories;
- creating peer files;
- writing destination file content;
- renaming peer entries;
- deleting destination files;
- displacing destination entries to peer BAK storage;
- setting peer modification times;
- uploading updated local temporary snapshot databases to peers;
- cleaning peer BAK storage;
- cleaning peer TMP storage.

The guard must decide before the transport operation is called. A skipped
dry-run action is not a transport failure and must not be retried as if the
peer had produced an I/O error.

When the skipped peer mutation is a planned deletion or BAK displacement,
DryRunPolicy still allows the corresponding `X` progress event in the same
case where a normal run would emit it. The event describes the plan; it does
not mean the peer entry was renamed or removed.

DryRunPolicy exposes a completion rule for local temporary snapshots:
dry-run may leave the local temporary databases updated, but it must not
upload them to peers at the end of the run, including peers that are
subordinate for this run.

DryRunPolicy exposes an output marker requirement: a dry-run execution must
cause stdout to contain the exact phrase `dry run` at least once. The child may
return this as a structured output event; final formatting and printing belong
to the output owner.

## Boundaries

DryRunPolicy does not parse `--dry-run`, validate command-line arguments,
choose peer roles, normalize URLs, establish SFTP authentication, select
fallback URLs, or decide whether the process exits. It receives the already
known run mode and returns dry-run policy decisions.

DryRunPolicy does not perform transport operations. It does not list
directories, open source files, download snapshots, create local SQLite
databases, update snapshot rows, enqueue copies, own copy-slot semaphores,
count retries, or print stdout lines. It tells those owners which operations
must still run in dry-run and which peer mutations must be skipped.

DryRunPolicy does not decide sync outcomes. It does not classify file changes,
choose winners, choose copy sources, select displacement targets, or recurse
through the tree. It preserves the requirement that dry-run uses the same
planning inputs as a normal run wherever those inputs can be gathered by
reading peers and local temporary state.

DryRunPolicy does not hide read errors. Snapshot download errors, directory
listing errors, and source read failures remain errors for the owning child to
handle under the normal rules. In dry-run, a missing peer root is the specific
startup case this child maps to an unreachable URL instead of directory
creation.

## Invariants

- Dry-run connects to peer URLs during startup.
- Dry-run never creates a missing peer root or missing peer root parent.
- A peer URL with a missing root path is unreachable for that dry-run.
- Dry-run never runs peer-side snapshot SWAP recovery before snapshot
  download.
- Dry-run downloads an existing peer snapshot as the live file currently on
  the peer.
- Dry-run creates a new empty local temporary snapshot database when a
  reachable peer has no live snapshot.
- Dry-run lists peer directories for sync decisions.
- Dry-run never runs peer-side user-data SWAP recovery during traversal.
- Dry-run may update local temporary snapshot databases.
- Dry-run exercises the copy queue, active-copy slots, source reads, and
  copy retry limits.
- Dry-run emits `C` copy progress events, failed-copy `X` progress events, and
  planned removal or displacement `X` progress events in the same cases as a
  normal run.
- Dry-run stdout contains `dry run` at least once.
- Dry-run performs no peer-side directory creation, file creation, content
  write, rename, delete, BAK displacement, modification-time update, snapshot
  upload to any peer including subordinate peers, BAK cleanup, or TMP cleanup
  through `file://` or `sftp://` peer URLs.
