# CommandAndOutput:

## Purpose

CommandAndOutput owns the command-facing surface of KitchenSync. It turns the
native executable invocation into a validated run request, normalizes peer
identities for comparison and lookup, and provides the single stdout-only output
path used by argument handling, sync execution, and shutdown.

The root product exposes this behavior through the `kitchensync` command. This
child specifies the argument meaning and observable output for that command.
Release assembly and the contents of `./released/` remain outside this child.

## Responsibilities

CommandAndOutput exposes an operation that parses the raw process argument list,
current working directory, and current operating-system username. With no
arguments, it returns the exact help screen from inside the fenced block in
`specs/help.md`, exit code `0`, and no stderr output. With arguments, it
validates the non-help invocation and either returns a complete run request or
returns a validation failure containing one error message followed by that same
help screen, exit code `1`, and no stderr output.

Non-help invocation parsing treats the command shape as:

```text
kitchensync [options] <peer> <peer> [<peer>...]
```

Options form the leading segment of the invocation. Peer operands follow the
options, must number at least two, and may include any number of additional
peers after the first two. The accepted run request preserves all accepted peer
operands in command-line order.

The parser accepts only the specified global options:

- `--dry-run` with no value.
- `--max-copies N`, `--retries-copy N`, `--retries-list N`,
  `--timeout-conn N`, `--timeout-idle N`, `--keep-tmp-days N`,
  `--keep-bak-days N`, and `--keep-del-days N`, where `N` is a positive
  integer.
- `--verbosity error`, `--verbosity info`, `--verbosity debug`, or
  `--verbosity trace`.
- Repeated `-x RELPATH` arguments, where each value is a relative slash path
  with no leading slash, trailing slash, backslash separator, empty segment,
  `.` segment, `..` segment, or NUL character.

The parser rejects fewer than two peer operands, more than one canon peer,
unrecognized flags, missing values for valued options, invalid positive integer
values, invalid verbosity values, invalid exclude paths, `max-copies` as a URL
query parameter, and any URL query parameter other than `timeout-conn` or
`timeout-idle`. URL query parameters `timeout-conn` and `timeout-idle` are valid
only when their values are positive integers.

The run request records the parsed global settings with these defaults:

- `dry-run` defaults to off.
- `max-copies` defaults to `10`.
- `retries-copy` defaults to `3`.
- `retries-list` defaults to `3`.
- `timeout-conn` defaults to `30` seconds.
- `timeout-idle` defaults to `30` seconds.
- `verbosity` defaults to `info`.
- `keep-tmp-days` defaults to `2`.
- `keep-bak-days` defaults to `90`.
- `keep-del-days` defaults to `180`.
- Command-line excludes default to an empty list and preserve all accepted
  values in command-line order.

The parser accepts peer operands that are local paths, `file://` URLs, or SFTP
URLs. A peer may have a leading `+` role marker for canon, a leading `-` role
marker for subordinate, or no marker for normal bidirectional sync. A bracketed
comma-separated peer operand is one peer with multiple fallback URLs,
preserving the URL order from the command line. A leading `+` or `-` before a
bracketed operand applies to the whole fallback peer, not to individual URLs
inside the brackets. At most one peer may be canon, and multiple peers may be
subordinate.

For each accepted URL, the parsed request keeps URL-level connection settings.
`timeout-conn` overrides the global connection timeout for that URL only.
`timeout-idle` overrides the global idle keep-alive time for that URL only.
`max-copies` is never a per-URL setting.

For local peers, CommandAndOutput parses bare paths with no URL scheme as local
file peers, including Unix absolute paths, Windows drive paths, and relative
paths. It also accepts `file://` peer URLs as local file peers. For SFTP peers,
it parses:

- `sftp://user@host/path` with the named user and port `22`.
- `sftp://user@host:port/path` with the named user and named port.
- `sftp://host/path` with the current operating-system user.
- `sftp://user:password@host/path` with the inline password.

Percent-encoded `@` and `:` characters in an SFTP password are decoded as
password characters. The SFTP path is parsed as an absolute path from the remote
filesystem root.

CommandAndOutput exposes an operation that normalizes each peer URL identity
before peer comparison or lookup. Normalization converts bare paths and Windows
drive paths to `file://` URLs, resolves relative local paths to absolute
`file://` URLs from the process current working directory, lowercases schemes
and hostnames, removes SFTP default port `22`, preserves non-default SFTP ports,
collapses consecutive path slashes, removes trailing path slashes, decodes
percent-encoded unreserved path characters, leaves percent-encoded reserved path
characters encoded, strips query strings, inserts the current operating-system
user for SFTP URLs with no username, and preserves explicit SFTP usernames.

CommandAndOutput exposes one output sink used by the rest of the product. The
sink writes all messages to stdout and never writes to stderr. It is line based
and does not inspect whether stdout is a terminal, so terminal output and
redirected output are identical. Callers pass already-decided sync events to the
sink; this child formats and filters them according to the parsed verbosity.

The output sink provides these message forms:

- Argument validation failures: one error message followed by the exact help
  text.
- First-sync failure: `First sync? Mark the authoritative peer with a leading +`.
- No-contributing-peer failure:
  `No contributing peer reachable - cannot make sync decisions`.
- Error-level diagnostics for every error condition named by `specs/sync.md`,
  including no snapshots and no canon, unreachable peers, canon peer
  unreachable, fewer than two reachable peers, no contributing peer reachable,
  listing failures, transfer failures before and after SWAP `old` exists,
  archive failures, displacement failures, TMP or SWAP staging failures,
  modification-time failures, and snapshot upload failures before and after
  SWAP `old` exists.
- Failed file-transfer diagnostics that identify the slash-separated relative
  path, destination peer URL, failed phase, and transport error category when
  available. The phase label must be one of `read_source`, `write_swap_new`,
  `move_existing_to_swap_old`, `rename_final`, `set_mod_time`, `archive_old`,
  or `cleanup`.
- Progress lines `C <relpath>` and `X <relpath>`, emitted in action order.
  A copied file path emits one `C` line no matter how many destination peers
  receive it. A displaced file or directory emits one `X` line no matter how
  many peers displace it.
- Trace copy-slot lines exactly as `copy-slots active=<n>/<max>`.
- A final completion message for a successful sync execution: exactly
  `sync complete`.

Verbosity is cumulative in this order: `error`, `info`, `debug`, `trace`.
`C` and `X` progress lines are info-level output and are suppressed by
`--verbosity error`. No debug-specific messages are defined, so debug output is
observably the same as info output. Trace output includes copy-slot acquire and
release events, and those events report global active file-copy slots rather
than network connection counts. The `sync complete` completion line is emitted
exactly once on a successful sync and is emitted at every verbosity level.

## Boundaries

CommandAndOutput does not connect to peers, choose a fallback URL winner, create
peer directories, authenticate SFTP sessions, list directories, make sync
decisions, mutate files, update snapshots, upload snapshots, or enforce copy
slot concurrency. Those children call this child with facts about actions,
failures, active copy slots, and completion, and this child formats the
user-visible stdout lines.

CommandAndOutput does not decide whether a file should be copied or a path
should be displaced. It only guarantees that when another child reports one
logical copy progress action for a relative path, one `C <relpath>` info line is
available, and when another child reports one logical displacement progress
action for a relative path, one `X <relpath>` info line is available. It must
not emit `C` or `X` progress lines for directory creation, directory listing,
snapshot work, or BAK, TMP, or SWAP cleanup.

CommandAndOutput does not own release packaging mechanics or decide which file
is shipped. Its command contract is that the root product exposes this behavior
through the `kitchensync` CLI.

The invariant for all operations is stdout-only output: argument parsing, sync
execution, and shutdown leave stderr empty. The output stream contains plain
lines only. It contains no live status screen, progress bar, percentage,
scanned-directory indicator, terminal control sequence, or terminal-dependent
formatting.
