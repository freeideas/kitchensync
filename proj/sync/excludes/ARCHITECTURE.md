# excludes Architecture

## Purpose

`excludes` owns the run-scoped predicate that decides whether a candidate
relative path is ignored by sync before any snapshot lookup, classification,
decision, operation, recursion, or snapshot update can occur. An excluded path
is treated as nonexistent for the current run: existing peer contents at that
path are left untouched, the path is not copied or displaced, excluded
directories are not recursed into, and no snapshot row is read or changed for
the excluded path or its excluded subtree.

The module recognizes only exclusion behavior required by the sync contract:
built-in omissions for KitchenSync metadata, VCS metadata, symbolic links,
special files, and other non-regular entries omitted by transport listing or
stat behavior; plus validated root-relative exclude anchors provided by
`RunConfig.excludes`.

## Responsibilities

`excludes` builds and applies a small predicate over validated `RelPath`
values and transport metadata. Its responsibilities are:

- compile the command-line exclude values already present on `RunConfig` into
  immutable root-relative path anchors without accepting raw CLI strings or
  defining another path syntax;
- identify built-in candidate directory name exclusions such as `.kitchensync`
  and `.git` wherever they appear;
- reject symbolic links, special files, and any non-regular entry that the
  transport surface exposes as outside the file/directory sync domain;
- expose a traversal-local check for candidate names before the caller asks
  snapshots, decision rules, operations, copy scheduling, or snapshot mutation
  to consider that path;
- expose a directory-recursion check so excluded directory subtrees are skipped
  as a whole.

The module does not inspect snapshot rows, choose winners, produce sync
outcomes, mutate peer files, schedule copy work, update stored state, render
diagnostics, or decide listing-failure behavior.

## Data Flow

At the start of a sync run, traversal constructs an exclude predicate from
`RunConfig.excludes`. The compiled predicate is immutable for the rest of the
run and is shared by traversal steps that enumerate candidate paths.

For each listed candidate entry, traversal forms the candidate `RelPath` from
the current directory and the transport-supplied name, then asks `excludes`
whether that path and entry metadata are excluded. If the answer is yes,
traversal drops the candidate immediately. No snapshot lookup, peer-state
classification, decision rule, operation dispatch, copy-task creation,
recursion, progress accounting for a decided entry, or snapshot update is
performed for that candidate.

For directory entries that are not excluded, traversal may recurse normally.
For directory entries that match a built-in directory exclusion or exactly
match a configured exclude anchor, traversal does not recurse into the subtree
on any peer for the current run.

For entries whose transport listing or stat behavior omits non-regular objects,
there is no candidate to filter. If metadata is available and marks an entry as
symbolic-link, special, or otherwise outside regular file/directory behavior,
`excludes` rejects it before the rest of sync observes it as a live file or
directory.

## Matching Rules

All matching is over root-relative `RelPath` values. `excludes` depends on the
root-owned path contract for validation and does not define an alternate path
syntax.

Each command-line exclude anchor matches a candidate whose `RelPath` is exactly
equal to that anchor. When the exact-match candidate is a directory, the
directory is also an excluded subtree root: traversal skips recursion there,
and any descendant candidate that is nevertheless considered by a caller is
excluded by the same anchor-prefix rule.

The module does not infer whether a configured exclude names a file or
directory by consulting snapshots or transports. Directory-subtree behavior is
therefore driven by the live candidate metadata supplied with an exact-match
directory, or by a caller asking about a descendant of a directory already
known to be excluded.

Built-in directory excludes match reserved metadata directory names, including
`.kitchensync` and `.git`, wherever they appear as candidate directory names
and skip their full subtrees. Built-in non-regular exclusions are driven by
metadata kind or by transport omission, not by snapshot contents.

The predicate must be deterministic and independent of peer order. It may rely
on the caller's validated `RelPath` and `EntryMeta` values, but it must not
perform transport I/O or snapshot I/O itself.

## Dependencies

`excludes` imports only narrow contracts from its ancestors:

- `RunConfig` for the command-line exclude list;
- `RelPath` for validated root-relative paths;
- `EntryMeta` and `EntryKind` for transport-provided candidate metadata.

It is private implementation inside `sync`. Other first-layer modules must not
call it directly and must interact with exclude behavior only through
`kitchensync::sync::run` and the observable sync effects described by the sync
API.

## Internal Design

The implementation should keep one run-scoped predicate object with the
configured exclude anchors and fixed built-in directory names. Configured
anchors are exact path matches until live traversal observes an exact-match
directory; at that point the same anchor also answers the directory-subtree
skip question. Built-in path excludes can be represented as reserved directory
name checks, because `.kitchensync` and `.git` are excluded wherever they
appear as candidate directory names. Metadata-based exclusion should be a
direct entry-kind check used when live candidate metadata is available.

Traversal should call the predicate at the boundary where candidate live names
are assembled. This keeps excluded paths out of later private sync records such
as candidate sets, classifications, decisions, operation plans, and snapshot
flows.

The predicate should return a simple boolean or a small private reason enum
only if local tests or diagnostics require distinguishing built-in, configured,
and metadata exclusions. Any such reason type remains private to `sync`; it is
not part of the public sync API.

## Leaf Scope

This scope is a leaf. The module is a narrow predicate over paths and entry
metadata, so subdividing it would create artificial child boundaries without
reducing future implementation risk. No child modules should be created under
`proj/sync/excludes`.
