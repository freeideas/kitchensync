# 002_help-and-argument-validation: Help text and invalid argument handling

## Behavior
This concern derives from `specs/help.md` section "Help Screen" and
`specs/sync.md` sections "Command Line" and "Startup". It covers the exact help
screen, no-argument behavior, non-help validation failures, accepted option value
forms, invalid URL query parameters, and the exit codes and stdout-only output
for those command-line outcomes.

## $REQ_IDs
- `002.1` -- Running `kitchensync` with no arguments prints the help text from the fenced block in `specs/help.md` verbatim to stdout.
- `002.2` -- Running `kitchensync` with no arguments exits 0.
- `002.3` -- Running `kitchensync` with no arguments leaves stderr empty.
- `002.4` -- Any non-help invocation with fewer than two peer arguments fails validation.
- `002.5` -- Any non-help invocation with more than one `+` peer fails validation.
- `002.6` -- Any non-help invocation with an unrecognized flag fails validation.
- `002.7` -- Non-help invocations accept `--dry-run` without a value.
- `002.8` -- Non-help invocations accept `--max-copies N` when `N` is a positive integer.
- `002.9` -- Non-help invocations accept `--retries-copy N` when `N` is a positive integer.
- `002.10` -- Non-help invocations accept `--retries-list N` when `N` is a positive integer.
- `002.11` -- Non-help invocations accept `--timeout-conn N` when `N` is a positive integer.
- `002.12` -- Non-help invocations accept `--timeout-idle N` when `N` is a positive integer.
- `002.13` -- Non-help invocations accept `--keep-tmp-days N` when `N` is a positive integer.
- `002.14` -- Non-help invocations accept `--keep-bak-days N` when `N` is a positive integer.
- `002.15` -- Non-help invocations accept `--keep-del-days N` when `N` is a positive integer.
- `002.16` -- Non-help invocations reject `--max-copies` when its value is not a positive integer.
- `002.17` -- Non-help invocations reject `--retries-copy` when its value is not a positive integer.
- `002.18` -- Non-help invocations reject `--retries-list` when its value is not a positive integer.
- `002.19` -- Non-help invocations reject `--timeout-conn` when its value is not a positive integer.
- `002.20` -- Non-help invocations reject `--timeout-idle` when its value is not a positive integer.
- `002.21` -- Non-help invocations reject `--keep-tmp-days` when its value is not a positive integer.
- `002.22` -- Non-help invocations reject `--keep-bak-days` when its value is not a positive integer.
- `002.23` -- Non-help invocations reject `--keep-del-days` when its value is not a positive integer.
- `002.24` -- Non-help invocations accept `--verbosity error`.
- `002.25` -- Non-help invocations accept `--verbosity info`.
- `002.26` -- Non-help invocations accept `--verbosity debug`.
- `002.27` -- Non-help invocations accept `--verbosity trace`.
- `002.28` -- Non-help invocations reject `--verbosity` with any other value.
- `002.29` -- Non-help invocations accept repeated `-x RELPATH` arguments when each `RELPATH` is a valid relative slash path.
- `002.30` -- Non-help invocations reject `-x` when its value has a leading `/`.
- `002.31` -- Non-help invocations reject `-x` when its value has a trailing `/`.
- `002.32` -- Non-help invocations reject `-x` when its value contains a `\` separator.
- `002.33` -- Non-help invocations reject `-x` when its value contains an empty path segment.
- `002.34` -- Non-help invocations reject `-x` when its value contains a `.` path segment.
- `002.35` -- Non-help invocations reject `-x` when its value contains a `..` path segment.
- `002.36` -- Non-help invocations accept URL query parameter `timeout-conn=N` on peer URLs when `N` is a positive integer.
- `002.37` -- Non-help invocations accept URL query parameter `timeout-idle=N` on peer URLs when `N` is a positive integer.
- `002.38` -- Non-help invocations accept a peer URL query string combining `timeout-conn=N` and `timeout-idle=M` when both values are positive integers.
- `002.39` -- Non-help invocations reject any URL query parameter other than `timeout-conn` or `timeout-idle`.
- `002.40` -- URL query parameter `timeout-conn` rejects a value that is not a positive integer.
- `002.41` -- URL query parameter `timeout-idle` rejects a value that is not a positive integer.
- `002.42` -- A command-line option that requires a value fails validation when no value is provided.
- `002.43` -- On any non-help validation failure, `kitchensync` prints an error message followed by the help text from the fenced block in `specs/help.md` to stdout.
- `002.44` -- On any non-help validation failure, `kitchensync` exits 1.
- `002.45` -- On any non-help validation failure, `kitchensync` leaves stderr empty.

## Notes
This file covers whether command input is accepted or rejected. The normalized
meaning of accepted peers and URLs belongs to `003_peer-arguments.md` and
`004_url-normalization.md`.

Operating systems do not pass NUL bytes inside command-line arguments, so the
NUL-byte exclude-path rule is not listed as a separate CLI-testable requirement
here.
