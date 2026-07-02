# QueueRunner:

## Purpose

QueueRunner owns the run-scoped scheduling of queued file-copy work. It accepts
eligible copy work as traversal discovers it, starts copy tries before the whole
tree has been scanned, enforces one global active file-copy limit for the run,
and tracks each queued copy's tries independently.

QueueRunner does not perform the file replacement phases itself. For each copy
try that receives a global copy slot, it calls the staged-transfer child and
uses that try result to decide whether the queued copy is complete, should be
placed behind other queued work, or should stop for the rest of the run.

## Responsibilities

QueueRunner exposes a run operation configured with:

- a maximum active copy count, defaulting to `10` when the caller did not
  supply `--max-copies`;
- a maximum total try count for each queued copy, defaulting to `3` when the
  caller did not supply `--retries-copy`;
- a staged-transfer operation that performs one copy try;
- an event sink for copy start, copy-slot acquire, copy-slot release, transfer
  success, transfer skip, and transfer failure events.

QueueRunner exposes an enqueue operation that may be called while tree scanning
is still in progress. A newly enqueued copy becomes eligible to start as soon
as it is queued and a global copy slot is available. QueueRunner must not wait
for traversal to finish before starting eligible file-copy work.

QueueRunner exposes a close-and-drain operation. Closing means traversal will
not enqueue more copies. Draining waits until all queued work has either
succeeded, been skipped for this run, or reached its copy try limit.

QueueRunner applies one active file-copy limit across the whole run. Every copy
try holds one slot from just before the staged-transfer try begins until after
that try has returned and QueueRunner has emitted the matching slot-release
event. These source and destination combinations all count against the same
global slot pool:

- `file://` source to `file://` destination;
- `file://` source to `sftp://` destination;
- `sftp://` source to `file://` destination;
- `sftp://` source to `sftp://` destination.

QueueRunner does not count directory listing, snapshot download, snapshot
upload, directory creation, BAK cleanup, TMP cleanup, or SWAP cleanup against
the active file-copy limit. Those activities are outside this child's slot
pool even when they happen during the same product run.

QueueRunner must not create a lower per-peer, per-host, or per-connection
active-copy limit. If the global limit is `N`, any mix of eligible queued copy
work may occupy up to `N` active slots across the whole run, subject only to
available queued work and completion of existing tries.

Each queued copy owns its own try count. The first staged-transfer call for a
queued copy counts as try `1`. `--retries-copy N` allows at most `N` total tries
for that queued copy, including the first try. Try counts for one queued copy
must never increase, reset, or cap the tries of another queued copy.

When a staged-transfer try succeeds, QueueRunner records that queued copy as
complete for the run and does not enqueue it again. When a staged-transfer try
returns a skip result for the run, QueueRunner records that queued copy as
skipped for the run and does not enqueue it again.

When a staged-transfer try fails before the queued copy has reached its total
try limit, QueueRunner increments only that queued copy's try count, places
that same queued copy behind other queued copy work, releases the slot for the
failed try, and continues draining other queued work in the same run.

When a staged-transfer try fails and the queued copy has reached its total try
limit, QueueRunner records that queued copy as failed for the run and does not
requeue it again in that run.

Copy try accounting is scheme-independent. QueueRunner applies the same total
try limit, requeue-behind rule, and no-requeue-after-limit rule to local
copies, SFTP copies, and mixed local/SFTP copies.

QueueRunner reports slot events around every staged-transfer try. A slot
acquire event includes the active count after acquiring the slot and the global
maximum. A slot release event includes the active count after releasing the
slot and the global maximum. The active count must describe this child's
global file-copy slot pool, not peer counts, host counts, or transport
connection counts.

## Boundaries

QueueRunner does not decide which files should be copied, which peer is canon,
which peer is subordinate, which paths are excluded, which directories are
traversed, or which entries should be displaced. The caller supplies copy work
that is already eligible.

QueueRunner does not connect peers, parse URLs, authenticate SFTP, list
directories, download or upload snapshots, create directories for traversal,
clean BAK/TMP/SWAP staging areas outside a copy try, or perform scheme-specific
filesystem calls.

QueueRunner does not derive SWAP paths, stream file bytes, move existing
destination files, rename staged content into place, set modification times,
archive old files, or clean the staged directories for one replacement. Those
phases belong to the staged-transfer child called for each try.

QueueRunner does not update snapshots and does not format stdout. It returns
structured run results and emits structured events so its caller can update
snapshots and route user-visible output through the output child.

QueueRunner's error obligation is scheduling correctness after staged-transfer
results. It must release a slot exactly once for every acquired slot, keep
draining other queued work after a retryable failure, and keep the failed copy
out of the queue after a skip result or exhausted try limit. Cleanup inside a
failed copy try is reported by staged transfer; QueueRunner only applies the
retry or stop decision.

QueueRunner must preserve these invariants:

- active file copies across the whole run never exceed the configured global
  maximum;
- active copy slots are shared across every supported source and destination
  scheme combination;
- no per-peer, per-host, or per-connection limit is imposed below the global
  maximum;
- eligible queued work can start before traversal closes the queue;
- each queued copy tracks total tries independently;
- the first try counts toward the total try limit;
- retryable failed copies move behind other queued copy work;
- other queued copy work continues after a retryable failure;
- copies that reach their total try limit are not requeued in the same run;
- local, SFTP, and mixed-scheme copies use the same try-limit rules.
