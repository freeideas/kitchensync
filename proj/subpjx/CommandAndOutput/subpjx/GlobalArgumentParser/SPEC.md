# GlobalArgumentParser:

## Purpose

GlobalArgumentParser owns the global command-line phase of KitchenSync. It
handles the no-argument help case, validates and parses options that appear
before peer operands, records global run settings and command-line excludes,
and returns the validation-failure shape used before sync startup can begin.

This child does not parse peer URL forms, peer role markers, fallback groups,
or per-URL query settings. Its successful non-help result is the parsed global
settings plus the remaining peer operand strings, in the same order they were
supplied, for the peer argument parser owned by a sibling.

## Responsibilities

GlobalArgumentParser exposes an operation that accepts the process argument
list after the executable name and the exact fenced help screen text from
`specs/help.md`.

When the argument list is empty, the operation returns a help result with:

- stdout equal to the fenced help screen text verbatim;
- exit code `0`;
- an empty stderr value;
- no run request.

For a non-help invocation, GlobalArgumentParser scans only the options placed
before peer operands. It accepts these global options:

- `--dry-run`, with no value;
- `--max-copies N`;
- `--retries-copy N`;
- `--retries-list N`;
- `--timeout-conn N`;
- `--timeout-idle N`;
- `--keep-tmp-days N`;
- `--keep-bak-days N`;
- `--keep-del-days N`;
- `--verbosity LEVEL`;
- repeated `-x RELPATH`.

The valued numeric options require a value and accept only positive integers:
`--max-copies`, `--retries-copy`, `--retries-list`, `--timeout-conn`,
`--timeout-idle`, `--keep-tmp-days`, `--keep-bak-days`, and
`--keep-del-days`. Zero, negative numbers, empty strings, fractional numbers,
and non-numeric strings are invalid.

The `--verbosity` option requires a value and accepts only `error`, `info`,
`debug`, or `trace`.

Each `-x` option requires one exclude value. The value is parsed as a
slash-separated relative path. It is invalid when it has a leading `/`, a
trailing `/`, a backslash separator, an empty path segment, a `.` path segment,
a `..` path segment, or a NUL character. Repeated `-x` options append repeated
exclude paths to the run settings in command-line order.

The successful global settings use these defaults when an option is absent:

- read-only planning mode is off;
- maximum concurrent copies is `10`;
- copy tries before giving up is `3`;
- listing tries before giving up is `3`;
- default SSH handshake timeout is `30` seconds;
- default SFTP idle keep-alive time is `30` seconds;
- verbosity is `info`;
- stale TMP staging deletion age is `2` days;
- displaced-file deletion age is `90` days;
- deletion-record retention age is `180` days;
- command-line excludes are an empty list.

GlobalArgumentParser returns the unparsed peer operand list after the leading
global options. Peer operands begin at the first argument that is not a global
option being consumed by this child. After the peer operand list has begun,
remaining arguments are peer operand strings rather than global options, so a
global option placed after a peer is not applied to the run settings. This
preserves the product command shape: options first, then peers.

In the leading option segment, any `--` option name other than the accepted
global options is an unrecognized flag. `-x` is the only single-hyphen option
this child consumes.

A non-help invocation fails global validation when:

- an unrecognized flag appears in the global option area;
- a valued global option has no following value;
- a valued numeric option value is not a positive integer;
- `--verbosity` has any value other than `error`, `info`, `debug`, or `trace`;
- `-x` has no value;
- an `-x` value is not a valid relative slash path.

On any non-help validation failure owned by this child, the operation returns a
failure result with:

- stdout equal to one error message followed by the fenced help screen text;
- exit code `1`;
- an empty stderr value;
- no run request.

The error message must be plain text and must identify the invalid argument
well enough for the caller to know which global validation rule failed. The
exact wording is not specified beyond preceding the help text.

## Boundaries

GlobalArgumentParser does not read `specs/help.md` itself. The caller supplies
the exact help text so this child can return it verbatim for no-argument help
and validation failures.

GlobalArgumentParser does not print to stdout or stderr and does not terminate
the process. It returns structured command outcomes containing the stdout text,
exit code, and empty stderr value for the command facade to apply.

GlobalArgumentParser does not decide whether enough peer operands were
supplied, whether more than one canon peer was supplied, whether peer URLs are
valid, whether URL query parameters are valid, or how local and SFTP peer
forms are interpreted. Those checks belong to peer parsing. This child only
separates global options from the remaining peer operand strings.

GlobalArgumentParser does not normalize URLs, inspect the current working
directory, connect to peers, start sync work, enforce copy concurrency, apply
retry limits, clean BAK/TMP data, or filter tree traversal with excludes. It
only records the parsed settings that later children use.

## Invariants

- No arguments always produce the exact help text on stdout, exit code `0`,
  and empty stderr.
- Non-help global validation failures always produce one error message followed
  by the exact help text on stdout, exit code `1`, and empty stderr.
- Successful non-help parsing never writes output and never changes stderr.
- Only the documented global options are accepted in the global option area.
- Global options are parsed before peer operands, and peer operand order is
  preserved.
- Positive-integer settings are stored as positive integer values.
- Verbosity is stored as one of `error`, `info`, `debug`, or `trace`.
- Command-line excludes are stored as valid relative slash paths in the order
  accepted from repeated `-x` arguments.
