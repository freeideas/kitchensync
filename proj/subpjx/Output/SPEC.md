# Output:

## Purpose

Output is the single channel through which KitchenSync emits everything a user
sees. Every other component hands its progress lines and diagnostics to Output,
and Output decides whether to print them based on the configured verbosity
level. It writes all of that text to standard output and never writes to
standard error.

It exists so that output discipline lives in one place: the rules for what
appears at each verbosity level, the exact shape of each line, and the guarantee
that stderr stays empty are enforced here rather than scattered across the
components that have something to report.

## Responsibilities

Output exposes a small set of emit operations across its boundary. Each
operation states the verbosity level at which its message belongs; Output emits
the message only when the configured level is at or above that threshold.

Verbosity model:

- Hold the run's verbosity level, one of `error`, `info`, `debug`, or `trace`,
  ordered least-to-most verbose as `error` < `info` < `debug` < `trace`.
- Treat the levels as cumulative: each level emits everything the lower levels
  emit plus its own additions (023.10).
- Treat `debug` as observationally identical to `info`; no message is defined
  that `debug` emits but `info` does not (023.13).

Progress lines (emitted at `info` or higher, 023.12):

- Emit exactly one `C <relpath>` line when a path has been copied to one or more
  peers, regardless of how many peers received it (023.7).
- Emit exactly one `X <relpath>` line when a path has been displaced or deleted
  on one or more peers, for both files and directories, regardless of how many
  peers (023.8).
- Format each progress line as the action letter, a single space, then the
  slash-separated relative path from the sync root (023.6).
- Emit progress lines in the order the actions happen, one plain line per action
  (023.3), and produce the same lines whether or not stdout is a terminal
  (023.4).
- Emit no progress line for directory creation, listing, snapshot work, or
  BAK/TMP cleanup (023.9).
- Emit no live status screen, progress bar, percentage, scanned-directory
  indicator, or terminal control sequence (023.5).

Diagnostics (emitted at `error` or higher, 023.11):

- Emit the enumerated error diagnostics and the nonfatal diagnostics for skipped
  peers and recoverable operation failures. The conditions that trigger these
  diagnostics live with their own behaviors; Output owns only the format and the
  verbosity gating.
- For a failed file transfer, emit a diagnostic that identifies the relative
  path, the destination peer URL, the failed phase, and the transport error
  category when one is available (023.16).
- Restrict the reported failed phase to one of `read_source`, `write_swap_new`,
  `move_existing_to_swap_old`, `rename_final`, `set_mod_time`, `archive_old`, or
  `cleanup` (023.17).

Trace events (emitted only at `trace`, 023.14):

- Emit each copy-slot acquire and release event as the line
  `copy-slots active=<n>/<max>` (023.15).

## Boundaries

Error obligations:

- Output does not itself fail in a way that callers must handle; emitting is a
  fire-and-forget operation from the caller's view. Output's obligation is to
  honor the verbosity threshold and the line format for every message it is
  given.

Invariants:

- All output Output produces is written to standard output (023.1).
- Output never writes to standard error; standard error remains empty across
  argument parsing, sync execution, and shutdown, so that a user running
  `2>/dev/null` misses nothing and a user running `2>&1` sees no duplicate lines
  (023.2).
- A message is emitted only when the configured verbosity level is at or above
  the level that message belongs to; messages below the threshold are silently
  dropped.
- The text of a progress line is identical whether or not stdout is a terminal;
  Output emits no terminal-specific control output.
- Output reports only what callers hand it. It does not decide which paths were
  copied or displaced, does not connect to peers, does not read snapshots, and
  does not own the meaning of any error condition; it owns only the channel, the
  verbosity gating, and the exact line formats.
- The completion message and the dry-run phrase are emitted by the components
  that own those behaviors; Output provides the channel and gating those
  emissions pass through, but the wording and the decision to emit them live
  with `006_run-lifecycle` and `024_dry-run`.

The operations Output exposes across its boundary are: configure the run's
verbosity level; emit a copy (`C`) progress line for a relative path; emit a
displace/delete (`X`) progress line for a relative path; emit an error or
nonfatal diagnostic; emit a failed-transfer diagnostic carrying the path,
destination peer URL, failed phase, and transport error category; and emit a
copy-slot trace event carrying the active and maximum slot counts. Each
operation's verbosity threshold and line format are the shape later jobs build
the interface, implementation, and tests against.
