# traversal Module API

`traversal` has no exported public API.

This module is a private leaf inside `sync`. Other first-layer modules must not
depend on `sync::traversal` types, traits, functions, cursors, listing state, or
candidate records. The public sync contract is owned by the parent `sync`
module, which coordinates traversal with decision logic, snapshot updates,
operation dispatch, copy scheduling, progress events, diagnostics, and report
assembly.

Rust items in `traversal` should therefore remain private to `sync` unless a
future shared contract is required by another module. In that case, the shared
type or trait belongs at the nearest common ancestor rather than in this private
child module.

Private traversal implementation may define internal Rust structs, enums,
traits, callbacks, and functions for:

- recursive pre-order directory walking;
- subtree-scoped active peer tracking;
- directory listing retries and listing failure classification;
- traversal-owned SWAP recovery and BAK/TMP cleanup request points;
- deterministic live candidate ordering;
- exclude enforcement before downstream processing;
- scanned-directory and skipped-subtree accounting.

Those implementation details are not stable and must not be imported by
sibling modules.
