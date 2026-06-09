# 023_logging: Output channels, progress, and diagnostics

## Behavior
This concern derives from `specs/sync.md` section "Logging" and
`specs/concurrency.md` sections "Progress Output" and "Trace Logging".

It covers the output discipline: all output goes to stdout and stderr stays
empty across parsing, execution, and shutdown (a user running `2>/dev/null`
misses nothing; `2>&1` sees no duplicates). It covers the per-action progress
lines emitted in action order, identical whether or not stdout is a terminal:
`C <relpath>` when a path is copied to one or more peers (one line per path) and
`X <relpath>` when a path is displaced/deleted on one or more peers (one line per
path, files and directories alike), with no line for directory creation,
listing, snapshot work, or BAK/TMP cleanup. It covers the cumulative verbosity
levels (`error` < `info` < `debug` < `trace`) and what each emits - `error` for
the enumerated error and nonfatal-skip diagnostics, `info` for the `C`/`X` lines,
`trace` for `copy-slots active=<n>/<max>` slot acquire/release events, with
`debug` observationally identical to `info`. It covers the failed-transfer
diagnostic format identifying the relative path, destination peer URL, failed
phase (one of the enumerated phases), and transport error category.

The error conditions that produce these diagnostics live with their behaviors
(for example `006_run-lifecycle`, `019_swap-replacement`, `020_copy-execution`).
The completion message is emitted at the end of `006_run-lifecycle`.

## $REQ_IDs

- `023.1` -- All output produced by KitchenSync is written to stdout.
- `023.2` -- stderr remains empty across argument parsing, sync execution, and shutdown.
- `023.3` -- During sync execution, KitchenSync emits one plain line per action to stdout, in the order the actions happen.
- `023.4` -- The progress output is identical whether or not stdout is a terminal.
- `023.5` -- KitchenSync emits no live status screen, progress bar, percentage, scanned-directory indicator, or terminal control sequence.
- `023.6` -- Each progress line is an action letter, a single space, then the slash-separated relative path from the sync root.
- `023.7` -- A path copied to one or more peers produces exactly one `C <relpath>` line, regardless of how many peers receive it.
- `023.8` -- A path displaced or deleted on one or more peers produces exactly one `X <relpath>` line, regardless of how many peers, for both files and directories.
- `023.9` -- No progress line is emitted for directory creation, listing, snapshot work, or BAK/TMP cleanup.
- `023.10` -- Verbosity levels are cumulative in the order `error` < `info` < `debug` < `trace`: each level emits everything the lower levels emit plus its own additions.
- `023.11` -- At verbosity `error`, KitchenSync emits the enumerated error diagnostics and the nonfatal diagnostics for skipped peers and recoverable operation failures.
- `023.12` -- The `C`/`X` progress lines are emitted at verbosity `info` or higher.
- `023.13` -- `--verbosity debug` produces output observationally identical to `--verbosity info`.
- `023.14` -- Copy-slot acquire and release events are emitted only at verbosity `trace`.
- `023.15` -- Each copy-slot acquire/release event is emitted as `copy-slots active=<n>/<max>`.
- `023.16` -- A failed file-transfer diagnostic identifies the relative path, the destination peer URL, the failed phase, and the transport error category when available.
- `023.17` -- The failed phase in a transfer diagnostic is one of `read_source`, `write_swap_new`, `move_existing_to_swap_old`, `rename_final`, `set_mod_time`, `archive_old`, or `cleanup`.
