# CommandLine:

## Purpose

CommandLine owns the released command-line surface for KitchenSync. It turns
process arguments into a validated run request, provides the exact help and
validation-error behavior, and owns the stdout-only process output rules that
are independent of sync decisions.

The root runner uses this child first. With no arguments, CommandLine reports a
help result that prints the help screen from `specs/help.md` verbatim to stdout
and exits 0. With arguments, it either returns one validation error that must be
printed before the same help screen and exit 1, or returns a validated sync
request for the rest of the product.

## Responsibilities

The released product contains exactly one shipped file under `released/`:
`released/kitchensync.exe`. That file is directly invocable as the KitchenSync
CLI and accepts non-help invocations in this shape:

```text
kitchensync [options] <peer> <peer> [<peer>...]
```

CommandLine accepts a non-help invocation only when it has at least two peer
arguments and at most one canon peer. It accepts normal peers, canon peers with
a leading `+`, and subordinate peers with a leading `-`. Multiple subordinate
peers are valid.

CommandLine parses each peer argument into one peer descriptor. A descriptor has
one or more URL alternatives in the order supplied by the user, and one peer
role: normal, canon, or subordinate. Bracketed fallback peer arguments in
`[url1,url2,...]`, `+[url1,url2,...]`, and `-[url1,url2,...]` forms are one
peer each; the prefix applies to the whole bracketed group, not to individual
URLs inside it.

CommandLine accepts peer URL text in these forms:

- Bare local paths with no URL scheme, including `/path`, `c:\path`, and
  `./relative`; these are treated as `file://` URLs for downstream work.
- SFTP URLs in `sftp://user@host/path`,
  `sftp://user@host:port/path`, `sftp://host/path`, and
  `sftp://user:password@host/path` forms.
- SFTP passwords containing percent-encoded `@` and `:` characters.

SFTP peer paths identify absolute paths from the remote filesystem root.
CommandLine validates URL query parameter names. Only `timeout-conn` and
`timeout-idle` are accepted as per-URL settings; every other query parameter
name is a command-line validation error. Accepted per-URL settings are carried
with the URL alternative for connection setup.

CommandLine accepts these global options and default values:

- `--dry-run`, default off.
- `--max-copies N`, positive integer, default 10.
- `--retries-copy N`, positive integer, default 3.
- `--retries-list N`, positive integer, default 3.
- `--timeout-conn N`, positive integer seconds, default 30.
- `--timeout-idle N`, positive integer seconds, default 30.
- `--verbosity LEVEL`, where `LEVEL` is `error`, `info`, `debug`, or `trace`,
  default `info`.
- `-x RELPATH`, repeatable.
- `--keep-tmp-days N`, positive integer, default 2.
- `--keep-bak-days N`, positive integer, default 90.
- `--keep-del-days N`, positive integer, default 180.

CommandLine rejects unrecognized flags and invalid option values. A positive
integer option value is valid only when it is an integer greater than zero.

Each `-x` value is a slash-separated relative path. CommandLine accepts a value
only when it has no leading `/`, no trailing `/`, no backslash separator, no
empty path segment, no `.` segment, and no `..` segment. Accepted excludes are
returned exactly as relative slash paths for traversal to apply.

CommandLine owns process output channels for its surface:

- Every line it prints goes to stdout.
- It never writes to stderr.
- No-argument help exits 0.
- Command-line validation errors exit 1.
- A validation error prints one error message followed by the help screen to
  stdout.
- A successful sync completion prints exactly one `sync complete` line to
  stdout and exits 0.

CommandLine also owns common verbosity gating for product output. Error-level
diagnostics are emitted at `error`, `info`, `debug`, and `trace`. Each higher
verbosity includes all messages from lower verbosity levels. With the current
specification, `debug` has the same observable output as `info`.

## Boundaries

CommandLine does not connect to peers, choose a reachable fallback URL, create
peer roots, authenticate to SFTP, inspect snapshot history, decide first-sync
validity, run sync traversal, copy files, mutate peers, update snapshot
databases, or generate operation-specific progress lines. It only parses and
validates the command line, carries accepted settings and peer descriptors
across its boundary, and provides output helpers for help, validation failures,
verbosity checks, and successful completion.

CommandLine does not own URL identity normalization for snapshot lookup. It may
classify a bare local path as a `file://` URL for parsing, but resolving paths,
lowercasing hosts, removing default ports, stripping query settings from
identity, inserting the current OS user, and other persistent identity rules
belong outside this child.

CommandLine must not require any third-party package. If later implementation
uses an argument parser or URL parser, that package must be justified by a plan
document before being declared.

Its invariants are:

- A parse result is either help, one validation error, or one complete run
  request.
- A complete run request has at least two peer descriptors and at most one
  canon peer.
- Every numeric option in a complete run request is a positive integer and has
  the documented default when omitted.
- Every exclude in a complete run request is a valid relative slash path.
- Every URL query parameter in a complete run request is one of the two
  accepted per-URL settings.
- stderr remains empty for help, validation, successful completion, and any
  output helper owned by this child.
