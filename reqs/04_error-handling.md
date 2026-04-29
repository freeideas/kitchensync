# 04_error-handling: Per-operation failures during a run

## Behavior

Several mid-run failure modes are documented to fail soft: log the error, skip the affected unit of work, and continue with the rest of the run. Failed listings exclude only the affected peer/subtree from decisions; failed transfers leave the destination unchanged; failed displacements leave the file in place; failed snapshot uploads leave the staging file behind for later cleanup. Derived from `./specs/sync.md` (`Errors`) and `./specs/multi-tree-sync.md` (`Listing errors`, `Offline Peers`).

## $REQ_IDs
- `04.21` — A file transfer that fails is logged and the affected entry is skipped — the rest of the run continues, and that file is re-discovered on the next run.
- `04.22` — When a transfer fails, the destination file at the final path is unchanged (no partial content appears at the target path; staging file is cleaned up).
- `04.23` — A displacement that cannot be performed (cannot rename to BAK/) is logged at error level and the file is left in place; if the displacement was part of a copy sequence, the copy is also skipped and its TMP staging is cleaned up.
- `04.24` — When `list_dir` fails for a specific path on an otherwise-reachable peer, the program logs the error and excludes that peer from decisions for that directory and its entire subtree, while continuing to use it elsewhere.
- `04.25` — When `list_dir` fails on a peer at a path, that peer's snapshot rows for the affected subtree are not modified by this run (no false-deletion inferred).
- `04.26` — A peer that is unreachable for the entire run is not used in decisions and has no snapshot rows altered by this run.
- `04.27` — A snapshot upload failure is logged and the staging file is left under `.kitchensync/TMP/` for cleanup after `--xd` days.
- `04.28` — A TMP staging failure (cannot create staging directory or write staging file) is treated as a transfer failure (per `04.21`/`04.22`).
