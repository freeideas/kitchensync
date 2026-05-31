# excludes API

The `excludes` module has no exported API for other modules.

`excludes` is a private leaf inside `kitchensync::sync`. Its predicate exists
only to let sync traversal remove excluded candidate paths before snapshot
lookup, classification, decision-making, operation dispatch, copy scheduling,
recursion, or snapshot updates. Sibling first-layer modules must not import
this module or depend on its internal Rust items.

## Public Rust Surface

No language-native public types, traits, functions, records, or errors are
exported from `excludes`.

Any Rust implementation details for the run-scoped exclude predicate, command
exclude anchors, built-in directory-name checks, metadata kind checks, and
local exclusion reasons must remain private to `sync` or narrower, using Rust
visibility such as private items or `pub(super)` only when needed by the parent
sync traversal implementation.

## Ownership Rules

Other modules may rely only on the observable `sync` contract: excluded paths
are absent from later sync work for the current run, and excluded peer entries
are left untouched. They must not rely on ownership, allocation, ordering, or
reason details from the `excludes` implementation.
