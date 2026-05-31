# excludes:

## Purpose

Own the private sync predicate that removes excluded candidate paths before
snapshot lookup, classification, decision-making, operation dispatch, copy
scheduling, recursion, or snapshot updates.

An excluded path is treated as nonexistent for the current run. Existing peer
files and directories at excluded paths are left untouched, absent excluded
paths are not created or copied, excluded entries are not displaced or deleted,
excluded directories are not recursed into, and snapshot rows for excluded paths
or excluded descendants are not consulted or changed.

The module covers only exclusion behavior inside `kitchensync::sync`: built-in
metadata and non-regular entry exclusions, plus command-line excludes already
validated and stored on `RunConfig.excludes`. It is a leaf module and must not
grow traversal, classification, decision, operation, runtime, or snapshot
storage responsibilities.

## Responsibilities

- Build one immutable run-scoped exclude predicate from `RunConfig.excludes`.
  The input paths are already validated root-relative `RelPath` values; this
  module does not accept raw CLI strings or define a second path syntax.
- Match command-line excludes against root-relative candidate paths. A candidate
  whose path equals an excluded path is excluded. A candidate below an excluded
  directory path is excluded as part of that directory subtree.
- Treat command-line excludes as additive to built-in excludes. A configured
  exclude cannot include or override `.kitchensync/`, `.git/`, symbolic links,
  special files, or other non-regular entries.
- Exclude `.kitchensync` directories wherever they appear as candidate
  directory names, including per-directory metadata directories used for SWAP,
  BAK, TMP, and snapshot state.
- Exclude `.git` directories wherever they appear as candidate directory names.
- Exclude symbolic-link files, symbolic-link directories, devices, FIFOs,
  sockets, and any other non-regular entry if such an entry is visible through
  candidate metadata. When transport listing or stat omits those entries by
  returning no candidate or `not_found`, this module has no extra work to do.
- Provide a deterministic check for a candidate `RelPath` and optional
  transport-provided entry metadata before any later sync child module sees the
  candidate.
- Provide the directory-subtree answer needed by traversal so a directory that
  matches a built-in or command-line exclude is skipped without visiting its
  descendants.
- Preserve transport-supplied filename spelling by deciding only whether a path
  is excluded; it must not normalize, rewrite, or choose replacement names.
- Keep any local reason enum private. Public sync behavior depends on excluded
  paths being absent from later work, not on a public exclusion-reason API.

## Boundaries

- `excludes` owns only path and metadata exclusion checks for candidates that
  traversal has already assembled from live listings.
- It does not list directories, retry listings, recover SWAP state, decide
  active peer sets, sort candidates, recurse, perform BAK/TMP cleanup, or
  handle listing-failure subtree rules.
- It does not parse command-line arguments or validate `-x` syntax. CLI
  validation owns rejecting leading slashes, trailing slashes, backslashes,
  empty segments, `.`, `..`, NUL characters, and missing values.
- It does not inspect, create, update, tombstone, purge, hash, or upload
  snapshot rows. Excluded paths must be filtered before snapshot lookup.
- It does not classify peer states, choose canon or bidirectional outcomes,
  resolve file-vs-directory conflicts, apply timestamp tolerance, or decide
  whether a peer should receive a copy, directory, or displacement.
- It does not call transport operations. It relies on traversal-supplied
  `RelPath` values and available `EntryMeta` or `EntryKind` values, and it does
  not perform stat calls to discover whether an exclude names a file or
  directory.
- It does not mutate peer files, create directories, displace entries, schedule
  copies, suppress dry-run mutations, or clean staging directories.
- It does not emit diagnostics or progress events. Excluded paths are silently
  omitted from later sync work unless the parent sync module later defines a
  private test hook.
- It is private under `sync`; sibling first-layer modules must observe exclude
  behavior only through `kitchensync::sync::run` effects and reports, not by
  importing this child module.

## Error Obligations

- Invalid command-line exclude values must not reach this module. If they do,
  the predicate construction must fail closed for the sync run rather than
  silently accepting an alternate path interpretation.
- Exclusion checks must be pure and deterministic for the same run
  configuration, candidate path, and metadata. They must not depend on peer
  order, snapshot contents, filesystem state outside the supplied metadata, or
  previous candidates.
- If candidate metadata cannot express a non-regular type because transport
  already omitted it, this module must not infer existence from snapshots or
  attempt transport I/O to find it.
- Once a candidate is reported excluded, callers must be able to rely on no
  downstream snapshot lookup, decision, peer mutation, copy task, or recursion
  being required for that candidate in the current run.
