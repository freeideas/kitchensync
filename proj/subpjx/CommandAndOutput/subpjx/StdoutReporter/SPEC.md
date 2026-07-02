# StdoutReporter:

## Purpose

StdoutReporter is the stdout-only reporting boundary for KitchenSync command
handling and sync execution. It receives already-decided reporting facts from
argument parsing, sync planning, transfer execution, copy-slot tracking, and
shutdown, then formats those facts as plain stdout lines.

This child does not decide whether a command is valid, whether a peer is
reachable, whether a path should be copied, or whether a path should be moved to
BAK. Its purpose is to make every user-visible line follow the same stream,
verbosity, ordering, and plain-text rules.

## Responsibilities

StdoutReporter exposes an output sink configured with a verbosity level:
`error`, `info`, `debug`, or `trace`. Verbosity is cumulative in this order:
`error`, `info`, `debug`, `trace`. Error-level diagnostics are visible at every
verbosity level. Info-level output is visible at `info`, `debug`, and `trace`.
No debug-only messages are defined, so `debug` produces the same observable
output as `info`. Trace-only output is visible only at `trace`.

StdoutReporter exposes an argument-validation report operation. For a non-help
argument validation failure, it writes the validation error message followed by
the help text from the fenced block in `specs/help.md` to stdout. The caller
supplies the already-selected error message and the exact help text.

StdoutReporter exposes startup and decision failure operations for these exact
lines:

- `First sync? Mark the authoritative peer with a leading +`
- `No contributing peer reachable - cannot make sync decisions`

StdoutReporter exposes an error diagnostic operation for each error condition
named by `specs/sync.md` section "Errors": argument errors, no snapshots and no
canon, unreachable peer, directory listing failure, canon peer unreachable,
fewer than two reachable peers, no contributing peer reachable, transfer
failure before SWAP `old` exists, transfer failure after SWAP `old` exists,
archive old failure, displacement failure, TMP or SWAP staging failure,
`set_mod_time` failure, snapshot upload failure before SWAP `old` exists, and
snapshot upload failure after SWAP `old` exists.

StdoutReporter exposes a failed file-transfer diagnostic operation. The caller
supplies the slash-separated relative path, destination peer URL, failed phase,
and optional transport error category. The diagnostic must include the relative
path, the destination peer URL, the phase, and the category when a category is
available. The failed phase must be one of:

- `read_source`
- `write_swap_new`
- `move_existing_to_swap_old`
- `rename_final`
- `set_mod_time`
- `archive_old`
- `cleanup`

StdoutReporter exposes progress operations for logical copy and displacement
actions. A copy progress action writes one info-level line:

```text
C <relpath>
```

A displacement progress action writes one info-level line:

```text
X <relpath>
```

In both forms, `<relpath>` is the slash-separated relative path from the sync
root. The action letter is followed by exactly one space. A copied file path
emits one `C` line no matter how many destination peers receive the file. A path
displaced to BAK emits one `X` line no matter how many peers displace it.
Displaced files and displaced directories both use the `X` form. These progress
lines are emitted in the order the actions are reported to StdoutReporter.

StdoutReporter exposes no progress operation for directory creation, directory
listing, snapshot work, or BAK, TMP, and SWAP cleanup. Those activities must not
produce `C` or `X` lines through this child.

StdoutReporter exposes a trace copy-slot operation for copy-slot acquire and
release events. At `trace` verbosity it writes exactly:

```text
copy-slots active=<n>/<max>
```

The `active` value is the global active file-copy slot count after the reported
event, and `max` is the global copy-slot limit. These values describe file-copy
slots, not network connection counts.

StdoutReporter exposes a completion operation for successful sync execution.
It writes one final completion message to stdout after the sync operation has
successfully completed. The line is exactly:

```text
sync complete
```

The completion line is emitted exactly once for a successful sync and is visible
at every verbosity level, including `error`.

## Boundaries

StdoutReporter writes only to stdout. It must never write to stderr, and it must
not duplicate diagnostics across stdout and stderr. Argument parsing, sync
execution, and shutdown must leave stderr empty when all user-visible output is
routed through this child.

StdoutReporter is line based. Each emitted record is a complete plain line on
stdout. It does not inspect whether stdout is a terminal, so terminal output and
redirected output are identical. It must not emit a live status screen, progress
bar, percentage, scanned-directory indicator, terminal control sequence, color
escape, cursor movement, or any other terminal-dependent formatting.

StdoutReporter does not parse arguments, load help text, normalize peer
identities, connect to peers, choose fallback URLs, list directories, compare
snapshots, decide copy or displacement actions, perform file transfer phases,
track retry counts, enforce copy-slot concurrency, mutate snapshots, or select
process exit codes. Callers provide the facts and the order in which reportable
events happened; StdoutReporter formats only the observable stdout lines.

StdoutReporter has no dependency on transport, database, hashing, URL, or file
metadata libraries. It should not declare third-party packages unless later
implementation work proves formatting cannot be done with the product language
standard library.
