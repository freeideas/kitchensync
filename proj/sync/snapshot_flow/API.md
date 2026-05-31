# snapshot_flow API

`snapshot_flow` has no exported public API for other first-layer modules.

This module is an internal child of `sync`. It owns the timing and ordering of
snapshot-store mutations that arise from traversal observations, selected sync
outcomes, successful inline operations, and terminal copy results during one
prepared sync run.

Other modules must not depend on `snapshot_flow` types, helper records,
functions, traits, or error variants. Cross-module snapshot behavior is exposed
only through the root-owned `SnapshotStore` contract and the public
`kitchensync::sync` API.

Any Rust items implemented under this module should remain private to `sync`
using private or `pub(super)` visibility. They may borrow caller-owned
`SnapshotStore` handles, relative paths, metadata, and result values only for
the duration of a single event update. They must not own stores, retain
references after returning, clone peer sessions, expose database row-shape
helpers, or make SQLite, path-hash, row-id, timestamp-formatting, copy-scheduler,
operation-executor, or transport details visible as part of this module's
contract.

Snapshot mutation failures are returned only to the enclosing `sync` flow so
`sync` can report peer/path failures through its documented API. Cleanup
failures remain non-decision-blocking unless the `SnapshotStore` contract says
the store can no longer be used.
