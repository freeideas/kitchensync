# Cli:

## Purpose

Cli turns a raw command-line argument vector into a validated run
configuration, or rejects the invocation with an error and the help text.

It is the first thing the program runs. Its job is to decide whether the
arguments describe a valid run, to assemble the structured configuration the
rest of the program needs (peers with their prefixes and fallback groups,
per-URL settings, command-line excludes, and global option values), and to
render the verbatim help screen. Cli never connects to a peer, never normalizes
a URL into its canonical identity, and never touches a snapshot or a file. It
only reads the argument strings and the help text.

## Responsibilities

Cli exposes one primary operation across its boundary: given the argument
vector, produce either a validated run configuration or a validation failure
carrying an error message. A separate operation renders the verbatim help text
so callers can print it. Concretely, Cli does the following.

Argument intake and the help/no-argument case:

- With no arguments, report that the help text should be printed to standard
  output and that the program should exit 0 (001.1, 001.2).
- Provide the help text exactly as written in `specs/help.md`, character for
  character, so a caller can print it verbatim to standard output (002.1).
  The no-argument case prints only this text and leaves standard error empty
  (002.2, 002.3).

Peers and prefixes:

- Accept a bare peer path with no URL scheme (forms such as `/path`, `c:\path`,
  or `./relative`) as a local `file://` peer, and accept an `sftp://` URL as a
  peer (001.6, 001.7).
- Recognize a leading `+` as the canon peer, a leading `-` as a subordinate
  peer, and no prefix as a normal bidirectional peer, and record the role on
  each peer (001.9, 001.10, 001.11).
- Recognize square brackets as grouping several comma-separated URLs into a
  single peer (a fallback group), and apply a `+` or `-` prefix placed before a
  bracketed group to the whole group (001.14, 001.15).
- Accept multiple `-` peers in one invocation (001.13).

Per-URL settings:

- Accept the `timeout-conn` and `timeout-idle` query parameters on a peer URL
  and record their values as per-URL settings (001.16, 001.17).
- Carry each peer URL's settings through to the produced configuration without
  normalizing the URL's identity; canonical normalization is the responsibility
  of `003_url-normalization` in another component.

Global options:

- Recognize the global flags `--dry-run`, `--max-copies`, `--retries-copy`,
  `--retries-list`, `--timeout-conn`, `--timeout-idle`, `--verbosity`, `-x`,
  `--keep-tmp-days`, `--keep-bak-days`, and `--keep-del-days`, and place their
  values (or defaults) into the configuration (001.20).
- Accept `--verbosity` values `error`, `info`, `debug`, and `trace` (001.24).
- Accept `-x <relative-path>` and allow it to appear multiple times, collecting
  each accepted path into the configuration's exclude list (001.26, 001.27).

Validation failures (each prints the error message, then the help text, then
exits 1):

- Fewer than two peers (001.8).
- More than one `+` peer (001.12).
- A peer URL query parameter other than `timeout-conn` or `timeout-idle`
  (001.18), and specifically a `max-copies` query parameter on a peer URL,
  which is singled out as its own rejection (001.19).
- An unrecognized flag (001.21).
- A zero, negative, or non-integer value for any of `--max-copies`,
  `--retries-copy`, `--retries-list`, `--timeout-conn`, `--timeout-idle`,
  `--keep-tmp-days`, `--keep-bak-days`, or `--keep-del-days` (001.22, 001.23).
- A `--verbosity` value other than the four accepted words (001.25).
- An `-x` path that is invalid: a leading `/` (001.28), a trailing `/`
  (001.29), a `\` separator (001.30), an empty, `.`, or `..` segment (001.31),
  or a NUL character (001.32).

## Boundaries

Error obligations:

- On any validation error for a non-help invocation, Cli's result must carry
  the error message to be printed to standard output, must cause the help text
  to be printed to standard output after that message, and must cause exit 1
  (001.3, 001.4, 001.5). The same verbatim help text appended after the error
  message is required on every validation failure (002.4).
- The first validation error encountered is sufficient to reject the
  invocation; Cli is not required to report more than one.

Invariants:

- A successfully produced run configuration always contains at least two peers,
  at most one canon (`+`) peer, only recognized global flags, only positive
  integer values for the integer-valued options, a `--verbosity` that is one of
  the four accepted words, and only well-formed relative exclude paths.
- Cli does not write to standard error in any case; all of its output (help
  text and error messages) is destined for standard output.
- Cli is purely a parsing and acceptance/rejection component. It does not
  resolve, normalize, or connect URLs (`003_url-normalization`,
  `005_connection-establishment`, `022_transports`), does not interpret the
  meaning of excluded paths (`009_excludes`), and does not apply option values
  beyond recording them; the effect of each option is exercised by the
  component it governs.
- The help text Cli renders is a fixed, verbatim copy of `specs/help.md` and
  does not vary with the arguments.

The operations Cli exposes across its boundary are: parse an argument vector
into either a validated run configuration or a validation failure (with its
error message), and render the verbatim help text. The validated run
configuration it yields carries the peers (each with role and its ordered
fallback URLs and per-URL settings), the command-line excludes, and the global
option values, which is the shape later jobs build the interface, implementation,
and tests against.
