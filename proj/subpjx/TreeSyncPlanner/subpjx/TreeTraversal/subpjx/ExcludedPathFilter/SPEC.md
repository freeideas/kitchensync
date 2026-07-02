# ExcludedPathFilter:

## Purpose

ExcludedPathFilter owns run-local exclusion policy for TreeTraversal. It receives
accepted command-line exclude paths and live entry classification facts from the
walk, then decides whether each path is visible to the rest of the planning
run.

The filter hides accepted command-line excludes and the built-in excluded
entries. Hidden paths must produce no scan request, recursion intent, sync
decision input, copy eligibility, deletion eligibility, displacement
eligibility, snapshot lookup eligibility, or snapshot update eligibility.

## Responsibilities

ExcludedPathFilter exposes an operation to build exclusion policy for one run
from the already accepted `-x <relative-path>` values. The operation treats each
accepted value as a run-local relative path match. It does not parse command-line
arguments or validate exclude syntax.

ExcludedPathFilter exposes a path visibility operation for TreeTraversal and
its facade. For each live path, the caller supplies the relative path and the
live entry classification known from listing facts: regular file, directory,
symbolic link file, symbolic link directory, or special file. The result states
whether the path is excluded and, when excluded, whether the exclusion applies
only to that exact path or to that directory and its descendants.

An accepted command-line exclude that matches a file excludes only that file
path for the run. An accepted command-line exclude that matches a directory
excludes that directory and every descendant path for the run. Descendant
exclusion is path-based after the directory match has been observed, so later
checks below that directory must stay hidden without scanning the directory
contents.

The built-in exclusion rules are always active. A directory named
`.kitchensync` is excluded with all descendants in every run. A directory named
`.git` is excluded with all descendants in every run. Symbolic link files,
symbolic link directories, and special files are excluded in every run. Built-in
excluded entries stay excluded regardless of the accepted command-line excludes
provided for that run.

The filter exposes eligibility decisions derived from visibility. For an
excluded path, every returned eligibility flag is false: scan, recursion, sync
decision, copy, delete, displace, snapshot lookup, and snapshot update. For a
non-excluded path, this child does not decide the final operation; it only
allows the caller to pass the live fact to the sibling planners or snapshot
owner for their own decisions.

The filter's error obligation is limited to its input shape. Accepted
command-line exclude values are assumed to have already passed command-line
validation before they reach this child. If an interface form later allows an
invalid relative path or unknown entry classification to cross this boundary,
the filter must fail that policy input without producing positive eligibility
for the path. It must not perform filesystem reads, transport listings,
snapshot reads, copies, deletes, moves, or other mutations while handling that
failure.

## Boundaries

ExcludedPathFilter does not parse `-x`, accept or reject command-line values,
walk directories, retry listings, classify peer roles, compare peer entries,
choose winners, decide copy sources, delete files, move paths to `BAK/`, read or
write snapshot rows, or print diagnostics. It receives accepted relative exclude
paths and live entry classification facts from TreeTraversal or its parent
facade, and it returns exclusion and eligibility facts.

The parent TreeTraversal facade applies this child before returning entries,
skips, and recursion intents. LiveDirectoryWalk or equivalent walk code owns the
combined directory listing facts and traversal order. Snapshot code owns actual
snapshot lookup and update. Outcome planners own copy, delete, displacement,
and no-op decisions for paths this child leaves visible.

ExcludedPathFilter must preserve these invariants for every run:

- every accepted command-line exclude hides its matching path;
- command-line excludes that match files hide only that file;
- command-line excludes that match directories hide the directory and all
  descendants;
- `.kitchensync` directories and `.git` directories are always hidden with all
  descendants;
- symbolic link files, symbolic link directories, and special files are always
  hidden;
- built-in exclusions cannot be overridden by command-line excludes;
- excluded paths are never eligible for scan, recursion, sync decision, copy,
  deletion, displacement, snapshot lookup, or snapshot update;
- non-excluded results only mean the path may continue to the next planner, not
  that any mutation should occur.
