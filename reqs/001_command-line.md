# 001_command-line: Command-line parsing and validation

## Behavior
This concern derives from `specs/sync.md` sections "Command Line" (Peers,
Fallback URLs, Per-URL Settings, Command-Line Excludes, Global Options, URL
Schemes), "Startup" step 1, and `specs/concurrency.md` section "Fallback URLs"
(the `max-copies`-not-per-URL rule).

It covers how the argument vector is parsed into peers and options, and how
invalid invocations are rejected. Observable surface: which argument strings are
accepted, how peer prefixes (`+`, `-`), bracketed fallback groups, and per-URL
query parameters are recognized, the supported global flags and their defaults,
the slash-path format required for `-x`, and the rule that any validation error
on a non-help invocation prints the error message followed by the help text and
exits 1 (too few peers, more than one `+` peer, unrecognized flags,
non-positive integer option values, bad `--verbosity` value, an invalid `-x`
path, an unknown URL query parameter, or `max-copies` in a URL query string).

This category covers only parsing and acceptance/rejection. The verbatim help
text is `002_help-screen`. The meaning and effect of excluded paths is
`009_excludes`. URL identity normalization is `003_url-normalization`.

## $REQ_IDs

- `001.1` -- Running `kitchensync` with no arguments prints the help text to standard output.
- `001.2` -- Running `kitchensync` with no arguments exits 0.
- `001.3` -- A validation error on a non-help invocation prints the error message to standard output.
- `001.4` -- A validation error on a non-help invocation prints the help text to standard output after the error message.
- `001.5` -- A validation error on a non-help invocation exits 1.
- `001.6` -- A bare peer path with no URL scheme (including forms like `/path`, `c:\path`, or `./relative`) is accepted as a local `file://` peer.
- `001.7` -- An `sftp://` URL is accepted as a peer argument.
- `001.8` -- An invocation with fewer than two peers is rejected as a validation error.
- `001.9` -- A peer argument prefixed with `+` is accepted as the canon peer.
- `001.10` -- A peer argument prefixed with `-` is accepted as a subordinate peer.
- `001.11` -- A peer argument with no prefix is accepted as a normal bidirectional peer.
- `001.12` -- An invocation with more than one `+` peer is rejected as a validation error.
- `001.13` -- Multiple `-` peers in a single invocation are accepted.
- `001.14` -- Square brackets group multiple comma-separated URLs into a single peer.
- `001.15` -- A `+` or `-` prefix placed before a bracketed fallback group designates the whole group as that peer type.
- `001.16` -- A `timeout-conn` query parameter on a peer URL is accepted.
- `001.17` -- A `timeout-idle` query parameter on a peer URL is accepted.
- `001.18` -- A peer URL query parameter other than `timeout-conn` or `timeout-idle` is rejected as a validation error.
- `001.19` -- A `max-copies` query parameter on a peer URL is rejected as a validation error.
- `001.20` -- The option flags `--dry-run`, `--max-copies`, `--retries-copy`, `--retries-list`, `--timeout-conn`, `--timeout-idle`, `--verbosity`, `-x`, `--keep-tmp-days`, `--keep-bak-days`, and `--keep-del-days` are recognized and do not trigger an unrecognized-flag error.
- `001.21` -- An unrecognized flag is rejected as a validation error.
- `001.22` -- A zero or negative value for any of `--max-copies`, `--retries-copy`, `--retries-list`, `--timeout-conn`, `--timeout-idle`, `--keep-tmp-days`, `--keep-bak-days`, or `--keep-del-days` is rejected as a validation error.
- `001.23` -- A non-integer value for any of `--max-copies`, `--retries-copy`, `--retries-list`, `--timeout-conn`, `--timeout-idle`, `--keep-tmp-days`, `--keep-bak-days`, or `--keep-del-days` is rejected as a validation error.
- `001.24` -- `--verbosity` accepts the values `error`, `info`, `debug`, and `trace`.
- `001.25` -- A `--verbosity` value other than `error`, `info`, `debug`, or `trace` is rejected as a validation error.
- `001.26` -- `-x <relative-path>` is accepted.
- `001.27` -- Multiple `-x` flags in a single invocation are accepted.
- `001.28` -- An `-x` path with a leading `/` is rejected as a validation error.
- `001.29` -- An `-x` path with a trailing `/` is rejected as a validation error.
- `001.30` -- An `-x` path containing a `\` separator is rejected as a validation error.
- `001.31` -- An `-x` path containing an empty, `.`, or `..` segment is rejected as a validation error.
- `001.32` -- An `-x` path containing a NUL character is rejected as a validation error.

## Notes

- Default values for the global options are exercised through the behavior each
  option controls (concurrency limits, retry counts, timeouts, retention
  windows), so their effects are tested in those categories rather than here;
  this category asserts only that each flag is recognized and that its value is
  accepted or rejected.
- `001.19` (`max-copies` in a URL query string) is kept distinct from `001.18`
  (any other unknown URL query parameter) because the spec singles out
  `max-copies` as a separately mandated rejection.
