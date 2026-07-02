# PeerRunRoles:

## Purpose

PeerRunRoles classifies peers for one KitchenSync run. It receives startup facts
about configured peers, reachability, command-line role markers, canon
designation, and whether each reachable peer had `.kitchensync/snapshot.db` on
disk at startup. It returns structured role facts that the TreeSyncPlanner
facade and sibling decision planners can use to decide which peers may vote,
which peers are active targets, and which startup failures must stop the run.

This child does not decide file, directory, or type-conflict outcomes. It only
defines who is allowed to contribute to those decisions and who remains eligible
to receive the selected outcome.

## Responsibilities

PeerRunRoles exposes a startup classification operation. The operation accepts
all peers known to the run and returns either a fatal startup result or a
successful run-role result.

The successful result contains only peers that are reachable at startup. A peer
that is unreachable at startup and does not cause a fatal startup result is
omitted from the active peer set. Omitted peers must not be present in listing
requests, sync decision inputs, sync decision targets, or snapshot update
targets for that run.

For each reachable peer, the operation returns:

- the peer identity used by the parent planner;
- whether the peer is the designated canon peer for this run;
- whether the peer is contributing and may vote in sync decisions;
- whether the peer is subordinate and cannot vote;
- whether the peer is an active target that receives the selected outcome after
  a decision is chosen.

A reachable canon peer is always contributing for the run, even when its
`.kitchensync/snapshot.db` did not exist on disk at startup. The returned facts
must mark a reachable canon peer as the final authority for conflict decisions:
sibling outcome planners must be able to tell that the canon peer's state wins
unconditionally.

A reachable non-canon peer whose `.kitchensync/snapshot.db` did not exist on
disk at startup is subordinate for that run. It is active and targetable, but it
does not vote in decision selection.

A reachable peer marked with `-` is subordinate for that run even when it has
snapshot history. It is active and targetable, but it does not vote in decision
selection.

A reachable peer that is not canon, is not marked with `-`, and has startup
snapshot history is contributing for that run. A peer that was subordinate in a
previous normal run follows the same rule on a later run: when it is reachable,
has snapshot history, and is not marked with `-`, it is contributing. Previous
subordination by itself must not be remembered as a later-run role.

Subordinate peers never contribute votes to sync decisions. They remain active
targets after a decision is selected, so sibling outcome planners can select
copy, creation, or displacement intents for them from the contributing outcome.

The operation returns the required fatal startup results:

- If a canon peer is designated but that peer is unreachable at startup, return
  a fatal result with exit status `1`. No stdout line is required by this child
  for that failure.
- If no peer in the reachable set has startup snapshot data and no canon peer is
  designated, return a fatal first-sync result with exit status `1` and stdout
  line `First sync? Mark the authoritative peer with a leading +`.
- After the first-sync case is ruled out, if automatic subordination leaves no
  reachable contributing peer, return a fatal no-contributing-peer result with
  exit status `1` and stdout line `No contributing peer reachable - cannot
  make sync decisions`.

A run with reachable snapshot history on at least one contributing peer does not
require a canon peer. The successful result must allow sibling planners to make
ordinary non-canon decisions from that contributing peer set.

Unreachable state is run-local. If a peer is unreachable in one run and
reachable in a later run, this child classifies it from the later run's current
marker and startup snapshot facts. When that later classification makes the
peer contributing, sibling planners use its filesystem state and existing
snapshot rows to drive sync decisions.

## Boundaries

PeerRunRoles does not parse command-line arguments, normalize peer URLs, connect
to peers, check whether a snapshot database file exists, read SQLite rows, list
directories, inspect file metadata, decide file or directory outcomes, execute
copies, move entries to `BAK/`, write snapshot rows, or format stdout beyond
returning the exact stdout text required for its fatal startup results.

The child returns structured facts and fatal intents only. The parent facade is
responsible for wiring those facts into tree traversal, file decisions,
directory decisions, type-conflict decisions, output, and process exit handling.

PeerRunRoles must preserve these invariants:

- exactly the reachable, non-fatal peers appear in the active peer set;
- unreachable non-fatal peers are absent from all listing, decision, target, and
  snapshot-update eligibility facts for the run;
- canon reachability is checked before any successful role result is returned;
- first-sync failure is reported when there is no reachable snapshot history and
  no canon designation;
- no-contributing-peer failure is reported when the run otherwise starts but no
  reachable peer can vote;
- a reachable canon peer can vote and has unconditional authority;
- a subordinate peer cannot vote but remains an active target;
- previous-run role classifications do not affect the current run.
