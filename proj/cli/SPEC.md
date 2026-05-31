# cli:

## Purpose

Define the public command-line interface for the native `kitchensync`
executable. The module interprets process arguments for:

```text
kitchensync [options] <peer> <peer> [<peer>...]
```

It owns help selection, verbatim help text, argument validation, peer operand
syntax, fallback URL grouping syntax, per-URL query setting syntax,
command-line exclude syntax, option defaults, and conversion of a valid
non-help invocation into the typed run request consumed by root orchestration.

The CLI module is limited to invocation interpretation. It does not connect to
peers, inspect peer filesystems, read or write snapshots, decide sync outcomes,
schedule transfers, execute file operations, or render sync progress.

## Responsibilities

- Recognize the no-argument invocation as the only help invocation. It must
  produce the exact help text defined by `specs/help.md`, request exit status
  `0`, and produce no stderr output.
- Parse non-help invocations in the form `kitchensync [options] <peer> <peer>
  [<peer>...]`, allowing options before peers and allowing repeated `-x
  <relative-path>` excludes after peer operands.
- Reject every non-help invocation with fewer than two peer operands.
- Reject every non-help invocation with more than one canon peer prefix.
- Reject unrecognized flags, omitted values for value-taking options, invalid
  option values, invalid peer operands, invalid fallback groups, invalid
  per-URL query parameters, and invalid exclude paths.
- For any non-help argument validation error, produce one specific validation
  error message followed by the exact help text, request exit status `1`, and
  produce no stderr output.
- Accept `--dry-run` as a value-less global flag.
- Accept these global value options only with positive integer values:
  `--max-copies`, `--retries-copy`, `--retries-list`, `--timeout-conn`,
  `--timeout-idle`, `--keep-tmp-days`, `--keep-bak-days`, and
  `--keep-del-days`.
- Apply these defaults when the corresponding option is omitted:
  `--max-copies 10`, `--retries-copy 3`, `--retries-list 3`,
  `--timeout-conn 30`, `--timeout-idle 30`, `--verbosity info`,
  `--keep-tmp-days 2`, `--keep-bak-days 90`, and `--keep-del-days 180`.
- Accept `--verbosity` only with `error`, `info`, `debug`, or `trace`.
- Accept repeatable `-x <relative-path>` excludes. Each exclude path must be a
  slash-separated relative path with no leading slash, no trailing slash, no
  backslash separators, no empty segment, no `.` segment, no `..` segment, and
  no NUL character.
- Parse each peer operand as one logical peer with one role: normal, canon
  (`+` prefix), or subordinate (`-` prefix).
- Accept bare local paths, `file://` URLs, and `sftp://` URLs as peer URLs.
  Bare paths include absolute Unix paths, Windows drive paths, and relative
  paths, and are represented in the run request as local `file://` peer URLs.
- Accept SFTP peer URLs in these forms:
  `sftp://user@host/path`, `sftp://user@host:port/path`,
  `sftp://host/path`, and `sftp://user:password@host/path`.
- Preserve inline SFTP password percent-encoding needed to represent reserved
  characters such as `%40` for `@` and `%3A` for `:`.
- Treat SFTP paths as absolute remote filesystem paths.
- Parse bracketed fallback groups, such as `[url1,url2,...]`, as one logical
  peer whose URLs are tried later by peer startup in the supplied order.
- Apply `+` or `-` role prefixes to the entire bracketed fallback group when
  the prefix appears before `[`.
- Reject `+` or `-` prefixes on individual URLs inside a bracketed fallback
  group.
- Accept bare local paths and Windows drive paths inside fallback groups as
  local `file://` fallback URLs.
- Parse URL query parameters only as per-URL settings. The only valid per-URL
  query parameters are `timeout-conn` and `timeout-idle`, and each must have a
  positive integer value.
- Reject any URL query parameter other than `timeout-conn` and `timeout-idle`;
  in particular, reject `max-copies` in a URL query string.
- Associate per-URL query settings only with the URL on which they appear,
  including URLs inside fallback groups.
- Normalize each parsed URL identity before including it in the run request:
  lowercase scheme and hostname, remove default SFTP port `22`, collapse
  consecutive slashes in the path, remove a trailing path slash, convert bare
  paths to `file://`, resolve `file://` paths to absolute paths from the
  process current working directory, percent-decode unreserved characters,
  strip query-string parameters from identity, and insert the current OS user
  into SFTP URLs that omit a username.
- Produce a typed successful parse result containing peer specs, each peer's
  ordered URL candidates and role, command-line excludes, dry-run mode, copy
  and list retry limits, connection and idle timeout defaults, retention-day
  values, and verbosity.
- Keep option and peer parsing deterministic and independent of connection
  success, snapshot existence, filesystem state, terminal interactivity, or
  sync traversal results.

## Boundaries

- The CLI module owns the exact help text and argument-validation envelope. It
  does not own runtime first-sync errors, no-contributing-peer errors,
  unreachable-peer diagnostics, listing diagnostics, transfer diagnostics,
  progress screens, or completion messages.
- The CLI module parses peer role prefixes and records the explicit role in the
  run request. It does not decide automatic subordination for snapshotless
  peers, apply canon authority to sync decisions, or determine whether a peer
  is reachable.
- The CLI module parses fallback URL order and per-URL timeout settings. It
  does not attempt connections, choose the winning fallback URL, create missing
  peer roots, authenticate SFTP sessions, verify known hosts, or keep SFTP
  sessions alive.
- The CLI module normalizes peer URL identity enough to produce stable
  `PeerUrl` values for the rest of the product. It does not implement transport
  filesystem operations or transport error categories.
- The CLI module validates command-line exclude path syntax and carries the
  accepted excludes in the run request. It does not apply built-in excludes,
  omit entries from traversal, protect excluded peer files, or preserve
  snapshot rows for excluded paths.
- The CLI module accepts `--dry-run` and records it in run configuration. It
  does not enforce read-only peer behavior, skip peer-side cleanup, exercise
  copy slots, read source files, or suppress snapshot uploads.
- The CLI module validates numeric limits and retention settings. It does not
  enforce active copy limits, retry copy work, retry directory listings, purge
  BAK/TMP entries, or purge deletion tombstones.
- The CLI module records verbosity. It does not filter progress events, draw
  live terminal output, emit trace copy-slot events, or choose interactive
  versus line-oriented rendering.
- The CLI module reports only parse-time outcomes: help, validation error, or a
  valid run request. Root orchestration owns invoking sibling modules after a
  valid request and mapping later product outcomes to process shutdown.

